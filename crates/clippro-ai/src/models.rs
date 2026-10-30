//! Model manager (SPEC AI-1, PLAN P12.1): the manifest, where models live, resumable downloads
//! verified by SHA-256, and deletion. Nothing downloads without the user's consent, which the
//! app asks for before calling `download`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const MANIFEST: &str = include_str!("../models.json");

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    /// faces | clip | transcripts | speakers
    pub module: String,
    pub name: String,
    pub file: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub licence: String,
    pub source: String,
}

pub fn manifest() -> Vec<ModelInfo> {
    serde_json::from_str(MANIFEST).unwrap_or_default()
}

pub fn model(id: &str) -> Option<ModelInfo> {
    manifest().into_iter().find(|m| m.id == id)
}

/// `CLIPPRO_MODELS` when set (development and tests), else `<data folder>\models`
/// (`%LOCALAPPDATA%\ClipPro\models` unless the user chose another Data folder).
pub fn models_dir() -> PathBuf {
    if let Some(d) = models_env() {
        return d;
    }
    clippro_types::paths::data_root().join("models")
}

/// The `CLIPPRO_MODELS` override, if set.
pub fn models_env() -> Option<PathBuf> {
    std::env::var_os("CLIPPRO_MODELS").filter(|d| !d.is_empty()).map(PathBuf::from)
}

pub fn path_of(m: &ModelInfo) -> PathBuf {
    models_dir().join(&m.file)
}

/// Installed means present at the expected size (the hash is checked when it was downloaded).
pub fn installed(m: &ModelInfo) -> bool {
    std::fs::metadata(path_of(m)).map(|x| x.len() == m.size).unwrap_or(false)
}

/// The installed file of a model, if any.
pub fn installed_path(id: &str) -> Option<PathBuf> {
    model(id).filter(installed).map(|m| path_of(&m))
}

pub fn disk_use() -> u64 {
    manifest().iter().filter(|m| installed(m)).map(|m| m.size).sum()
}

pub fn sha256_file(p: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Downloads (or resumes) a model into the models folder and verifies its SHA-256.
/// `progress(done, total)`; `cancel` stops between chunks and keeps the partial file.
pub fn download(m: &ModelInfo, cancel: &AtomicBool, mut progress: impl FnMut(u64, u64)) -> Result<PathBuf, String> {
    let dir = models_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let out = path_of(m);
    if installed(m) {
        return Ok(out);
    }
    let part = dir.join(format!("{}.part", m.file));
    let have = std::fs::metadata(&part).map(|x| x.len()).unwrap_or(0).min(m.size);
    let mut req = ureq::get(&m.url);
    if have > 0 {
        req = req.header("Range", &format!("bytes={have}-"));
    }
    let resp = req.call().map_err(|e| format!("{}: {e}", m.url))?;
    let resumed = have > 0 && resp.status() == 206;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(&part)
        .map_err(|e| format!("{}: {e}", part.display()))?;
    let mut done = if resumed { have } else { 0 };
    let mut body = resp.into_body();
    let mut reader = body.as_reader();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let n = reader.read(&mut buf).map_err(|e| format!("download: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| format!("{}: {e}", part.display()))?;
        done += n as u64;
        progress(done, m.size);
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    let got = sha256_file(&part).map_err(|e| e.to_string())?;
    if got != m.sha256 {
        let _ = std::fs::remove_file(&part);
        return Err(format!("{}: checksum mismatch (got {got}); the download was discarded", m.name));
    }
    let _ = std::fs::remove_file(&out);
    std::fs::rename(&part, &out).map_err(|e| e.to_string())?;
    Ok(out)
}

pub fn delete(m: &ModelInfo) -> std::io::Result<()> {
    for p in [path_of(m), models_dir().join(format!("{}.part", m.file))] {
        if p.exists() {
            std::fs::remove_file(p)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_complete() {
        let m = manifest();
        assert!(m.len() >= 11);
        for x in &m {
            assert_eq!(x.sha256.len(), 64, "{}", x.id);
            assert!(x.size > 0 && x.url.starts_with("https://"), "{}", x.id);
            assert!(["MIT", "Apache-2.0"].contains(&x.licence.as_str()), "{} is {}", x.id, x.licence);
        }
        for module in ["faces", "clip", "transcripts", "speakers"] {
            assert!(m.iter().any(|x| x.module == module), "{module}");
        }
    }
}
