//! Preview transport (ADR-002). Codecs WebView2 plays natively go to `<video>` through the asset
//! protocol; everything else is decoded here and served as JPEG frames on the `clippro://`
//! scheme (on Windows the webview sees `http://clippro.localhost/...`):
//! - `/frame/{media}/{us}?w={max_width}`: JPEG, for `<img>` and the 2D canvas.
//! - `/raw/{media}/{us}?w={max_width}`: I420 planes behind a 24-byte header, for WebGL (no
//!   encode cost; see ADR-002 for the numbers).

use clippro_core::Session;
use clippro_media::video::{DecodeOptions, HwMode, PreviewFormat, VideoDecoder};
use clippro_media::MediaInfo;
use clippro_types::MediaId;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Decoders kept open; scrubbing between a few clips shouldn't reopen files.
const OPEN_DECODERS: usize = 3;
const DEFAULT_WIDTH: u32 = 960;
const MAX_WIDTH: u32 = 3840;
const JPEG_QUALITY: u8 = 85;

struct Entry {
    media: i64,
    path: PathBuf,
    /// Time of the last frame (µs); requests past the end show it.
    last_us: i64,
    /// (matrix, range) codes for raw frames; see `raw_header`.
    colour: (u8, u8),
    dec: Arc<Mutex<VideoDecoder>>,
}

struct Open {
    dec: Arc<Mutex<VideoDecoder>>,
    last_us: i64,
    colour: (u8, u8),
}

/// Matrix code (0 = BT.601, 1 = BT.709, 2 = BT.2020) and range code (0 = limited, 1 = full) of
/// the I420 planes swscale produces for this source.
fn raw_colour(info: &MediaInfo) -> (u8, u8) {
    let Some(v) = &info.video else { return (1, 0) };
    if v.pix_fmt.starts_with("rgb") || v.pix_fmt.starts_with("gbr") || v.pix_fmt.starts_with("bgr") {
        return (0, 0); // swscale's default RGB→YUV matrix
    }
    let matrix = match v.color_matrix.as_str() {
        "bt709" => 1,
        "bt470bg" | "smpte170m" | "bt601" => 0,
        m if m.starts_with("bt2020") => 2,
        _ if v.height > 576 => 1,
        _ => 0,
    };
    // yuvj* (full range) is converted to limited by swscale; full-range data in a plain yuv
    // format passes through unchanged.
    let range = u8::from(v.color_range == "pc" && !v.pix_fmt.starts_with("yuvj"));
    (matrix, range)
}

#[derive(Default)]
pub struct PreviewCache {
    open: Mutex<Vec<Entry>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl PreviewCache {
    /// Drops every decoder (project closed or media relinked).
    pub fn clear(&self) {
        lock(&self.open).clear();
    }

    fn decoder(&self, session: &Session, media: i64) -> Result<Open, String> {
        let (path, info) = session.preview_source(MediaId(media)).map_err(|e| e.to_string())?.ok_or("Unknown media")?;
        let mut open = lock(&self.open);
        if let Some(i) = open.iter().position(|e| e.media == media && e.path == path) {
            let e = open.remove(i);
            let out = Open { dec: e.dec.clone(), last_us: e.last_us, colour: e.colour };
            open.insert(0, e);
            return Ok(out);
        }
        let dec = VideoDecoder::open(&path, &info, &DecodeOptions { threads: 0, hw: HwMode::Auto }).map_err(|e| e.to_string())?;
        let frame = info.video.as_ref().map_or(40_000.0, |v| v.fps.frame_duration_us());
        let last_us = (info.duration_us as f64 - frame).max(0.0) as i64;
        let colour = raw_colour(&info);
        let dec = Arc::new(Mutex::new(dec));
        open.insert(0, Entry { media, path, last_us, colour, dec: dec.clone() });
        open.truncate(OPEN_DECODERS);
        Ok(Open { dec, last_us, colour })
    }

    /// The frame shown at `us` (µs from the first video frame), at most `max_w` wide, as JPEG
    /// or as raw I420 with a header.
    pub fn frame(&self, session: &Session, kind: FrameKind, media: i64, us: i64, max_w: u32) -> Result<Vec<u8>, String> {
        let o = self.decoder(session, media)?;
        let mut dec = lock(&o.dec);
        let frame = dec.frame_near(us.clamp(0, o.last_us)).map_err(|e| e.to_string())?.ok_or("No frame at that time")?;
        let pts = dec.rel_pts_us(&frame);
        let max_w = max_w.clamp(16, MAX_WIDTH);
        match kind {
            FrameKind::Jpeg => {
                let (w, h, rgb) = dec.to_rgb(&frame, max_w).map_err(|e| e.to_string())?;
                let mut out = Vec::with_capacity(rgb.len() / 8);
                encode_jpeg(&mut out, &rgb, w, h)?;
                Ok(out)
            }
            FrameKind::Raw => {
                let (w, h, planes) = dec.to_preview(&frame, max_w, PreviewFormat::I420).map_err(|e| e.to_string())?;
                let mut out = Vec::with_capacity(RAW_HEADER + planes.len());
                out.extend_from_slice(&raw_header(w, h, o.colour, pts));
                out.extend_from_slice(&planes);
                Ok(out)
            }
        }
    }

    pub fn frame_jpeg(&self, session: &Session, media: i64, us: i64, max_w: u32) -> Result<Vec<u8>, String> {
        self.frame(session, FrameKind::Jpeg, media, us, max_w)
    }
}

pub const RAW_HEADER: usize = 24;

/// `CPF1`, width u32, height u32, matrix u8, range u8, 2 reserved bytes, pts µs i64 (all
/// little-endian), then the Y, U and V planes, tightly packed.
pub fn raw_header(w: u32, h: u32, (matrix, range): (u8, u8), pts_us: i64) -> [u8; RAW_HEADER] {
    let mut hd = [0u8; RAW_HEADER];
    hd[..4].copy_from_slice(b"CPF1");
    hd[4..8].copy_from_slice(&w.to_le_bytes());
    hd[8..12].copy_from_slice(&h.to_le_bytes());
    hd[12] = matrix;
    hd[13] = range;
    hd[16..24].copy_from_slice(&pts_us.to_le_bytes());
    hd
}

fn encode_jpeg(out: &mut Vec<u8>, rgb: &[u8], w: u32, h: u32) -> Result<(), String> {
    let mut enc = jpeg_encoder::Encoder::new(out, JPEG_QUALITY);
    enc.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_2_0);
    enc.encode(rgb, w as u16, h as u16, jpeg_encoder::ColorType::Rgb).map_err(|e| e.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameKind {
    Jpeg,
    Raw,
}

/// Parsed `clippro://` frame request.
#[derive(Debug, PartialEq)]
pub struct FrameRequest {
    pub kind: FrameKind,
    pub media: i64,
    pub us: i64,
    pub width: u32,
}

pub fn parse_frame_uri(path: &str, query: Option<&str>) -> Option<FrameRequest> {
    let mut parts = path.trim_start_matches('/').split('/');
    let kind = match parts.next()? {
        "frame" => FrameKind::Jpeg,
        "raw" => FrameKind::Raw,
        _ => return None,
    };
    let media = parts.next()?.parse().ok()?;
    let us = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let width = query
        .into_iter()
        .flat_map(|q| q.split('&'))
        .find_map(|kv| kv.strip_prefix("w=")?.parse().ok())
        .unwrap_or(DEFAULT_WIDTH);
    Some(FrameRequest { kind, media, us, width })
}

/// How the viewer should show a clip.
#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct MediaSource {
    /// File for `<video>` (asset protocol), when the codec is one WebView2 may play natively.
    /// The viewer falls back to frames if the element reports an error.
    pub native_path: Option<String>,
    pub duration_us: f64,
    pub fps: f64,
    pub width: u32,
    pub height: u32,
    /// First video pts (µs). Native playback time minus this is the measurement time.
    pub start_us: f64,
    pub codec: String,
}

/// WebView2 (Chromium) decodes these without extensions; HEVC depends on the machine, so it is
/// offered and left to the element's error fallback.
pub fn native_playable(info: &MediaInfo) -> bool {
    let Some(v) = &info.video else { return false };
    let container = info.container.as_str();
    let mp4 = container.contains("mp4") || container.contains("mov");
    let webm = container.contains("webm") || container.contains("matroska");
    let codec_ok = match v.codec.as_str() {
        "h264" => mp4 && v.bit_depth == 8 && v.chroma == "4:2:0",
        "hevc" => mp4 && v.chroma == "4:2:0",
        "vp8" | "vp9" | "av1" => (webm || mp4) && v.chroma == "4:2:0",
        _ => false,
    };
    let audio_ok = info.audio.iter().all(|a| matches!(a.codec.as_str(), "aac" | "mp3" | "opus" | "vorbis" | "flac"));
    codec_ok && audio_ok && !info.proxy_required
}

pub fn media_source(path: &std::path::Path, info: &MediaInfo) -> Option<MediaSource> {
    let v = info.video.as_ref()?;
    Some(MediaSource {
        native_path: native_playable(info).then(|| path.to_string_lossy().into_owned()),
        duration_us: info.duration_us as f64,
        fps: v.fps.0.to_f64(),
        width: v.width,
        height: v.height,
        start_us: info.start_us as f64,
        codec: v.codec.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_uris() {
        assert_eq!(parse_frame_uri("/frame/3/1500000", Some("w=640")), Some(FrameRequest { kind: FrameKind::Jpeg, media: 3, us: 1_500_000, width: 640 }));
        assert_eq!(parse_frame_uri("frame/3/0", None), Some(FrameRequest { kind: FrameKind::Jpeg, media: 3, us: 0, width: DEFAULT_WIDTH }));
        assert_eq!(parse_frame_uri("/raw/3/0", None).map(|r| r.kind), Some(FrameKind::Raw));
        assert_eq!(parse_frame_uri("/frame/3/0", Some("t=1&w=320")).map(|r| r.width), Some(320));
        assert_eq!(parse_frame_uri("/frame/x/0", None), None);
        assert_eq!(parse_frame_uri("/frame/3/0/extra", None), None);
        assert_eq!(parse_frame_uri("/other/3/0", None), None);
    }

    #[test]
    fn serves_jpeg_frames_for_project_media() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let clip = root.join("testdata/gen/prores_hq.mov");
        if !clip.exists() {
            eprintln!("corpus missing: skipping");
            return;
        }
        let tmp = root.join("target/tmp-preview-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let opts = clippro_core::SessionOptions { cache_path: None, ..Default::default() };
        let s = Session::create(&tmp.join("p.clippro"), "p", opts).unwrap();
        s.import(&[clip]).unwrap();
        let media = s.clip_statuses().unwrap()[0].media.0;
        let (path, info) = s.preview_source(MediaId(media)).unwrap().unwrap();
        let src = media_source(&path, &info).unwrap();
        assert_eq!(src.native_path, None, "ProRes is served as frames");

        let cache = PreviewCache::default();
        let t0 = std::time::Instant::now();
        let mut sizes = Vec::new();
        for k in 0..24 {
            let jpeg = cache.frame_jpeg(&s, media, 500_000 + k * 40_000, 640).unwrap();
            assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "JPEG SOI");
            sizes.push(jpeg.len());
        }
        eprintln!("24 preview frames in {:?}, ~{} KB each", t0.elapsed(), sizes.iter().sum::<usize>() / sizes.len() / 1024);
        assert!(cache.frame_jpeg(&s, 999, 0, 640).is_err());
        let raw = cache.frame(&s, FrameKind::Raw, media, 1_000_000, 640).unwrap();
        assert_eq!(&raw[..4], b"CPF1");
        let w = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize;
        let h = u32::from_le_bytes(raw[8..12].try_into().unwrap()) as usize;
        assert_eq!(w, 640);
        assert_eq!(raw.len(), RAW_HEADER + w * h * 3 / 2);
        let pts = i64::from_le_bytes(raw[16..24].try_into().unwrap());
        assert!((pts - 1_000_000).abs() <= 21_000, "pts {pts}");
        drop(cache);
        s.close().unwrap();
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// ADR-002 numbers: sequential playback throughput of the frame transport. Gated by
    /// `CLIPPRO_PERF=1`; renders 1080p and 4K ProRes with the bundled ffmpeg.
    #[test]
    fn frame_transport_throughput() {
        if std::env::var("CLIPPRO_PERF").as_deref() != Ok("1") {
            return;
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let ffmpeg = root.join("third_party/ffmpeg/bin/ffmpeg.exe");
        let tmp = root.join("target/tmp-preview-bench");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let mut clips = Vec::new();
        for (name, size) in [("p1080.mov", "1920x1080"), ("p2160.mov", "3840x2160")] {
            let out = tmp.join(name);
            let st = std::process::Command::new(&ffmpeg)
                .args(["-v", "error", "-f", "lavfi", "-i", &format!("testsrc2=size={size}:rate=25:duration=4"), "-c:v", "prores_ks", "-profile:v", "2"])
                .arg(&out)
                .status()
                .unwrap();
            assert!(st.success());
            clips.push(out);
        }
        let dnx = root.join("testdata/gen/dnxhr.mxf");
        if dnx.exists() {
            clips.push(dnx);
        }
        let opts = clippro_core::SessionOptions { cache_path: None, ..Default::default() };
        let s = Session::create(&tmp.join("p.clippro"), "p", opts).unwrap();
        s.import(&clips).unwrap();
        let cache = PreviewCache::default();
        for st in s.clip_statuses().unwrap() {
            let (_, info) = s.preview_source(st.media).unwrap().unwrap();
            let v = info.video.clone().unwrap();
            for w in [960u32, 1920, v.width] {
                let n = 60;
                let _ = cache.frame_jpeg(&s, st.media.0, 0, w).unwrap(); // open + first seek
                let t0 = std::time::Instant::now();
                let mut bytes = 0;
                for k in 1..=n {
                    bytes += cache.frame_jpeg(&s, st.media.0, (k * 40_000) % info.duration_us, w).unwrap().len();
                }
                let per = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
                eprintln!("{:<14} {}x{} -> w{:<5} {:>6.1} ms/frame ({:>5.1} fps), {:>4} KB", st.name, v.width, v.height, w.min(v.width), per, 1000.0 / per, bytes / n as usize / 1024);
            }
            // Stage breakdown at full width: decode, colour conversion, JPEG.
            {
                let (path, info) = s.preview_source(st.media).unwrap().unwrap();
                let mut dec = VideoDecoder::open(&path, &info, &DecodeOptions { threads: 0, hw: HwMode::Auto }).unwrap();
                let (mut td, mut tc, mut te) = (0.0, 0.0, 0.0);
                let n = 30;
                for k in 0..n {
                    let t = std::time::Instant::now();
                    let f = dec.frame_near(k * 40_000).unwrap().unwrap();
                    td += t.elapsed().as_secs_f64();
                    let t = std::time::Instant::now();
                    let (w, h, rgb) = dec.to_rgb(&f, v.width).unwrap();
                    tc += t.elapsed().as_secs_f64();
                    let t = std::time::Instant::now();
                    let mut out = Vec::new();
                    encode_jpeg(&mut out, &rgb, w, h).unwrap();
                    te += t.elapsed().as_secs_f64();
                }
                let ms = |x: f64| x * 1000.0 / n as f64;
                eprintln!("{:<14} stages at full width: decode {:.1} ms, rgb {:.1} ms, jpeg {:.1} ms (hw {})", st.name, ms(td), ms(tc), ms(te), dec.stats.hw);
            }
            let t0 = std::time::Instant::now();
            for k in 0..10 {
                let _ = cache.frame_jpeg(&s, st.media.0, (((k * 7919) % 90) as i64 * 40_000 + 40_000) % info.duration_us, 960).unwrap();
            }
            eprintln!("{:<14} random seek + frame at w960: {:.1} ms", st.name, t0.elapsed().as_secs_f64() * 100.0);
        }
        drop(cache);
        s.close().unwrap();
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
