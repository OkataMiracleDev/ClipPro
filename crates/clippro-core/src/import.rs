//! Media discovery, proxy matching and relinking (SPEC ING-1, ING-3, ING-9, PRJ-3).

use clippro_media::MediaInfo;
use std::path::{Path, PathBuf};

/// Extensions ClipPro imports (SPEC §9.2), lower-case.
pub const MEDIA_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "m4v", "mkv", "mxf", "m2ts", "mts", "avi", "webm", "dv", "mpg", "mpeg", "ts", "3gp", "braw", "r3d", "ari",
    "arx", "crm", "nev", "wav", "mp3", "m4a", "aif", "aiff",
];

pub fn is_media(p: &Path) -> bool {
    p.extension().map(|e| MEDIA_EXTENSIONS.contains(&e.to_string_lossy().to_lowercase().as_str())).unwrap_or(false)
}

/// Expands files and folders (recursively) into media files, sorted, skipping hidden entries.
pub fn discover(inputs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            let hidden = p.file_name().map(|n| n.to_string_lossy().starts_with('.')).unwrap_or(false);
            if hidden {
                continue;
            }
            if p.is_dir() {
                walk(&p, out);
            } else if is_media(&p) {
                out.push(p);
            }
        }
    }
    for i in inputs {
        if i.is_dir() {
            walk(i, &mut out);
        } else if i.is_file() {
            out.push(i.clone());
        }
    }
    out.dedup();
    out
}

/// Base name with common proxy suffixes removed ("A001_Proxy.mov" → "a001").
pub fn proxy_base(p: &Path) -> String {
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut s = stem.as_str();
    for suf in ["_proxy", "-proxy", ".proxy", " proxy", "_prx", "_lowres"] {
        if let Some(t) = s.strip_suffix(suf) {
            s = t;
        }
    }
    s.to_string()
}

pub fn looks_like_proxy(p: &Path) -> bool {
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let in_proxy_dir = p
        .parent()
        .and_then(|d| d.file_name())
        .map(|n| matches!(n.to_string_lossy().to_lowercase().as_str(), "proxy" | "proxies" | "proxy media"))
        .unwrap_or(false);
    proxy_base(p) != stem || in_proxy_dir
}

/// Whether `proxy` matches `original` (SPEC §19): same base name, duration within one frame,
/// and the same start timecode when both have one.
pub fn proxy_matches(original: (&Path, &MediaInfo), proxy: (&Path, &MediaInfo)) -> bool {
    if proxy_base(original.0) != proxy_base(proxy.0) {
        return false;
    }
    let frame = original.1.fps().frame_duration_us() as i64;
    if (original.1.duration_us - proxy.1.duration_us).abs() > frame + 1000 {
        return false;
    }
    match (&original.1.start_timecode, &proxy.1.start_timecode) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

/// Finds a moved file under `roots` by file name, confirmed by content fingerprint.
pub fn relink_search(name: &str, content_fingerprint: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    let mut found = None;
    for f in discover(roots) {
        if f.file_name().map(|n| n.to_string_lossy() == name).unwrap_or(false) {
            if let Ok(fp) = clippro_media::fingerprint::content_fingerprint(&f) {
                if fp == content_fingerprint {
                    found = Some(f);
                    break;
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_names() {
        assert_eq!(proxy_base(Path::new("D:/x/A001C003_Proxy.mov")), "a001c003");
        assert!(looks_like_proxy(Path::new("D:/x/A001C003_Proxy.mov")));
        assert!(looks_like_proxy(Path::new("D:/x/Proxy/A001C003.mov")));
        assert!(!looks_like_proxy(Path::new("D:/x/A001C003.mov")));
        assert!(is_media(Path::new("a.MXF")));
        assert!(!is_media(Path::new("a.txt")));
    }
}
