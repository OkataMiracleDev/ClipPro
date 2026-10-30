//! The Data folder (Settings → General): where AI models and the measurement cache live. The
//! default is `%LOCALAPPDATA%\ClipPro`; moving it carries the existing files along so nothing is
//! downloaded or analysed again.

use crate::{lock, AppState};
use clippro_types::paths;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};
use tauri::State;
use tauri_specta::Event;

const CACHE_FILES: [&str; 3] = ["measure-cache.sqlite", "measure-cache.sqlite-wal", "measure-cache.sqlite-shm"];

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct DataInfo {
    /// The data folder in effect.
    pub root: String,
    pub default_root: String,
    pub is_default: bool,
    pub models_dir: String,
    /// `CLIPPRO_MODELS` overrides the models folder (development setups).
    pub models_env: bool,
    pub cache_path: String,
    /// The cache waits for the next start-up to move (a project was open).
    pub cache_move_pending: bool,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct DataMoveResult {
    pub info: DataInfo,
    /// Files moved now.
    pub moved: u32,
    pub notes: Vec<String>,
}

/// Applies the settings' data folder to this process (start-up and after a change).
pub fn apply(data_dir: Option<&str>) {
    paths::set_data_root(data_dir.map(PathBuf::from));
}

pub fn info(state: &AppState) -> DataInfo {
    let root = paths::data_root();
    DataInfo {
        is_default: root == paths::default_data_root(),
        root: root.display().to_string(),
        default_root: paths::default_data_root().display().to_string(),
        models_dir: clippro_ai::models::models_dir().display().to_string(),
        models_env: clippro_ai::models::models_env().is_some(),
        cache_path: clippro_project::MeasureCache::default_path().display().to_string(),
        cache_move_pending: lock(&state.settings).pending_cache_move.is_some(),
    }
}

/// Moves `from` to `to`: a rename on the same drive, else copy then delete. An identical file
/// already at the destination (same size) is kept and the source removed; a different one is
/// left alone and reported.
fn move_file(from: &Path, to: &Path) -> std::io::Result<bool> {
    if !from.is_file() {
        return Ok(false);
    }
    if let Some(d) = to.parent() {
        std::fs::create_dir_all(d)?;
    }
    if let (Ok(a), Ok(b)) = (from.metadata(), to.metadata()) {
        if a.len() == b.len() {
            std::fs::remove_file(from)?;
            return Ok(false);
        }
        return Err(std::io::Error::other(format!("{} already exists with different content", to.display())));
    }
    if std::fs::rename(from, to).is_err() {
        let tmp = to.with_extension("clippro-part");
        std::fs::copy(from, &tmp)?;
        std::fs::rename(&tmp, to)?;
        std::fs::remove_file(from)?;
    }
    Ok(true)
}

/// Moves the measurement cache (the database and its WAL files) between cache paths.
pub fn move_cache(from: &Path, to: &Path) -> std::io::Result<u32> {
    let (fd, td) = (from.parent().unwrap_or(Path::new(".")), to.parent().unwrap_or(Path::new(".")));
    let mut n = 0;
    for f in CACHE_FILES {
        n += move_file(&fd.join(f), &td.join(f))? as u32;
    }
    Ok(n)
}

/// Start-up: finishes a cache move that waited for its project to close.
pub fn finish_pending(state: &AppState) {
    let pending = lock(&state.settings).pending_cache_move.clone();
    let Some(from) = pending else { return };
    let to = clippro_project::MeasureCache::default_path();
    match move_cache(Path::new(&from), &to) {
        Ok(_) => crate::diag::log("INFO", "measurement cache moved to the data folder"),
        Err(e) => crate::diag::log("WARN", &format!("couldn't move the measurement cache: {e}")),
    }
    let mut st = lock(&state.settings);
    st.pending_cache_move = None;
    if let Some(p) = lock(&state.settings_path).as_ref() {
        let _ = st.save(p);
    }
}

#[tauri::command]
#[specta::specta]
pub fn data_info(state: State<'_, AppState>) -> DataInfo {
    info(&state)
}

/// Changes the data folder (`None` = the default) and, with `move_existing`, moves the models
/// and the measurement cache into it. A cache in use by the open project moves at next start-up.
#[tauri::command]
#[specta::specta]
pub fn data_dir_set(app: tauri::AppHandle, state: State<'_, AppState>, path: Option<String>, move_existing: bool) -> Result<DataMoveResult, String> {
    if lock(&state.ai).is_some() || !lock(&state.ai_downloads).is_empty() {
        return Err("Wait for the AI analysis and model downloads to finish, or stop them first.".into());
    }
    let new_root = match path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => {
            let p = PathBuf::from(p);
            if !p.is_absolute() {
                return Err("Choose a full folder path.".into());
            }
            // It must be creatable and writable.
            std::fs::create_dir_all(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            let probe = p.join(".clippro-write-test");
            std::fs::write(&probe, b"ok").map_err(|e| format!("{} isn't writable: {e}", p.display()))?;
            let _ = std::fs::remove_file(probe);
            Some(p)
        }
        None => None,
    };
    let (old_models, old_cache) = (clippro_ai::models::models_dir(), clippro_project::MeasureCache::default_path());
    let new_value = new_root.as_ref().filter(|p| **p != paths::default_data_root()).map(|p| p.display().to_string());
    apply(new_value.as_deref());
    let (new_models, new_cache) = (clippro_ai::models::models_dir(), clippro_project::MeasureCache::default_path());
    let mut moved = 0;
    let mut notes = Vec::new();
    let mut pending = None;
    if move_existing {
        if old_models != new_models {
            for m in clippro_ai::models::manifest() {
                match move_file(&old_models.join(&m.file), &new_models.join(&m.file)) {
                    Ok(true) => moved += 1,
                    Ok(false) => {}
                    Err(e) => notes.push(format!("{}: {e}", m.name)),
                }
            }
        } else if clippro_ai::models::models_env().is_some() {
            notes.push("The models stay where CLIPPRO_MODELS points.".into());
        }
        if old_cache != new_cache && old_cache.exists() {
            if lock(&state.session).is_some() {
                pending = Some(old_cache.display().to_string());
                notes.push("The measurement cache moves the next time ClipPro starts (a project is using it now).".into());
            } else {
                match move_cache(&old_cache, &new_cache) {
                    Ok(n) => moved += n,
                    Err(e) => notes.push(format!("Measurement cache: {e}")),
                }
            }
        }
    }
    {
        let mut st = lock(&state.settings);
        st.data_dir = new_value;
        if pending.is_some() {
            st.pending_cache_move = pending;
        }
        if let Some(p) = lock(&state.settings_path).as_ref() {
            st.save(p)?;
        }
        let _ = st.emit(&app);
    }
    crate::diag::log("INFO", &format!("data folder set to {}", paths::data_root().display()));
    Ok(DataMoveResult { info: info(&state), moved, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_files_and_keeps_identical_destinations() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-data-move");
        let _ = std::fs::remove_dir_all(&dir);
        let (a, b) = (dir.join("a"), dir.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        for f in CACHE_FILES {
            std::fs::write(a.join(f), f).unwrap();
        }
        assert_eq!(move_cache(&a.join(CACHE_FILES[0]), &b.join(CACHE_FILES[0])).unwrap(), 3);
        assert!(!a.join(CACHE_FILES[0]).exists() && b.join(CACHE_FILES[1]).exists());
        // The same file already there: the source goes, the destination stays.
        std::fs::write(a.join("m.bin"), b"1234").unwrap();
        std::fs::write(b.join("m.bin"), b"1234").unwrap();
        assert!(!move_file(&a.join("m.bin"), &b.join("m.bin")).unwrap());
        assert!(!a.join("m.bin").exists() && b.join("m.bin").exists());
        // A different file there: refused, nothing lost.
        std::fs::write(a.join("m.bin"), b"12").unwrap();
        assert!(move_file(&a.join("m.bin"), &b.join("m.bin")).is_err());
        assert!(a.join("m.bin").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
