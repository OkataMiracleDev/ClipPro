//! `RunStore` backed by the project database (checkpoints, active area, results) and the
//! global measurement cache.

use clippro_analysis::jobs::{JobId, RunStore};
use clippro_measure::calibration::Calibration;
use clippro_measure::codec;
use clippro_measure::columns::MeasurementSet;
use clippro_measure::image::Rect;
use clippro_measure::sample::SampleRaw;
use clippro_project::{Db, MeasureCache, RunState};
use clippro_types::RunId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Per-job cache context: which content fingerprint / run key a job's result belongs to.
#[derive(Clone, Debug)]
pub struct CacheKey {
    pub content_fingerprint: String,
    pub run_key: String,
}

pub struct ProjectStore {
    pub db: Arc<Mutex<Db>>,
    pub cache: Option<Arc<Mutex<MeasureCache>>>,
    pub cal: Calibration,
    pub keys: Mutex<HashMap<JobId, CacheKey>>,
    /// Errors from storage callbacks (the scheduler can't return them); surfaced by the session.
    pub errors: Mutex<Vec<String>>,
}

impl ProjectStore {
    fn note<T, E: std::fmt::Display>(&self, r: Result<T, E>) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                lock(&self.errors).push(e.to_string());
                None
            }
        }
    }

    fn run(job: JobId) -> RunId {
        RunId(job as i64)
    }
}

impl RunStore for ProjectStore {
    fn checkpoints(&self, job: JobId) -> Vec<(u32, Vec<SampleRaw>)> {
        let chunks = {
            let db = lock(&self.db);
            self.note(db.chunks(Self::run(job))).unwrap_or_default()
        };
        let mut out: Vec<(u32, Vec<SampleRaw>)> = Vec::new();
        for ch in chunks {
            let Some(samples) = self.note(codec::decode_samples(&ch.data)) else { continue };
            match out.last_mut() {
                Some((s, all)) if *s == ch.section => all.extend(samples),
                _ => out.push((ch.section, samples)),
            }
        }
        out
    }

    fn save_checkpoint(&self, job: JobId, section: u32, chunk_index: u32, samples: &[SampleRaw]) {
        let Some(blob) = self.note(codec::encode_samples(samples)) else { return };
        let first = samples.first().map(|s| s.index).unwrap_or(0);
        let db = lock(&self.db);
        self.note(db.save_chunk(Self::run(job), section, chunk_index, first, samples.len(), &blob));
    }

    fn active_area(&self, job: JobId) -> Option<Rect> {
        let db = lock(&self.db);
        let run = self.note(db.run(Self::run(job)))??;
        serde_json::from_str(run.active_json.as_deref()?).ok()
    }

    fn save_active_area(&self, job: JobId, area: Rect) {
        let json = serde_json::to_string(&area).unwrap_or_default();
        let db = lock(&self.db);
        self.note(db.set_run_meta(Self::run(job), &json, ""));
        self.note(db.set_run_state(Self::run(job), RunState::Running, None));
    }

    fn finished(&self, job: JobId, ms: &MeasurementSet) {
        let Some(blob) = self.note(codec::encode(ms, &self.cal)) else { return };
        let summary = serde_json::json!({ "samples": ms.len(), "rate_hz": ms.rate_hz }).to_string();
        {
            let mut db = lock(&self.db);
            self.note(db.save_measurements(Self::run(job), &blob, &summary));
        }
        if let (Some(cache), Some(key)) = (&self.cache, lock(&self.keys).get(&job).cloned()) {
            self.note(lock(cache).put(&key.content_fingerprint, &key.run_key, &blob));
        }
    }

    fn audio_finished(&self, job: JobId, audio: &clippro_audio::AudioSet) {
        let summary = serde_json::json!({
            "blocks": audio.len(),
            "integrated_lufs": audio.integrated_lufs(i64::MIN / 2, i64::MAX / 2),
        })
        .to_string();
        let db = lock(&self.db);
        self.note(db.save_audio(Self::run(job), &audio.encode(), &summary));
    }

    fn failed(&self, job: JobId, error: &str) {
        let db = lock(&self.db);
        self.note(db.set_run_state(Self::run(job), RunState::Failed, Some(error)));
    }
}
