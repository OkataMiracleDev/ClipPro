//! Thin RAII wrappers over the FFmpeg C API. All `unsafe` FFmpeg access lives in this crate.

use ffmpeg_sys_next as ff;
use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr;

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("{path}: {msg}")]
    Open { path: String, msg: String },
    #[error("ffmpeg: {0}")]
    Ffmpeg(String),
    #[error("no {0} stream")]
    NoStream(&'static str),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, MediaError>;

pub fn err_string(code: i32) -> String {
    let mut buf = [0 as libc::c_char; 256];
    unsafe {
        ff::av_strerror(code, buf.as_mut_ptr(), buf.len());
        CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
    }
}

pub fn check(code: i32) -> Result<i32> {
    if code < 0 {
        Err(MediaError::Ffmpeg(err_string(code)))
    } else {
        Ok(code)
    }
}

pub const EAGAIN: i32 = ff::AVERROR(libc::EAGAIN);
pub const EOF: i32 = ff::AVERROR_EOF;

/// Silences FFmpeg's stderr logging (errors are surfaced through return codes).
pub fn init_logging() {
    unsafe { ff::av_log_set_level(ff::AV_LOG_QUIET) };
}

/// Owned `AVFormatContext` opened for input.
pub struct Input {
    pub ctx: *mut ff::AVFormatContext,
}

unsafe impl Send for Input {}

impl Input {
    pub fn open(path: &Path) -> Result<Input> {
        init_logging();
        let p = path.to_string_lossy().into_owned();
        let c = CString::new(p.clone()).map_err(|_| MediaError::Open { path: p.clone(), msg: "path contains NUL".into() })?;
        let mut ctx: *mut ff::AVFormatContext = ptr::null_mut();
        let r = unsafe { ff::avformat_open_input(&mut ctx, c.as_ptr(), ptr::null(), ptr::null_mut()) };
        if r < 0 {
            return Err(MediaError::Open { path: p, msg: err_string(r) });
        }
        let input = Input { ctx };
        let r = unsafe { ff::avformat_find_stream_info(ctx, ptr::null_mut()) };
        if r < 0 {
            return Err(MediaError::Open { path: p, msg: err_string(r) });
        }
        Ok(input)
    }

    pub fn streams(&self) -> &[*mut ff::AVStream] {
        unsafe {
            let c = &*self.ctx;
            if c.nb_streams == 0 || c.streams.is_null() {
                return &[];
            }
            std::slice::from_raw_parts(c.streams, c.nb_streams as usize)
        }
    }

    pub fn stream(&self, i: usize) -> &ff::AVStream {
        unsafe { &*self.streams()[i] }
    }

    pub fn best_stream(&self, kind: ff::AVMediaType) -> Option<usize> {
        let r = unsafe { ff::av_find_best_stream(self.ctx, kind, -1, -1, ptr::null_mut(), 0) };
        (r >= 0).then_some(r as usize)
    }

    pub fn read(&mut self, pkt: &mut Packet) -> i32 {
        unsafe { ff::av_read_frame(self.ctx, pkt.ptr) }
    }

    /// Seeks so the next packets start at or before `ts` (stream time base).
    pub fn seek(&mut self, stream: usize, ts: i64) -> Result<()> {
        let r = unsafe { ff::avformat_seek_file(self.ctx, stream as i32, i64::MIN, ts, ts, 0) };
        if r < 0 {
            let r2 = unsafe { ff::av_seek_frame(self.ctx, stream as i32, ts, ff::AVSEEK_FLAG_BACKWARD as i32) };
            check(r2)?;
        }
        Ok(())
    }

    pub fn duration_us(&self) -> i64 {
        unsafe { (*self.ctx).duration }
    }

    pub fn format_name(&self) -> String {
        unsafe {
            let f = (*self.ctx).iformat;
            if f.is_null() {
                return String::new();
            }
            cstr((*f).name)
        }
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        unsafe { ff::avformat_close_input(&mut self.ctx) };
    }
}

pub struct Packet {
    pub ptr: *mut ff::AVPacket,
}

unsafe impl Send for Packet {}

impl Packet {
    pub fn new() -> Packet {
        Packet { ptr: unsafe { ff::av_packet_alloc() } }
    }
    pub fn unref(&mut self) {
        unsafe { ff::av_packet_unref(self.ptr) };
    }
    pub fn stream(&self) -> usize {
        unsafe { (*self.ptr).stream_index as usize }
    }
    pub fn is_key(&self) -> bool {
        unsafe { (*self.ptr).flags & ff::AV_PKT_FLAG_KEY as i32 != 0 }
    }
    pub fn pts(&self) -> i64 {
        unsafe {
            let p = &*self.ptr;
            if p.pts != ff::AV_NOPTS_VALUE {
                p.pts
            } else {
                p.dts
            }
        }
    }
}

impl Default for Packet {
    fn default() -> Self {
        Packet::new()
    }
}

impl Drop for Packet {
    fn drop(&mut self) {
        unsafe { ff::av_packet_free(&mut self.ptr) };
    }
}

pub struct Frame {
    pub ptr: *mut ff::AVFrame,
}

unsafe impl Send for Frame {}

impl Frame {
    pub fn new() -> Frame {
        Frame { ptr: unsafe { ff::av_frame_alloc() } }
    }
    pub fn unref(&mut self) {
        unsafe { ff::av_frame_unref(self.ptr) };
    }
    /// New reference to the same buffers.
    pub fn clone_ref(&self) -> Result<Frame> {
        let f = Frame::new();
        check(unsafe { ff::av_frame_ref(f.ptr, self.ptr) })?;
        Ok(f)
    }
    pub fn raw(&self) -> &ff::AVFrame {
        unsafe { &*self.ptr }
    }
    pub fn pts(&self) -> i64 {
        let f = self.raw();
        if f.best_effort_timestamp != ff::AV_NOPTS_VALUE {
            f.best_effort_timestamp
        } else {
            f.pts
        }
    }
}

impl Default for Frame {
    fn default() -> Self {
        Frame::new()
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        unsafe { ff::av_frame_free(&mut self.ptr) };
    }
}

/// Owned decoder context.
pub struct Decoder {
    pub ctx: *mut ff::AVCodecContext,
}

unsafe impl Send for Decoder {}

impl Decoder {
    pub fn open(par: *const ff::AVCodecParameters, threads: i32, setup: impl FnOnce(*mut ff::AVCodecContext)) -> Result<Decoder> {
        unsafe {
            let codec = ff::avcodec_find_decoder((*par).codec_id);
            if codec.is_null() {
                return Err(MediaError::Unsupported(format!("no decoder for {}", codec_name((*par).codec_id))));
            }
            let ctx = ff::avcodec_alloc_context3(codec);
            if ctx.is_null() {
                return Err(MediaError::Ffmpeg("out of memory".into()));
            }
            let dec = Decoder { ctx };
            check(ff::avcodec_parameters_to_context(ctx, par))?;
            (*ctx).thread_count = threads;
            (*ctx).thread_type = (ff::FF_THREAD_FRAME | ff::FF_THREAD_SLICE) as i32;
            setup(ctx);
            check(ff::avcodec_open2(ctx, codec, ptr::null_mut()))?;
            Ok(dec)
        }
    }

    pub fn send(&mut self, pkt: Option<&Packet>) -> i32 {
        unsafe { ff::avcodec_send_packet(self.ctx, pkt.map(|p| p.ptr as *const _).unwrap_or(ptr::null())) }
    }

    pub fn receive(&mut self, f: &mut Frame) -> i32 {
        unsafe { ff::avcodec_receive_frame(self.ctx, f.ptr) }
    }

    pub fn flush(&mut self) {
        unsafe { ff::avcodec_flush_buffers(self.ctx) };
    }

    pub fn set_skip_nonref(&mut self, on: bool) {
        unsafe {
            (*self.ctx).skip_frame = if on { ff::AVDiscard::AVDISCARD_NONREF } else { ff::AVDiscard::AVDISCARD_DEFAULT };
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe { ff::avcodec_free_context(&mut self.ctx) };
    }
}

pub fn cstr(p: *const libc::c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() }
}

pub fn codec_name(id: ff::AVCodecID) -> String {
    unsafe { cstr(ff::avcodec_get_name(id)) }
}

/// All entries of an `AVDictionary`.
pub fn dict_entries(d: *const ff::AVDictionary) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if d.is_null() {
        return out;
    }
    let empty = c"";
    let mut e: *const ff::AVDictionaryEntry = ptr::null();
    loop {
        e = unsafe { ff::av_dict_get(d, empty.as_ptr(), e, ff::AV_DICT_IGNORE_SUFFIX as i32) };
        if e.is_null() {
            break;
        }
        unsafe { out.push((cstr((*e).key), cstr((*e).value))) };
    }
    out
}

pub fn q2d(r: ff::AVRational) -> f64 {
    if r.den == 0 {
        0.0
    } else {
        r.num as f64 / r.den as f64
    }
}

/// Converts `ts` in time base `tb` to microseconds.
pub fn ts_to_us(ts: i64, tb: ff::AVRational) -> i64 {
    if tb.den == 0 {
        return 0;
    }
    ((ts as i128 * tb.num as i128 * 1_000_000) / tb.den as i128) as i64
}

/// Converts microseconds to time base `tb` (floor).
pub fn us_to_ts(us: i64, tb: ff::AVRational) -> i64 {
    if tb.num == 0 {
        return 0;
    }
    ((us as i128 * tb.den as i128).div_euclid(tb.num as i128 * 1_000_000)) as i64
}
