//! Render selects to files (SPEC §19, PLAN P13.3): stream copy when the in point is a keyframe
//! (or, if allowed, from the keyframe before it), else a re-encode. Source timecode at the
//! rendered start is embedded; names come from a template.
//!
//! A re-encode is frame-exact. A stream copy ends on a decode-order packet, so with B-frames it
//! can run up to the codec's reorder delay (typically 2 frames) past the out point; cutting any
//! earlier would drop B-frames before it.

use crate::ffmpeg;
use crate::proxy::tc_reel_args;
use clippro_types::{FrameRate, Timecode};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RenderCodec {
    #[default]
    Prores422,
    ProresHq,
    DnxhrHq,
    H264,
}

impl RenderCodec {
    fn video_args(self) -> Result<Vec<String>, String> {
        let pick = |c: &[&'static str]| c.iter().copied().find(|e| ffmpeg::encoder_works(e)).ok_or_else(|| "No working encoder for this codec on this computer.".to_string());
        let v: Vec<String> = match self {
            RenderCodec::Prores422 => ["-c:v", pick(&["prores_ks"])?, "-profile:v", "2", "-pix_fmt", "yuv422p10le", "-vendor", "apl0"].map(String::from).to_vec(),
            RenderCodec::ProresHq => ["-c:v", pick(&["prores_ks"])?, "-profile:v", "3", "-pix_fmt", "yuv422p10le", "-vendor", "apl0"].map(String::from).to_vec(),
            RenderCodec::DnxhrHq => ["-c:v", pick(&["dnxhd"])?, "-profile:v", "dnxhr_hq", "-pix_fmt", "yuv422p"].map(String::from).to_vec(),
            RenderCodec::H264 => {
                let e = pick(&["h264_nvenc", "h264_amf", "h264_qsv", "h264_mf", "libopenh264"])?;
                ["-c:v", e, "-b:v", "40M", "-pix_fmt", if e == "libopenh264" { "yuv420p" } else { "nv12" }].map(String::from).to_vec()
            }
        };
        Ok(v)
    }

    fn audio_args(self) -> Vec<String> {
        if self == RenderCodec::H264 { ["-c:a", "aac", "-b:a", "256k"] } else { ["-c:a", "pcm_s24le", "-ar", "48000"] }.map(String::from).to_vec()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenderOptions {
    /// Try stream copy first.
    pub stream_copy: bool,
    /// Stream copy may start at the keyframe before the in point.
    pub allow_nearest_keyframe: bool,
    /// Used whenever stream copy isn't possible (or wanted).
    pub codec: RenderCodec,
    pub out_dir: PathBuf,
    /// Tokens: {clip} {index} {tc_in} {tc_out} {score} {rating} {label}.
    pub template: String,
}

/// One select to render; times in µs from the source's first video frame.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderItem {
    pub src: PathBuf,
    pub start_us: i64,
    pub end_us: i64,
    pub rate: FrameRate,
    /// Source start timecode (frames at `rate`) and drop-frame flag.
    pub start_tc_frames: Option<i64>,
    pub drop_frame: bool,
    pub reel: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenderOutcome {
    pub path: PathBuf,
    pub copied: bool,
    /// The rendered span (stream copy may start earlier than asked).
    pub start_us: i64,
    pub end_us: i64,
}

/// Fills a filename template; unsafe filename characters become '-'.
pub fn file_name(template: &str, tokens: &[(&str, String)]) -> String {
    let mut s = if template.trim().is_empty() { "{clip}_{index}_{tc_in}".to_string() } else { template.to_string() };
    for (k, v) in tokens {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    let s: String = s.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '-' } else { c }).collect();
    let s = s.trim().trim_end_matches('.').to_string();
    if s.is_empty() { "select".into() } else { s }
}

/// Timecode label at `us` into the source.
pub fn tc_at(item: &RenderItem, us: i64) -> Option<String> {
    let base = item.start_tc_frames?;
    Some(Timecode::from_frames(base + item.rate.us_to_frames_round(us), item.rate, item.drop_frame).to_string())
}

/// Where stream copy would start: the in point if it's a keyframe (within half a frame), the
/// keyframe before it if allowed, else `None` (re-encode).
pub fn copy_start(keyframes: &[i64], start_us: i64, half_frame_us: i64, allow_nearest: bool) -> Option<i64> {
    let i = keyframes.partition_point(|k| *k <= start_us + half_frame_us);
    let k = *keyframes.get(i.checked_sub(1)?)?;
    if (start_us - k).abs() <= half_frame_us {
        Some(k)
    } else if allow_nearest {
        Some(k)
    } else {
        None
    }
}

/// Renders one select to `out` (`.mov`).
pub fn render(item: &RenderItem, keyframes: &[i64], opts: &RenderOptions, out: &Path, cancel: &AtomicBool, progress: impl FnMut(f32)) -> Result<RenderOutcome, String> {
    if item.end_us <= item.start_us {
        return Err("The select has no length.".into());
    }
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let half = (item.rate.frame_duration_us() / 2.0).round() as i64;
    let copy_from = if opts.stream_copy { copy_start(keyframes, item.start_us, half, opts.allow_nearest_keyframe) } else { None };
    let start = copy_from.unwrap_or(item.start_us);
    let dur = item.end_us - start;
    let secs = |us: i64| format!("{:.6}", us as f64 / 1e6);
    let mut args: Vec<String> = vec!["-ss".into(), secs(start), "-i".into(), item.src.to_string_lossy().into_owned(), "-t".into(), secs(dur)];
    args.extend(["-map".into(), "0:v:0".into(), "-map".into(), "0:a?".into()]);
    if copy_from.is_some() {
        args.extend(["-c".into(), "copy".into(), "-avoid_negative_ts".into(), "make_zero".into()]);
    } else {
        args.extend(opts.codec.video_args()?);
        args.extend(opts.codec.audio_args());
    }
    args.extend(tc_reel_args(tc_at(item, start).as_deref(), item.reel.as_deref()));
    let tmp = out.with_extension("partial.mov");
    args.extend(["-f".into(), "mov".into(), tmp.to_string_lossy().into_owned()]);
    match ffmpeg::run(&args, dur, cancel, progress) {
        Ok(()) => {
            let _ = std::fs::remove_file(out);
            std::fs::rename(&tmp, out).map_err(|e| format!("{}: {e}", out.display()))?;
            Ok(RenderOutcome { path: out.to_path_buf(), copied: copy_from.is_some(), start_us: start, end_us: item.end_us })
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_copy_points() {
        let n = file_name("{clip}_{index}_{tc_in}", &[("clip", "A001C003".into()), ("index", "007".into()), ("tc_in", "10:00:01:12".into())]);
        assert_eq!(n, "A001C003_007_10-00-01-12");
        assert_eq!(file_name("  ", &[("clip", "x".into()), ("index", "1".into()), ("tc_in", "".into())]), "x_1_");
        assert_eq!(file_name("a/b:c?.", &[]), "a-b-c-");
        let k = [0, 2_000_000, 4_000_000];
        assert_eq!(copy_start(&k, 2_010_000, 20_000, false), Some(2_000_000), "within half a frame");
        assert_eq!(copy_start(&k, 2_500_000, 20_000, false), None);
        assert_eq!(copy_start(&k, 2_500_000, 20_000, true), Some(2_000_000));
        assert_eq!(copy_start(&k, 1_990_000, 20_000, false), Some(2_000_000), "just before a keyframe still counts");
        assert_eq!(copy_start(&[], 0, 20_000, true), None);
    }
}
