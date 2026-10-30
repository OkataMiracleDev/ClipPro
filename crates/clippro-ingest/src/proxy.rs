//! Proxy generation (SPEC §19, PLAN P13.2) with LGPL encoders only, keeping timecode, reel and
//! audio. Proxies are named `<clip>_Proxy.mov`, which the importer's proxy matcher pairs with
//! the original.

use crate::ffmpeg;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProxyCodec {
    #[default]
    ProresProxy,
    ProresLt,
    DnxhrLb,
    /// H.264 on the GPU's encoder (NVENC, AMF, Quick Sync, Media Foundation), else OpenH264.
    H264,
}

impl ProxyCodec {
    pub fn label(self) -> &'static str {
        match self {
            ProxyCodec::ProresProxy => "ProRes 422 Proxy",
            ProxyCodec::ProresLt => "ProRes 422 LT",
            ProxyCodec::DnxhrLb => "DNxHR LB",
            ProxyCodec::H264 => "H.264",
        }
    }

    /// The encoder this codec uses here, if any works.
    pub fn encoder(self) -> Option<&'static str> {
        let candidates: &[&'static str] = match self {
            ProxyCodec::ProresProxy | ProxyCodec::ProresLt => &["prores_ks"],
            ProxyCodec::DnxhrLb => &["dnxhd"],
            ProxyCodec::H264 => &["h264_nvenc", "h264_amf", "h264_qsv", "h264_mf", "libopenh264"],
        };
        candidates.iter().copied().find(|e| ffmpeg::encoder_works(e))
    }

    /// Video encoder arguments.
    pub(crate) fn video_args(self, encoder: &str) -> Vec<String> {
        let v: Vec<&str> = match self {
            ProxyCodec::ProresProxy => vec!["-c:v", encoder, "-profile:v", "0", "-pix_fmt", "yuv422p10le", "-vendor", "apl0"],
            ProxyCodec::ProresLt => vec!["-c:v", encoder, "-profile:v", "1", "-pix_fmt", "yuv422p10le", "-vendor", "apl0"],
            ProxyCodec::DnxhrLb => vec!["-c:v", encoder, "-profile:v", "dnxhr_lb", "-pix_fmt", "yuv422p"],
            ProxyCodec::H264 => vec!["-c:v", encoder, "-b:v", "8M", "-pix_fmt", if encoder == "libopenh264" { "yuv420p" } else { "nv12" }],
        };
        v.into_iter().map(String::from).collect()
    }

    pub(crate) fn audio_args(self) -> Vec<String> {
        let v: &[&str] = if self == ProxyCodec::H264 { &["-c:a", "aac", "-b:a", "192k"] } else { &["-c:a", "pcm_s16le"] };
        v.iter().map(|s| s.to_string()).collect()
    }
}

/// Codecs with a working encoder on this machine.
pub fn available() -> Vec<ProxyCodec> {
    [ProxyCodec::ProresProxy, ProxyCodec::ProresLt, ProxyCodec::DnxhrLb, ProxyCodec::H264].into_iter().filter(|c| c.encoder().is_some()).collect()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProxyOptions {
    pub codec: ProxyCodec,
    /// Scale down to at most this height (never up).
    pub max_height: u32,
    /// `None`: a `Proxy` folder beside the source.
    pub out_dir: Option<PathBuf>,
}

impl Default for ProxyOptions {
    fn default() -> Self {
        ProxyOptions { codec: ProxyCodec::ProresProxy, max_height: 1080, out_dir: None }
    }
}

/// What the proxy must carry over from its source.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProxySource {
    pub path: PathBuf,
    pub duration_us: i64,
    pub start_tc: Option<String>,
    pub reel: Option<String>,
}

pub fn proxy_path(src: &Path, opts: &ProxyOptions) -> PathBuf {
    let dir = opts.out_dir.clone().unwrap_or_else(|| src.parent().map(|p| p.join("Proxy")).unwrap_or_default());
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "clip".into());
    dir.join(format!("{stem}_Proxy.mov"))
}

/// Shared tail of an encode: timecode, reel and container metadata.
pub(crate) fn tc_reel_args(tc: Option<&str>, reel: Option<&str>) -> Vec<String> {
    let mut a = vec!["-map_metadata".to_string(), "0".to_string()];
    if let Some(tc) = tc.filter(|t| !t.is_empty()) {
        a.extend(["-timecode".to_string(), tc.to_string()]);
    }
    if let Some(r) = reel.filter(|r| !r.is_empty()) {
        a.extend(["-metadata:s:v:0".to_string(), format!("reel_name={r}")]);
    }
    a
}

/// Writes a proxy (via a temporary name, so a cancelled or failed encode leaves nothing).
pub fn make_proxy(src: &ProxySource, opts: &ProxyOptions, cancel: &AtomicBool, progress: impl FnMut(f32)) -> Result<PathBuf, String> {
    let encoder = opts.codec.encoder().ok_or_else(|| format!("No working {} encoder on this computer.", opts.codec.label()))?;
    let out = proxy_path(&src.path, opts);
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let tmp = out.with_extension("partial.mov");
    let mut args: Vec<String> = vec!["-i".into(), src.path.to_string_lossy().into_owned(), "-map".into(), "0:v:0".into(), "-map".into(), "0:a?".into()];
    args.extend(["-vf".into(), format!("scale=-2:'min(ih,{})'", opts.max_height.max(144) / 2 * 2)]);
    args.extend(opts.codec.video_args(encoder));
    args.extend(opts.codec.audio_args());
    args.extend(tc_reel_args(src.start_tc.as_deref(), src.reel.as_deref()));
    args.extend(["-f".into(), "mov".into(), tmp.to_string_lossy().into_owned()]);
    match ffmpeg::run(&args, src.duration_us, cancel, progress) {
        Ok(()) => {
            let _ = std::fs::remove_file(&out);
            std::fs::rename(&tmp, &out).map_err(|e| format!("{}: {e}", out.display()))?;
            Ok(out)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}
