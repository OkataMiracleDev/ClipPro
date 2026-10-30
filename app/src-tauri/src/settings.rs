//! App-wide settings (SPEC OTH-1, PLAN P5.7): General, Performance and Analysis defaults.
//! Stored as JSON in the app config directory; analysis and performance values apply to
//! projects opened or created after the change.

use clippro_core::SessionOptions;
use clippro_media::video::HwMode;
use clippro_types::SampleRate;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Type, Clone, Debug, PartialEq, tauri_specta::Event)]
#[serde(default)]
pub struct AppSettings {
    /// system | light | dark
    pub theme: String,
    /// mac (traffic lights, left) | windows (minimise/maximise/close, right)
    pub window_controls: String,
    /// Open the last project at start-up.
    pub reopen_last: bool,
    pub last_project: Option<String>,
    /// Most recent first, at most 10.
    pub recent_projects: Vec<String>,
    /// Analysis workers; 0 = automatic (by CPU count).
    pub workers: u32,
    /// Decode on the GPU (D3D11VA) when the codec allows.
    pub gpu_decode: bool,
    /// Reuse measurements across projects (global cache).
    pub use_cache: bool,
    /// draft | fast | standard | detailed | thorough | all
    pub sample_rate: String,
    /// Measure motion from frame pairs (more accurate shake and blur).
    pub pair_sampling: bool,
    /// Rebound shortcuts: action id → key combo (only those that differ from the defaults).
    pub keybindings: std::collections::BTreeMap<String, String>,
    /// Score colours: oklch red→green, or blue→orange for colour-blind viewers.
    pub colour_blind_scores: bool,
    /// The first-run tour has been seen (P14.4).
    pub tour_done: bool,
    /// Local AI is allowed: models may be downloaded and run on this PC (SPEC §18).
    pub ai_consent: bool,
    /// Faces and People.
    pub ai_faces: bool,
    /// Shot types, aesthetics and visual search (CLIP).
    pub ai_shots: bool,
    pub ai_transcripts: bool,
    pub ai_speakers: bool,
    /// Whisper model id: whisper-tiny | whisper-base | whisper-small | whisper-medium | whisper-large-v3-turbo
    pub whisper_model: String,
    /// Transcript language (ISO 639-1); empty = detect.
    pub ai_language: String,
    /// CPU only, repeatable results.
    pub ai_deterministic: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            theme: "system".into(),
            window_controls: "mac".into(),
            reopen_last: true,
            last_project: None,
            recent_projects: Vec::new(),
            workers: 0,
            gpu_decode: true,
            use_cache: true,
            sample_rate: "standard".into(),
            pair_sampling: true,
            keybindings: Default::default(),
            colour_blind_scores: false,
            tour_done: false,
            ai_consent: false,
            ai_faces: true,
            ai_shots: true,
            ai_transcripts: true,
            ai_speakers: true,
            whisper_model: "whisper-small".into(),
            ai_language: String::new(),
            ai_deterministic: false,
        }
    }
}

impl AppSettings {
    pub fn load(path: &Path) -> AppSettings {
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).map(AppSettings::sanitised).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    /// Unknown values fall back to defaults; numbers are clamped.
    pub fn sanitised(mut self) -> AppSettings {
        let d = AppSettings::default();
        if !["system", "light", "dark"].contains(&self.theme.as_str()) {
            self.theme = d.theme;
        }
        if !["mac", "windows"].contains(&self.window_controls.as_str()) {
            self.window_controls = d.window_controls;
        }
        if SampleRate::parse(&self.sample_rate).is_none() {
            self.sample_rate = d.sample_rate;
        }
        if !self.whisper_model.starts_with("whisper-") || clippro_ai::models::model(&self.whisper_model).is_none() {
            self.whisper_model = d.whisper_model;
        }
        if self.ai_language.len() > 3 || !self.ai_language.chars().all(|c| c.is_ascii_lowercase()) {
            self.ai_language = String::new();
        }
        self.workers = self.workers.min(64);
        self.recent_projects.truncate(10);
        self
    }

    /// Session options for projects opened with these settings.
    pub fn session_options(&self) -> SessionOptions {
        let mut o = SessionOptions::default();
        if self.workers > 0 {
            o.scheduler.workers = self.workers as usize;
        }
        o.analyze.rate = SampleRate::parse(&self.sample_rate).unwrap_or_default();
        o.analyze.pair_sampling = self.pair_sampling;
        o.analyze.decode.hw = if self.gpu_decode { HwMode::Auto } else { HwMode::Off };
        if !self.use_cache {
            o.cache_path = None;
        }
        o
    }
}

pub fn settings_path(config_dir: PathBuf) -> PathBuf {
    config_dir.join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_sanitise_and_options() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-settings-test");
        let _ = std::fs::remove_dir_all(&dir);
        let p = settings_path(dir.clone());
        assert_eq!(AppSettings::load(&p), AppSettings::default(), "missing file → defaults");
        let s = AppSettings { theme: "dark".into(), workers: 3, gpu_decode: false, sample_rate: "detailed".into(), use_cache: false, ..Default::default() };
        s.save(&p).unwrap();
        assert_eq!(AppSettings::load(&p), s);
        std::fs::write(&p, r#"{"theme":"neon","workers":999,"sample_rate":"x","future_key":1}"#).unwrap();
        let l = AppSettings::load(&p);
        assert_eq!((l.theme.as_str(), l.workers, l.sample_rate.as_str()), ("system", 64, "standard"));
        let o = s.session_options();
        assert_eq!(o.scheduler.workers, 3);
        assert_eq!(o.analyze.rate, SampleRate::Detailed);
        assert_eq!(o.analyze.decode.hw, HwMode::Off);
        assert!(o.cache_path.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
