//! Where ClipPro keeps its per-user data: AI models and the measurement cache. The default is
//! `%LOCALAPPDATA%\ClipPro`; the app replaces it with the user's Data folder setting at start-up.

use std::path::PathBuf;
use std::sync::RwLock;

static ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// `%LOCALAPPDATA%\ClipPro` (the temp folder if that variable is missing).
pub fn default_data_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("ClipPro")
}

/// The data folder in effect.
pub fn data_root() -> PathBuf {
    ROOT.read().ok().and_then(|r| r.clone()).unwrap_or_else(default_data_root)
}

/// Sets the data folder for this process; `None` restores the default.
pub fn set_data_root(root: Option<PathBuf>) {
    if let Ok(mut r) = ROOT.write() {
        *r = root;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_and_default() {
        set_data_root(Some(PathBuf::from(r"D:\Data")));
        assert_eq!(data_root(), PathBuf::from(r"D:\Data"));
        set_data_root(None);
        assert_eq!(data_root(), default_data_root());
        assert!(default_data_root().ends_with("ClipPro"));
    }
}
