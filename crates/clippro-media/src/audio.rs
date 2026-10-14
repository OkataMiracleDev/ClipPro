//! Audio decoding to planar f32 at 48 kHz (SPEC §17, P1.5).

use crate::ffi::{check, ts_to_us, Decoder, Frame, Input, MediaError, Packet, Result, EAGAIN, EOF};
use ffmpeg_sys_next as ff;
use std::path::Path;
use std::ptr;

pub const OUT_RATE: i32 = 48_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioStats {
    pub channels: usize,
    pub samples: u64,
    /// Presentation time of the first decoded sample relative to the video start (µs).
    pub start_us: i64,
}

struct Swr(*mut ff::SwrContext);

impl Drop for Swr {
    fn drop(&mut self) {
        unsafe { ff::swr_free(&mut self.0) };
    }
}

/// Decodes stream `index` (or the best audio stream when `None`) and calls `f` with each
/// block of planar samples (one `Vec` per channel). `f` returns `false` to stop.
pub fn decode_audio(
    path: &Path,
    index: Option<usize>,
    video_start_us: i64,
    mut f: impl FnMut(&[Vec<f32>]) -> bool,
) -> Result<AudioStats> {
    let mut input = Input::open(path)?;
    let stream = match index {
        Some(i) => i,
        None => input.best_stream(ff::AVMediaType::AVMEDIA_TYPE_AUDIO).ok_or(MediaError::NoStream("audio"))?,
    };
    let st = input.stream(stream);
    if unsafe { (*st.codecpar).codec_type } != ff::AVMediaType::AVMEDIA_TYPE_AUDIO {
        return Err(MediaError::NoStream("audio"));
    }
    let tb = st.time_base;
    let start_ts = if st.start_time != ff::AV_NOPTS_VALUE { st.start_time } else { 0 };
    let mut dec = Decoder::open(st.codecpar, 1, |_| {})?;
    let ch = unsafe { (*dec.ctx).ch_layout.nb_channels.max(1) } as usize;
    let mut stats = AudioStats { channels: ch, start_us: ts_to_us(start_ts, tb) - video_start_us, ..Default::default() };

    // Resampler: native layout/format/rate → same layout, planar f32, 48 kHz.
    let mut swr_ptr: *mut ff::SwrContext = ptr::null_mut();
    unsafe {
        let c = &*dec.ctx;
        check(ff::swr_alloc_set_opts2(
            &mut swr_ptr,
            &c.ch_layout,
            ff::AVSampleFormat::AV_SAMPLE_FMT_FLTP,
            OUT_RATE,
            &c.ch_layout,
            c.sample_fmt,
            c.sample_rate.max(1),
            0,
            ptr::null_mut(),
        ))?;
        check(ff::swr_init(swr_ptr))?;
    }
    let swr = Swr(swr_ptr);
    let mut pkt = Packet::new();
    let mut frame = Frame::new();
    let mut out: Vec<Vec<f32>> = vec![Vec::new(); ch];
    let mut eof = false;
    let mut run = |frame: Option<&Frame>, out: &mut Vec<Vec<f32>>, stats: &mut AudioStats| -> Result<bool> {
        let (in_ptr, in_n) = match frame {
            Some(fr) => (fr.raw().extended_data as *mut *const u8, fr.raw().nb_samples),
            None => (ptr::null_mut(), 0),
        };
        let max_out = unsafe { ff::swr_get_out_samples(swr.0, in_n) }.max(0) as usize + 32;
        for c in out.iter_mut() {
            c.resize(max_out, 0.0);
        }
        let mut ptrs: Vec<*mut u8> = out.iter_mut().map(|c| c.as_mut_ptr() as *mut u8).collect();
        let n = unsafe { ff::swr_convert(swr.0, ptrs.as_mut_ptr(), max_out as i32, in_ptr, in_n) };
        check(n)?;
        let n = n as usize;
        if n == 0 {
            return Ok(true);
        }
        for c in out.iter_mut() {
            c.truncate(n);
        }
        stats.samples += n as u64;
        Ok(f(out))
    };
    loop {
        let r = dec.receive(&mut frame);
        if r == 0 {
            if !run(Some(&frame), &mut out, &mut stats)? {
                return Ok(stats);
            }
            frame.unref();
            continue;
        }
        if r == EOF {
            break;
        }
        if r != EAGAIN {
            continue;
        }
        if eof {
            break;
        }
        loop {
            if input.read(&mut pkt) < 0 {
                dec.send(None);
                eof = true;
                break;
            }
            if pkt.stream() == stream {
                dec.send(Some(&pkt));
                pkt.unref();
                break;
            }
            pkt.unref();
        }
    }
    // Drain the resampler.
    run(None, &mut out, &mut stats)?;
    Ok(stats)
}
