//! Video decoding along a sampling plan (ALGORITHMS §0, SPEC §9.3, P1.2/P1.4).

use crate::ffi::{check, q2d, ts_to_us, us_to_ts, Decoder, Frame, Input, MediaError, Packet, Result, EAGAIN, EOF};
use crate::probe::MediaInfo;
use clippro_measure::image::{PlaneData, PlaneRef};
use clippro_types::{FrameRate, SampleRate};
use ffmpeg_sys_next as ff;
use std::path::Path;
use std::ptr;

/// Sample times (µs relative to the first frame) for a clip (ALGORITHMS §0.1).
pub fn sample_plan(duration_us: i64, fps: FrameRate, rate: SampleRate) -> Vec<i64> {
    let frame_us = fps.frame_duration_us();
    if duration_us <= 0 {
        return vec![0];
    }
    let nframes = ((duration_us as f64 / frame_us).round() as i64).max(1);
    let last = fps.frames_to_us(nframes - 1);
    match rate.hz() {
        None => (0..nframes).map(|k| fps.frames_to_us(k)).collect(),
        Some(r) => {
            let step = 1e6 / r;
            let mut v: Vec<i64> = (0..).map(|k| (k as f64 * step).round() as i64).take_while(|t| *t <= last).collect();
            if v.is_empty() {
                v.push(0);
            }
            if (last - *v.last().unwrap_or(&0)) as f64 > 0.5 * frame_us {
                v.push(last);
            }
            v
        }
    }
}

/// Hardware decode preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HwMode {
    /// D3D11VA when the codec/profile is supported, otherwise CPU.
    #[default]
    Auto,
    Off,
}

/// D3D11VA decodes this stream: the decoder offers a D3D11VA device config, and the format is one
/// DXVA profiles cover (4:2:0; 8-bit for H.264, up to 10-bit for HEVC/VP9/AV1).
pub fn hw_decodable(codec: ff::AVCodecID, info: &MediaInfo) -> bool {
    let Some(v) = &info.video else { return false };
    let max_bits = match codec {
        ff::AVCodecID::AV_CODEC_ID_H264 | ff::AVCodecID::AV_CODEC_ID_MPEG2VIDEO | ff::AVCodecID::AV_CODEC_ID_VC1 => 8,
        ff::AVCodecID::AV_CODEC_ID_HEVC | ff::AVCodecID::AV_CODEC_ID_VP9 | ff::AVCodecID::AV_CODEC_ID_AV1 => 10,
        _ => return false,
    };
    if v.chroma != "4:2:0" || v.bit_depth > max_bits {
        return false;
    }
    unsafe {
        let dec = ff::avcodec_find_decoder(codec);
        if dec.is_null() {
            return false;
        }
        let mut i = 0;
        loop {
            let cfg = ff::avcodec_get_hw_config(dec, i);
            if cfg.is_null() {
                return false;
            }
            if (*cfg).device_type == ff::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA && ((*cfg).methods & ff::AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as i32) != 0 {
                return true;
            }
            i += 1;
        }
    }
}

/// Whether a D3D11VA decode device can be created on this machine (status bar, SPEC P5.3).
pub fn hw_decode_available() -> bool {
    let mut dev: *mut ff::AVBufferRef = ptr::null_mut();
    let r = unsafe { ff::av_hwdevice_ctx_create(&mut dev, ff::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA, ptr::null(), ptr::null_mut(), 0) };
    if r >= 0 && !dev.is_null() {
        unsafe { ff::av_buffer_unref(&mut dev) };
        true
    } else {
        false
    }
}

/// Pixel layout for preview frames (`VideoDecoder::to_preview`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewFormat {
    Rgb24 = 0,
    I420 = 1,
}

#[derive(Clone, Debug)]
pub struct DecodeOptions {
    pub threads: i32,
    pub hw: HwMode,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        DecodeOptions { threads: 2, hw: HwMode::Auto }
    }
}

/// Borrowed view of a decoded frame in system memory.
pub struct FrameView<'a> {
    pub width: usize,
    pub height: usize,
    pub pts_us: i64,
    pub luma: PlaneRef<'a>,
    pub chroma: Option<(PlaneRef<'a>, PlaneRef<'a>)>,
}

/// One sample delivered to the caller.
pub struct SampleFrames<'a> {
    /// Index in the clip's sample plan.
    pub index: usize,
    pub target_us: i64,
    pub frame: FrameView<'a>,
    pub pair: Option<FrameView<'a>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DecodeStats {
    pub frames_decoded: u64,
    pub seeks: u64,
    pub samples: u64,
    pub missing: u64,
    pub corrupt_packets: u64,
    pub hw: bool,
}

struct Sws {
    ctx: *mut ff::SwsContext,
}

impl Drop for Sws {
    fn drop(&mut self) {
        unsafe { ff::sws_freeContext(self.ctx) };
    }
}

unsafe extern "C" fn pick_d3d11(_ctx: *mut ff::AVCodecContext, fmts: *const ff::AVPixelFormat) -> ff::AVPixelFormat {
    let mut p = fmts;
    let mut first_sw = ff::AVPixelFormat::AV_PIX_FMT_NONE;
    unsafe {
        while *p != ff::AVPixelFormat::AV_PIX_FMT_NONE {
            if *p == ff::AVPixelFormat::AV_PIX_FMT_D3D11 {
                return *p;
            }
            let d = ff::av_pix_fmt_desc_get(*p);
            if first_sw == ff::AVPixelFormat::AV_PIX_FMT_NONE
                && !d.is_null()
                && (*d).flags & ff::AV_PIX_FMT_FLAG_HWACCEL as u64 == 0
            {
                first_sw = *p;
            }
            p = p.add(1);
        }
    }
    first_sw
}

/// Decoder bound to one file's best video stream.
pub struct VideoDecoder {
    input: Input,
    dec: Decoder,
    stream: usize,
    tb: ff::AVRational,
    start_ts: i64,
    fps: FrameRate,
    intra_only: bool,
    /// Intra-only streams: packets presented before this (µs, relative) are dropped unread by
    /// the decoder. Sampling uses it to decode only the frames it needs.
    drop_before_us: i64,
    pkt: Packet,
    eof_sent: bool,
    gop_us: i64,
    last_key_us: Option<i64>,
    key_intervals: Vec<i64>,
    sws: Option<Sws>,
    sws_key: (i32, i32, i32),
    sw_frame: Frame,
    conv: Frame,
    pub stats: DecodeStats,
    hw_device: *mut ff::AVBufferRef,
    errors_in_row: u32,
    /// Preview state: pts (µs) of the last frame returned by `frame_near`.
    last_preview_us: Option<i64>,
    /// Decoded frame just past the last preview target (not yet shown).
    pending: Option<Frame>,
    /// Frame returned by the last `frame_near` (re-shown while the target hasn't advanced).
    preview_last: Option<Frame>,
    rgb_sws: Option<Sws>,
    rgb_key: (i32, i32, i32, u32),
}

unsafe impl Send for VideoDecoder {}

impl Drop for VideoDecoder {
    fn drop(&mut self) {
        if !self.hw_device.is_null() {
            unsafe { ff::av_buffer_unref(&mut self.hw_device) };
        }
    }
}

impl VideoDecoder {
    pub fn open(path: &Path, info: &MediaInfo, opts: &DecodeOptions) -> Result<VideoDecoder> {
        let input = Input::open(path)?;
        let stream = input.best_stream(ff::AVMediaType::AVMEDIA_TYPE_VIDEO).ok_or(MediaError::NoStream("video"))?;
        let st = input.stream(stream);
        let tb = st.time_base;
        let start_ts = if st.start_time != ff::AV_NOPTS_VALUE { st.start_time } else { 0 };
        let par = st.codecpar;
        let mut hw_device: *mut ff::AVBufferRef = ptr::null_mut();
        // Only open a GPU device when D3D11VA can actually decode this stream: having one forces
        // single-threaded decoding, which made 4K ProRes ~6× slower than it needed to be.
        if opts.hw == HwMode::Auto && hw_decodable(unsafe { (*par).codec_id }, info) {
            let r = unsafe {
                ff::av_hwdevice_ctx_create(
                    &mut hw_device,
                    ff::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
                    ptr::null(),
                    ptr::null_mut(),
                    0,
                )
            };
            if r < 0 {
                hw_device = ptr::null_mut();
            }
        }
        let hwdev = hw_device;
        let intra_only = info.video.as_ref().map(|v| v.intra_only).unwrap_or(false);
        // Intra decoders slice-thread one frame at a time, so they can use every core without
        // piling up frames; others keep the per-worker share.
        let threads = if !hw_device.is_null() { 1 } else if intra_only { 0 } else { opts.threads };
        // SAFETY: `par` belongs to the open input, which outlives the call.
        let dec = unsafe { Decoder::open(par, threads, |ctx| unsafe {
            if !hwdev.is_null() {
                (*ctx).hw_device_ctx = ff::av_buffer_ref(hwdev);
                (*ctx).get_format = Some(pick_d3d11);
            }
            // Sampled decoding seeks between frames, which drains a frame-threading pipeline
            // every time. Intra codecs (ProRes, DNx) parallelise within a frame instead.
            if intra_only {
                (*ctx).thread_type = ff::FF_THREAD_SLICE;
            }
        }) }?;
        let fps = info.fps();
        Ok(VideoDecoder {
            input,
            dec,
            stream,
            tb,
            start_ts,
            fps,
            intra_only,
            drop_before_us: i64::MIN,
            pkt: Packet::new(),
            eof_sent: false,
            gop_us: if intra_only { fps.frame_duration_us() as i64 } else { 2_000_000 },
            last_key_us: None,
            key_intervals: Vec::new(),
            sws: None,
            sws_key: (0, 0, -1),
            sw_frame: Frame::new(),
            conv: Frame::new(),
            stats: DecodeStats::default(),
            hw_device,
            errors_in_row: 0,
            last_preview_us: None,
            pending: None,
            preview_last: None,
            rgb_sws: None,
            rgb_key: (0, 0, -1, 0),
        })
    }

    pub fn fps(&self) -> FrameRate {
        self.fps
    }

    fn rel_us(&self, ts: i64) -> i64 {
        ts_to_us(ts - self.start_ts, self.tb)
    }

    /// Latest indexed keyframe at or before `rel_us`, if the container has an index.
    fn keyframe_before(&self, rel_us: i64) -> Option<i64> {
        let st = self.input.streams()[self.stream];
        let ts = self.start_ts + us_to_ts(rel_us, self.tb);
        let e = unsafe { ff::avformat_index_get_entry_from_timestamp(st, ts, ff::AVSEEK_FLAG_BACKWARD) };
        if e.is_null() {
            None
        } else {
            Some(self.rel_us(unsafe { (*e).timestamp }))
        }
    }

    fn seek_to(&mut self, rel_us: i64) -> Result<()> {
        let ts = self.start_ts + us_to_ts(rel_us, self.tb);
        self.input.seek(self.stream, ts)?;
        self.dec.flush();
        self.eof_sent = false;
        self.stats.seeks += 1;
        self.last_preview_us = None;
        self.pending = None;
        self.preview_last = None;
        Ok(())
    }

    /// Pulls the next decoded frame into `out`. `Ok(false)` at end of stream.
    fn next_frame(&mut self, out: &mut Frame) -> Result<bool> {
        loop {
            let r = self.dec.receive(out);
            if r == 0 {
                self.stats.frames_decoded += 1;
                self.errors_in_row = 0;
                return Ok(true);
            }
            if r == EOF {
                return Ok(false);
            }
            if r != EAGAIN {
                // A decode error: count it and feed more input rather than spinning on receive.
                self.stats.corrupt_packets += 1;
                self.errors_in_row += 1;
                if self.errors_in_row > 256 {
                    return Err(MediaError::Ffmpeg(format!("decoder keeps failing: {}", crate::ffi::err_string(r))));
                }
                if self.eof_sent {
                    return Ok(false);
                }
            } else if self.eof_sent {
                return Ok(false);
            }
            loop {
                let r = self.input.read(&mut self.pkt);
                if r < 0 {
                    self.dec.send(None);
                    self.eof_sent = true;
                    break;
                }
                if self.pkt.stream() == self.stream {
                    if self.intra_only && self.drop_before_us != i64::MIN && self.pkt.pts() != ff::AV_NOPTS_VALUE && self.rel_us(self.pkt.pts()) < self.drop_before_us {
                        self.pkt.unref();
                        continue;
                    }
                    if self.pkt.is_key() && !self.intra_only {
                        let t = self.rel_us(self.pkt.pts());
                        if let Some(prev) = self.last_key_us {
                            if t > prev {
                                self.key_intervals.push(t - prev);
                                if self.key_intervals.len() > 16 {
                                    self.key_intervals.remove(0);
                                }
                                let mut v = self.key_intervals.clone();
                                v.sort_unstable();
                                self.gop_us = v[v.len() / 2].max(1);
                            }
                        }
                        self.last_key_us = Some(t);
                    }
                    let s = self.dec.send(Some(&self.pkt));
                    self.pkt.unref();
                    if s < 0 && s != EAGAIN {
                        self.stats.corrupt_packets += 1;
                    }
                    break;
                }
                self.pkt.unref();
            }
        }
    }

    /// Ensures `f` is in system memory and a directly readable format; returns the frame to view.
    fn to_viewable<'a>(&'a mut self, f: &'a Frame) -> Result<&'a Frame> {
        let mut src = f;
        if f.raw().format == ff::AVPixelFormat::AV_PIX_FMT_D3D11 as i32 {
            self.sw_frame.unref();
            check(unsafe { ff::av_hwframe_transfer_data(self.sw_frame.ptr, f.ptr, 0) })?;
            unsafe {
                (*self.sw_frame.ptr).pts = (*f.ptr).pts;
                (*self.sw_frame.ptr).best_effort_timestamp = (*f.ptr).best_effort_timestamp;
                (*self.sw_frame.ptr).color_range = (*f.ptr).color_range;
            }
            self.stats.hw = true;
            src = &self.sw_frame;
        }
        let fmt: ff::AVPixelFormat = unsafe { std::mem::transmute(src.raw().format) };
        if directly_viewable(fmt) {
            return Ok(src);
        }
        // Convert RGB / big-endian / exotic layouts to planar 16-bit 4:4:4.
        let (w, h) = (src.raw().width, src.raw().height);
        let key = (w, h, src.raw().format);
        if self.sws.is_none() || self.sws_key != key {
            let ctx = unsafe {
                ff::sws_getContext(
                    w,
                    h,
                    fmt,
                    w,
                    h,
                    ff::AVPixelFormat::AV_PIX_FMT_YUV444P16LE,
                    ff::SwsFlags::SWS_POINT as i32,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null(),
                )
            };
            if ctx.is_null() {
                return Err(MediaError::Unsupported(format!("pixel format {fmt:?}")));
            }
            self.sws = Some(Sws { ctx });
            self.sws_key = key;
        }
        self.conv.unref();
        unsafe {
            let c = &mut *self.conv.ptr;
            c.width = w;
            c.height = h;
            c.format = ff::AVPixelFormat::AV_PIX_FMT_YUV444P16LE as i32;
            check(ff::av_frame_get_buffer(self.conv.ptr, 0))?;
            let s = src.raw();
            #[allow(clippy::unwrap_used)]
            ff::sws_scale(
                self.sws.as_ref().unwrap().ctx,
                s.data.as_ptr() as *const *const u8,
                s.linesize.as_ptr(),
                0,
                h,
                c.data.as_ptr(),
                c.linesize.as_ptr(),
            );
            c.pts = s.pts;
            c.best_effort_timestamp = s.best_effort_timestamp;
            c.color_range = ff::AVColorRange::AVCOL_RANGE_JPEG;
        }
        Ok(&self.conv)
    }

    /// Decodes the frames for `targets` (sorted, µs relative to the first frame) and calls `f`
    /// for each. Sample indices start at `first_index`. `f` returns `false` to stop early.
    pub fn decode_samples(
        &mut self,
        targets: &[i64],
        first_index: usize,
        pair: bool,
        mut f: impl FnMut(SampleFrames) -> bool,
    ) -> Result<DecodeStats> {
        if targets.is_empty() {
            return Ok(self.stats.clone());
        }
        let frame_us = self.fps.frame_duration_us() as i64;
        if targets[0] > frame_us * 2 {
            self.seek_to(targets[0])?;
        }
        let mut ti = 0usize;
        let mut cur = Frame::new();
        // Samples waiting for their pair frame: (plan index, target, frame ref).
        let mut pending: Vec<(usize, i64, Frame)> = Vec::new();
        let mut last: Option<Frame> = None;
        let mut stop = false;
        let mut seeked_for: Option<usize> = None;
        while ti < targets.len() || !pending.is_empty() {
            // Skip/seek decisions only when no pair is outstanding.
            if pending.is_empty() && ti < targets.len() {
                let pos = last.as_ref().map(|l| self.rel_us(l.pts())).unwrap_or(0);
                let gap = targets[ti] - pos;
                let seek = match self.keyframe_before(targets[ti]) {
                    // With a keyframe index: seek when the keyframe before the target is ahead
                    // of where sequential decoding already is.
                    Some(kf) => kf > pos + frame_us && gap > 2 * frame_us,
                    None => {
                        let thr = if self.intra_only { 3 * frame_us } else { (2 * self.gop_us).max(1_000_000) };
                        gap > thr
                    }
                };
                // Seek at most once per target: after a seek the decoder may land slightly before
                // the indexed keyframe, which must not trigger another seek.
                if last.is_some() && seek && seeked_for != Some(ti) {
                    self.seek_to(targets[ti])?;
                    seeked_for = Some(ti);
                }
                self.dec.set_skip_nonref(!self.intra_only && gap > 300_000 + 8 * frame_us);
                if self.intra_only {
                    self.drop_before_us = targets[ti] - frame_us / 2;
                }
            } else {
                self.dec.set_skip_nonref(false);
                self.drop_before_us = i64::MIN;
            }
            cur.unref();
            if !self.next_frame(&mut cur)? {
                break;
            }
            let pts = self.rel_us(cur.pts());
            // Resolve pending samples with this frame as their pair.
            if !pending.is_empty() {
                let pend = std::mem::take(&mut pending);
                for (idx, tgt, sf) in pend {
                    if stop {
                        break;
                    }
                    if !self.emit(&sf, Some(&cur), idx, tgt, &mut f)? {
                        stop = true;
                    }
                }
            }
            if stop {
                break;
            }
            while ti < targets.len() && pts >= targets[ti] {
                let idx = first_index + ti;
                if pair {
                    pending.push((idx, targets[ti], cur.clone_ref()?));
                } else if !self.emit(&cur, None, idx, targets[ti], &mut f)? {
                    stop = true;
                    break;
                }
                ti += 1;
            }
            if stop {
                break;
            }
            last = Some(cur.clone_ref()?);
        }
        if !stop {
            for (idx, tgt, sf) in std::mem::take(&mut pending) {
                if !self.emit(&sf, None, idx, tgt, &mut f)? {
                    stop = true;
                    break;
                }
            }
        }
        // Targets past the last decodable frame use the last frame if it is close enough.
        if !stop {
            if let Some(l) = last {
                let lp = self.rel_us(l.pts());
                while ti < targets.len() {
                    if lp + 2 * frame_us >= targets[ti] {
                        if !self.emit(&l, None, first_index + ti, targets[ti], &mut f)? {
                            break;
                        }
                    } else {
                        self.stats.missing += 1;
                    }
                    ti += 1;
                }
            } else {
                self.stats.missing += (targets.len() - ti) as u64;
            }
        }
        self.drop_before_us = i64::MIN;
        Ok(self.stats.clone())
    }

    fn emit(
        &mut self,
        sf: &Frame,
        pf: Option<&Frame>,
        idx: usize,
        target: i64,
        f: &mut impl FnMut(SampleFrames) -> bool,
    ) -> Result<bool> {
        // Convert the pair first into its own owned frame so both views can coexist.
        let pair_owned = match pf {
            Some(p) => Some(self.owned_viewable(p)?),
            None => None,
        };
        let sample_owned = self.owned_viewable(sf)?;
        let pts_s = self.rel_us(sf.pts());
        let frame = view(&sample_owned, pts_s)?;
        let pair = match &pair_owned {
            Some(p) => Some(view(p, self.rel_us(p.pts()))?),
            None => None,
        };
        self.stats.samples += 1;
        Ok(f(SampleFrames { index: idx, target_us: target, frame, pair }))
    }

    fn owned_viewable(&mut self, f: &Frame) -> Result<Frame> {
        let v = self.to_viewable(f)?;
        v.clone_ref()
    }

    /// Decodes the frame at or after `us` (seeking), for previews and thumbnails.
    pub fn frame_at(&mut self, us: i64) -> Result<Option<Frame>> {
        self.seek_to(us)?;
        let mut cur = Frame::new();
        loop {
            cur.unref();
            if !self.next_frame(&mut cur)? {
                return Ok(None);
            }
            if self.rel_us(cur.pts()) + (self.fps.frame_duration_us() as i64) / 2 >= us {
                let owned = self.owned_viewable(&cur)?;
                return Ok(Some(owned));
            }
        }
    }

    /// Preview decode: returns the frame shown at `us` (the last frame with pts ≤ us + ½ frame).
    /// Decodes forward from the previous request when `us` is a little ahead (playback), and
    /// seeks otherwise (scrubbing).
    pub fn frame_near(&mut self, us: i64) -> Result<Option<Frame>> {
        self.drop_before_us = i64::MIN;
        let half = (self.fps.frame_duration_us() / 2.0) as i64;
        let forward = matches!(self.last_preview_us, Some(last) if us >= last && us - last <= 2_000_000);
        let mut best: Option<Frame> = None;
        if forward {
            // A frame decoded past the previous target is still the next one in the stream.
            if let Some(p) = self.pending.take() {
                if self.rel_us(p.pts()) > us + half {
                    self.pending = Some(p);
                    let last = self.preview_last.take();
                    return self.preview_owned(last);
                }
                best = Some(p);
            } else if let Some(l) = self.preview_last.take() {
                if self.rel_us(l.pts()) + half >= us {
                    return self.preview_owned(Some(l));
                }
            }
        } else {
            self.seek_to(us)?;
        }
        let mut cur = Frame::new();
        while best.as_ref().is_none_or(|b| self.rel_us(b.pts()) + half < us) {
            cur.unref();
            if !self.next_frame(&mut cur)? {
                break;
            }
            if self.rel_us(cur.pts()) > us + half && best.is_some() {
                self.pending = Some(cur.clone_ref()?);
                break;
            }
            best = Some(cur.clone_ref()?);
        }
        if best.is_none() {
            best = self.preview_last.take();
        }
        self.preview_owned(best)
    }

    fn preview_owned(&mut self, f: Option<Frame>) -> Result<Option<Frame>> {
        let Some(f) = f else { return Ok(None) };
        self.last_preview_us = Some(self.rel_us(f.pts()));
        let out = self.owned_viewable(&f)?;
        self.preview_last = Some(f);
        Ok(Some(out))
    }

    /// Converts a viewable frame to packed RGB24, scaled to at most `max_w` wide.
    pub fn to_rgb(&mut self, f: &Frame, max_w: u32) -> Result<(u32, u32, Vec<u8>)> {
        self.to_preview(f, max_w, PreviewFormat::Rgb24)
    }

    /// Converts a viewable frame for preview, scaled to at most `max_w` wide (even sizes).
    /// `Rgb24` is packed; `I420` is the Y plane then U then V, each tightly packed.
    pub fn to_preview(&mut self, f: &Frame, max_w: u32, fmt: PreviewFormat) -> Result<(u32, u32, Vec<u8>)> {
        let r = f.raw();
        let (w, h) = (r.width, r.height);
        let scale = (max_w as f64 / w.max(1) as f64).min(1.0);
        let ow = ((w as f64 * scale).round() as i32).max(2) & !1;
        let oh = ((h as f64 * scale).round() as i32).max(2) & !1;
        let key = (w, h, r.format, ow as u32 | (fmt as u32) << 31);
        if self.rgb_sws.is_none() || self.rgb_key != key {
            let src: ff::AVPixelFormat = unsafe { std::mem::transmute(r.format) };
            let dst = match fmt {
                PreviewFormat::Rgb24 => ff::AVPixelFormat::AV_PIX_FMT_RGB24,
                PreviewFormat::I420 => ff::AVPixelFormat::AV_PIX_FMT_YUV420P,
            };
            let ctx = unsafe { ff::sws_getContext(w, h, src, ow, oh, dst, ff::SwsFlags::SWS_BILINEAR as i32, ptr::null_mut(), ptr::null_mut(), ptr::null()) };
            if ctx.is_null() {
                return Err(MediaError::Unsupported("preview conversion".into()));
            }
            self.rgb_sws = Some(Sws { ctx });
            self.rgb_key = key;
        }
        let (ow_u, oh_u) = (ow as usize, oh as usize);
        let (len, strides, offsets) = match fmt {
            PreviewFormat::Rgb24 => (ow_u * 3 * oh_u, [ow * 3, 0, 0, 0], [0, 0, 0]),
            PreviewFormat::I420 => {
                let y = ow_u * oh_u;
                let c = (ow_u / 2) * (oh_u / 2);
                (y + 2 * c, [ow, ow / 2, ow / 2, 0], [0, y, y + c])
            }
        };
        let mut out = vec![0u8; len];
        unsafe {
            let base = out.as_mut_ptr();
            let dst = match fmt {
                PreviewFormat::Rgb24 => [base, ptr::null_mut(), ptr::null_mut(), ptr::null_mut()],
                PreviewFormat::I420 => [base, base.add(offsets[1]), base.add(offsets[2]), ptr::null_mut()],
            };
            #[allow(clippy::unwrap_used)]
            ff::sws_scale(self.rgb_sws.as_ref().unwrap().ctx, r.data.as_ptr() as *const *const u8, r.linesize.as_ptr(), 0, h, dst.as_ptr(), strides.as_ptr());
        }
        Ok((ow as u32, oh as u32, out))
    }

    pub fn rel_pts_us(&self, f: &Frame) -> i64 {
        self.rel_us(f.pts())
    }

    pub fn stream_fps(&self) -> f64 {
        q2d(self.input.stream(self.stream).avg_frame_rate)
    }
}

fn directly_viewable(fmt: ff::AVPixelFormat) -> bool {
    let d = unsafe { ff::av_pix_fmt_desc_get(fmt) };
    if d.is_null() {
        return false;
    }
    let d = unsafe { &*d };
    let bad = (ff::AV_PIX_FMT_FLAG_RGB | ff::AV_PIX_FMT_FLAG_PAL | ff::AV_PIX_FMT_FLAG_BE | ff::AV_PIX_FMT_FLAG_HWACCEL
        | ff::AV_PIX_FMT_FLAG_BITSTREAM | ff::AV_PIX_FMT_FLAG_FLOAT) as u64;
    d.flags & bad == 0 && d.comp[0].depth <= 16 && d.comp[0].depth >= 8
}

/// Builds plane views over a viewable (system-memory, YUV/gray) frame.
pub fn view(f: &Frame, pts_us: i64) -> Result<FrameView<'_>> {
    let r = f.raw();
    let fmt: ff::AVPixelFormat = unsafe { std::mem::transmute(r.format) };
    let d = unsafe { ff::av_pix_fmt_desc_get(fmt) };
    if d.is_null() {
        return Err(MediaError::Unsupported("unknown pixel format".into()));
    }
    let d = unsafe { &*d };
    let (w, h) = (r.width.max(0) as usize, r.height.max(0) as usize);
    let full_range = r.color_range == ff::AVColorRange::AVCOL_RANGE_JPEG
        // SAFETY: descriptor names are static strings.
        || unsafe { crate::ffi::cstr(d.name) }.starts_with("yuvj");
    let plane = |ci: usize, pw: usize, ph: usize, chroma: bool| -> Result<PlaneRef<'_>> {
        let c = d.comp[ci];
        let depth = c.depth as u8;
        let wide = depth > 8;
        let esz = if wide { 2 } else { 1 };
        let p = c.plane as usize;
        let ls = r.linesize[p];
        if ls <= 0 || r.data[p].is_null() {
            return Err(MediaError::Unsupported("negative or missing linesize".into()));
        }
        let stride = ls as usize / esz;
        let step = (c.step as usize / esz).max(1);
        let off = c.offset as usize / esz;
        let len_el = stride * ph;
        let data = unsafe {
            if wide {
                let s = std::slice::from_raw_parts(r.data[p] as *const u16, len_el);
                PlaneData::U16(&s[off.min(len_el)..])
            } else {
                let s = std::slice::from_raw_parts(r.data[p] as *const u8, len_el);
                PlaneData::U8(&s[off.min(len_el)..])
            }
        };
        Ok(PlaneRef {
            data,
            width: pw,
            height: ph,
            stride,
            step,
            bit_depth: depth,
            msb_aligned: c.shift > 0,
            full_range,
            chroma,
        })
    };
    let luma = plane(0, w, h, false)?;
    let chroma = if d.nb_components >= 3 && d.flags & ff::AV_PIX_FMT_FLAG_RGB as u64 == 0 {
        let cw = (w + (1 << d.log2_chroma_w) - 1) >> d.log2_chroma_w;
        let ch = (h + (1 << d.log2_chroma_h) - 1) >> d.log2_chroma_h;
        Some((plane(1, cw, ch, true)?, plane(2, cw, ch, true)?))
    } else {
        None
    };
    Ok(FrameView { width: w, height: h, pts_us, luma, chroma })
}

/// Keyframe times of the best video stream (µs from its first frame): from the container index
/// when it has one, else by reading every packet (no decoding). Index entries carry decode
/// timestamps, so they are measured from the first entry, which removes a constant B-frame delay.
pub fn keyframes(path: &Path) -> Result<Vec<i64>> {
    let mut input = Input::open(path)?;
    let stream = input.best_stream(ff::AVMediaType::AVMEDIA_TYPE_VIDEO).ok_or(MediaError::NoStream("video"))?;
    let st = input.streams()[stream];
    let (tb, start) = unsafe { ((*st).time_base, if (*st).start_time != ff::AV_NOPTS_VALUE { (*st).start_time } else { 0 }) };
    let mut out = Vec::new();
    let n = unsafe { ff::avformat_index_get_entries_count(st) };
    let mut first: Option<i64> = None;
    for i in 0..n {
        let e = unsafe { ff::avformat_index_get_entry(st, i) };
        if e.is_null() {
            continue;
        }
        let (ts, flags) = unsafe { ((*e).timestamp, (*e).flags() as i32) };
        let base = *first.get_or_insert(ts);
        if flags & ff::AVINDEX_KEYFRAME != 0 {
            out.push(ts_to_us(ts - base, tb));
        }
    }
    if out.is_empty() {
        let mut pkt = Packet::new();
        while input.read(&mut pkt) >= 0 {
            if pkt.stream() == stream && pkt.is_key() {
                let pts = pkt.pts();
                if pts != ff::AV_NOPTS_VALUE {
                    out.push(ts_to_us(pts - start, tb));
                }
            }
            pkt.unref();
        }
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_standard_rate() {
        let fps = FrameRate::new(25, 1);
        let p = sample_plan(10_000_000, fps, SampleRate::Standard);
        assert_eq!(p[0], 0);
        assert_eq!(p[1], 250_000);
        // last frame (9.96 s) is always sampled
        assert_eq!(*p.last().unwrap(), fps.frames_to_us(249));
        assert!(p.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn plan_ntsc_and_all() {
        let fps = FrameRate::new(30000, 1001);
        let p = sample_plan(5_005_000, fps, SampleRate::All);
        assert_eq!(p.len(), 150);
        assert_eq!(p[1], fps.frames_to_us(1));
        let p = sample_plan(5_005_000, fps, SampleRate::Draft);
        assert_eq!(&p[..3], &[0, 1_000_000, 2_000_000]);
        assert_eq!(*p.last().unwrap(), fps.frames_to_us(149));
    }

    #[test]
    fn plan_short_clip() {
        let p = sample_plan(40_000, FrameRate::new(25, 1), SampleRate::Standard);
        assert_eq!(p, vec![0]);
    }
}
