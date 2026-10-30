//! A project session: the database, the analysis scheduler and the scoring engine together.
//! Used by both `clippro-cli` and the desktop app.

use crate::import::{discover, looks_like_proxy, proxy_matches};
use crate::store::{lock, CacheKey, ProjectStore};
use clippro_analysis::clip::{seed_from_fingerprint, AnalyzeOptions};
use clippro_analysis::jobs::{Event, JobSpec, JobStatus, Scheduler, SchedulerConfig};
use clippro_measure::calibration::Calibration;
use clippro_measure::codec;
use clippro_measure::columns::MeasurementSet;
use clippro_media::{fingerprint, probe, MediaInfo};
use clippro_project::{Db, MeasureCache, MediaRow, NewMedia, OpenReport, RunParams, RunState};
use clippro_interchange::{ExportOptions, Format, MediaRef, SelectInput, Timeline};
use clippro_score::{ClipInput, Engine, Override, OverrideKind, Selects, Settings, SortOrder};
use clippro_types::{ClipId, ColourProfile, FlagKind, MediaId, Timecode};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error(transparent)]
    Project(#[from] clippro_project::ProjectError),
    #[error(transparent)]
    Media(#[from] clippro_media::MediaError),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, SessionError>;

#[derive(Clone, Debug)]
pub struct SessionOptions {
    pub analyze: AnalyzeOptions,
    pub scheduler: SchedulerConfig,
    /// Global measurement cache location (SPEC ING-8); `None` disables it.
    pub cache_path: Option<PathBuf>,
}

impl Default for SessionOptions {
    fn default() -> Self {
        SessionOptions {
            analyze: AnalyzeOptions::default(),
            scheduler: SchedulerConfig::default(),
            cache_path: Some(MeasureCache::default_path()),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ImportReport {
    pub added: Vec<(PathBuf, ClipId)>,
    /// Files whose content is already in the project (SPEC ING-9); not added again.
    pub duplicates: Vec<(PathBuf, MediaId)>,
    /// (original, proxy)
    pub proxies_linked: Vec<(PathBuf, PathBuf)>,
    pub failed: Vec<(PathBuf, String)>,
    /// Media whose measurements came from the global cache.
    pub cache_hits: usize,
}

/// Per-clip analysis status for UIs and the CLI.
#[derive(Clone, Debug)]
pub struct ClipStatus {
    pub clip: ClipId,
    pub media: MediaId,
    pub name: String,
    pub path: String,
    pub state: RunState,
    pub progress: f64,
    pub error: Option<String>,
    pub profile: ColourProfile,
    pub bin: Option<i64>,
    /// Camera make/model from metadata, and the recording date (ISO), for collections.
    pub camera: Option<String>,
    pub created: Option<String>,
    pub codec: String,
    /// The media file isn't at its recorded path (relink needed).
    pub missing: bool,
}

struct Probed {
    path: PathBuf,
    info: MediaInfo,
    fp: String,
    cfp: String,
}

pub struct Session {
    pub db: Arc<Mutex<Db>>,
    store: Arc<ProjectStore>,
    pub(crate) sched: Scheduler,
    events: Mutex<Receiver<Event>>,
    pub(crate) engine: Mutex<Engine>,
    pub(crate) opts: SessionOptions,
    pub(crate) cal: Calibration,
    loaded: Mutex<HashMap<i64, Arc<MeasurementSet>>>,
    /// Decoded audio sets by run id (None = the run has no audio).
    loaded_audio: Mutex<HashMap<i64, Option<Arc<clippro_audio::AudioSet>>>>,
    /// Stored AI results by media id (P12).
    pub(crate) ai_cache: Mutex<HashMap<i64, Arc<crate::ai::AiData>>>,
    /// Loaded AI models, kept between clips and searches.
    pub(crate) ai_engines: Mutex<crate::ai::AiEngines>,
}

impl Session {
    fn build(db: Db, opts: SessionOptions) -> Session {
        let cal = Calibration::embedded().clone();
        let cache = opts.cache_path.as_ref().and_then(|p| MeasureCache::open(p).ok()).map(|c| Arc::new(Mutex::new(c)));
        let db = Arc::new(Mutex::new(db));
        let store = Arc::new(ProjectStore {
            db: db.clone(),
            cache,
            cal: cal.clone(),
            keys: Mutex::new(HashMap::new()),
            errors: Mutex::new(Vec::new()),
        });
        let (tx, rx) = mpsc::channel();
        let sched = Scheduler::new(opts.scheduler.clone(), cal.clone(), store.clone(), Some(tx));
        Session {
            db,
            store,
            sched,
            events: Mutex::new(rx),
            engine: Mutex::new(Engine::new(cal.clone())),
            opts,
            cal,
            loaded: Mutex::new(HashMap::new()),
            loaded_audio: Mutex::new(HashMap::new()),
            ai_cache: Mutex::new(HashMap::new()),
            ai_engines: Mutex::new(crate::ai::AiEngines::default()),
        }
    }

    pub fn create(path: &Path, name: &str, opts: SessionOptions) -> Result<Session> {
        Ok(Self::build(Db::create(path, name)?, opts))
    }

    pub fn open(path: &Path, opts: SessionOptions) -> Result<(Session, OpenReport)> {
        let (db, rep) = Db::open(path)?;
        Ok((Self::build(db, opts), rep))
    }

    /// Stops analysis (checkpoints are kept) and closes the project cleanly.
    pub fn close(self) -> Result<()> {
        let Session { db, store, sched, .. } = self;
        drop(sched);
        drop(store);
        let db = Arc::try_unwrap(db).map_err(|_| SessionError::Other("project still in use".into()))?;
        db.into_inner().unwrap_or_else(|p| p.into_inner()).close()?;
        Ok(())
    }

    pub fn run_params(&self) -> RunParams {
        RunParams {
            engine_version: clippro_measure::ENGINE_VERSION,
            calibration_version: self.cal.version,
            rate: self.opts.analyze.rate,
            pair_sampling: self.opts.analyze.pair_sampling,
            groups: self.opts.analyze.groups,
        }
    }

    pub fn scheduler(&self) -> &Scheduler {
        &self.sched
    }

    /// Storage errors raised inside analysis callbacks since the last call.
    pub fn take_storage_errors(&self) -> Vec<String> {
        std::mem::take(&mut lock(&self.store.errors))
    }

    // ---------- Import ----------

    /// Imports files and folders. Proxies are linked to their originals rather than added;
    /// files already in the project are reported as duplicates.
    pub fn import(&self, inputs: &[PathBuf]) -> Result<ImportReport> {
        let mut rep = ImportReport::default();
        let mut probed = Vec::new();
        for path in discover(inputs) {
            let info = match probe(&path) {
                Ok(i) => i,
                Err(e) => {
                    rep.failed.push((path, e.to_string()));
                    continue;
                }
            };
            match (fingerprint::fingerprint(&path), fingerprint::content_fingerprint(&path)) {
                (Ok(fp), Ok(cfp)) => probed.push(Probed { path, info, fp, cfp }),
                (Err(e), _) | (_, Err(e)) => rep.failed.push((path, e.to_string())),
            }
        }
        let (proxies, originals): (Vec<Probed>, Vec<Probed>) = probed.into_iter().partition(|p| looks_like_proxy(&p.path));
        let params = self.run_params();
        let mut db = lock(&self.db);
        for p in originals {
            self.add_original(&mut db, p, &params, &mut rep)?;
        }
        // Match proxies against every original in the project.
        let candidates: Vec<(MediaRow, MediaInfo)> = db
            .media_all()?
            .into_iter()
            .filter(|m| !m.is_proxy)
            .filter_map(|m| serde_json::from_str::<MediaInfo>(&m.probe_json).ok().map(|i| (m, i)))
            .collect();
        for p in proxies {
            let hit = candidates.iter().find(|(m, i)| proxy_matches((Path::new(&m.path), i), (&p.path, &p.info)));
            match hit {
                Some((orig, _)) => {
                    if !db.media_by_fingerprint(&p.cfp)?.is_empty() {
                        continue;
                    }
                    let pj = probe_json(&p)?;
                    let pid = db.add_media(&new_media(&p, &pj))?;
                    db.set_proxy(orig.id, Some(pid))?;
                    rep.proxies_linked.push((PathBuf::from(&orig.path), p.path));
                }
                None => self.add_original(&mut db, p, &params, &mut rep)?,
            }
        }
        Ok(rep)
    }

    fn add_original(&self, db: &mut Db, p: Probed, params: &RunParams, rep: &mut ImportReport) -> Result<()> {
        if let Some(existing) = db.media_by_fingerprint(&p.cfp)?.first() {
            rep.duplicates.push((p.path, existing.id));
            return Ok(());
        }
        let pj = probe_json(&p)?;
        let id = db.add_media(&new_media(&p, &pj))?;
        // Log footage: guess the profile from camera metadata (the user can change it).
        if let Some(profile) = clippro_score::colour::guess_profile(&p.info.tags) {
            if let Ok(key) = serde_json::to_string(&profile) {
                db.set_profile(id, key.trim_matches('"'))?;
            }
        }
        let name = p.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let clip = db.add_clip(id, &name)?;
        if let Some(cache) = &self.store.cache {
            let hit = lock(cache).get(&p.cfp, &params.key());
            if let Ok(Some(blob)) = hit {
                if codec::decode(&blob).is_ok() {
                    let run = db.run_for(id, params)?;
                    // Audio first: a run with measurements counts as done.
                    if let Ok(Some(audio)) = lock(cache).get(&p.cfp, &crate::store::audio_key(&params.key())) {
                        db.save_audio(run.id, &audio, "{\"from_cache\":true}")?;
                    }
                    db.save_measurements(run.id, &blob, "{\"from_cache\":true}")?;
                    rep.cache_hits += 1;
                }
            }
        }
        rep.added.push((p.path, clip));
        Ok(())
    }

    /// Updates a missing media item's path (relink); the content must match.
    pub fn relink(&self, media: MediaId, new_path: &Path) -> Result<()> {
        let db = lock(&self.db);
        let row = db.media(media)?.ok_or_else(|| SessionError::Other("no such media".into()))?;
        let cfp = fingerprint::content_fingerprint(new_path).map_err(|e| SessionError::Other(e.to_string()))?;
        if cfp != row.content_fingerprint {
            return Err(SessionError::Other(format!("{} has different content", new_path.display())));
        }
        let fp = fingerprint::fingerprint(new_path).map_err(|e| SessionError::Other(e.to_string()))?;
        db.set_media_path(media, new_path, &fp)?;
        Ok(())
    }

    /// Relinks every missing media item found under `roots` (by name + content).
    pub fn relink_missing(&self, roots: &[PathBuf]) -> Result<Vec<(MediaId, PathBuf)>> {
        let missing: Vec<MediaRow> = lock(&self.db).media_all()?.into_iter().filter(|m| !Path::new(&m.path).exists()).collect();
        let mut done = Vec::new();
        for m in missing {
            let name = Path::new(&m.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if let Some(found) = crate::import::relink_search(&name, &m.content_fingerprint, roots) {
                self.relink(m.id, &found)?;
                done.push((m.id, found));
            }
        }
        Ok(done)
    }

    pub fn set_profile(&self, media: MediaId, profile: ColourProfile) -> Result<()> {
        let name = serde_json::to_string(&profile).map_err(|e| SessionError::Other(e.to_string()))?;
        lock(&self.db).set_profile(media, name.trim_matches('"'))?;
        Ok(())
    }

    // ---------- Analysis ----------

    /// Queues every media item without a finished run for the current analysis settings.
    /// Returns how many jobs were queued.
    /// File and probe used for preview playback of `media`: the linked proxy when there is one
    /// (it is lighter to decode), otherwise the original. `None` when the media is unknown.
    pub fn preview_source(&self, media: MediaId) -> Result<Option<(PathBuf, MediaInfo)>> {
        let db = lock(&self.db);
        let Some(m) = db.media(media)? else { return Ok(None) };
        let src = match m.proxy_media_id {
            Some(pid) => db.media(pid)?.filter(|p| Path::new(&p.path).exists()).unwrap_or_else(|| m.clone()),
            None => m,
        };
        let info = serde_json::from_str::<MediaInfo>(&src.probe_json).map_err(|e| SessionError::Other(e.to_string()))?;
        Ok(Some((PathBuf::from(src.path), info)))
    }

    pub fn queue_analysis(&self) -> Result<usize> {
        let params = self.run_params();
        let db = lock(&self.db);
        let mut n = 0;
        for m in db.media_all()? {
            if m.is_proxy || db.run_for(m.id, &params)?.state == RunState::Done {
                continue;
            }
            let mut opts = self.opts.analyze.clone();
            opts.audio_stream = m.audio_stream.map(|s| s as usize);
            if self.queue_media(&db, &m, &params, opts)? {
                n += 1;
            }
        }
        Ok(n)
    }

    /// Queues one media item's analysis run (whatever its state). False when it can't be analysed
    /// (no video, or a proxy is required); the run is then marked failed with the reason.
    fn queue_media(&self, db: &Db, m: &MediaRow, params: &RunParams, opts: AnalyzeOptions) -> Result<bool> {
        let run = db.run_for(m.id, params)?;
        // Analyse the proxy when one is linked (export still points at the original).
        let src = match m.proxy_media_id {
            Some(pid) => db.media(pid)?.unwrap_or_else(|| m.clone()),
            None => m.clone(),
        };
        let Ok(info) = serde_json::from_str::<MediaInfo>(&src.probe_json) else { return Ok(false) };
        if info.video.is_none() || info.proxy_required {
            db.set_run_state(run.id, RunState::Failed, Some(if info.proxy_required { "Proxy required" } else { "No video stream" }))?;
            return Ok(false);
        }
        lock(&self.store.keys).insert(run.id.0 as u64, CacheKey { content_fingerprint: m.content_fingerprint.clone(), run_key: params.key() });
        db.set_run_state(run.id, RunState::Queued, None)?;
        self.sched.add(JobSpec { id: run.id.0 as u64, path: PathBuf::from(&src.path), info, seed_base: seed_from_fingerprint(&m.content_fingerprint), opts });
        Ok(true)
    }

    pub(crate) fn clip_media(&self, db: &Db, clip: ClipId) -> Result<MediaRow> {
        let c = db.clips()?.into_iter().find(|c| c.id == clip).ok_or_else(|| SessionError::Other("Unknown clip".into()))?;
        db.media(c.media_id)?.ok_or_else(|| SessionError::Other("The clip's media is missing".into()))
    }

    /// The audio streams of a clip's media and the chosen one (`None` = automatic).
    pub fn audio_streams(&self, clip: ClipId) -> Result<(Vec<clippro_media::probe::AudioInfo>, Option<usize>)> {
        let db = lock(&self.db);
        let m = self.clip_media(&db, clip)?;
        let info: MediaInfo = serde_json::from_str(&m.probe_json).map_err(|e| SessionError::Other(e.to_string()))?;
        Ok((info.audio, m.audio_stream.map(|s| s as usize)))
    }

    /// Chooses the audio stream of a clip's media and re-analyses it (P10.2 stream picker). The
    /// index is the container's stream index from the probe (`AudioInfo::index`).
    pub fn set_audio_stream(&self, clip: ClipId, stream: Option<usize>) -> Result<()> {
        let params = self.run_params();
        let db = lock(&self.db);
        let m = self.clip_media(&db, clip)?;
        db.set_audio_stream(m.id, stream.map(|s| s as i64))?;
        let run = db.run_for(m.id, &params)?;
        lock(&self.loaded_audio).remove(&run.id.0);
        let m = db.media(m.id)?.ok_or_else(|| SessionError::Other("The clip's media is missing".into()))?;
        let mut opts = self.opts.analyze.clone();
        opts.audio_stream = stream;
        self.queue_media(&db, &m, &params, opts)?;
        Ok(())
    }

    /// Analyses a clip again (context menu "Retry"); `force_cpu` decodes without the GPU.
    pub fn retry(&self, clip: ClipId, force_cpu: bool) -> Result<bool> {
        let params = self.run_params();
        let db = lock(&self.db);
        let m = self.clip_media(&db, clip)?;
        let mut opts = self.opts.analyze.clone();
        opts.audio_stream = m.audio_stream.map(|s| s as usize);
        if force_cpu {
            opts.decode.hw = clippro_media::video::HwMode::Off;
        }
        self.queue_media(&db, &m, &params, opts)
    }

    /// Moves a queued clip to the front of the analysis queue ("Move to next").
    pub fn bump(&self, clip: ClipId) -> Result<()> {
        let params = self.run_params();
        let db = lock(&self.db);
        let m = self.clip_media(&db, clip)?;
        let run = db.run_for(m.id, &params)?;
        self.sched.bump(run.id.0 as u64);
        Ok(())
    }

    /// Links `proxy` to a clip's media by hand ("Link proxy…") and re-analyses from it.
    pub fn link_proxy(&self, clip: ClipId, proxy: &Path) -> Result<()> {
        let path = std::path::absolute(proxy).unwrap_or_else(|_| proxy.to_path_buf());
        let info = probe(&path).map_err(|e| SessionError::Other(e.to_string()))?;
        if info.video.is_none() {
            return Err(SessionError::Other("The proxy has no video stream.".into()));
        }
        let fp = fingerprint::fingerprint(&path).map_err(|e| SessionError::Other(e.to_string()))?;
        let cfp = fingerprint::content_fingerprint(&path).map_err(|e| SessionError::Other(e.to_string()))?;
        let params = self.run_params();
        let db = lock(&self.db);
        let m = self.clip_media(&db, clip)?;
        let tolerance = 1_000_000i64.max(m.duration_us / 100);
        if (info.duration_us - m.duration_us).abs() > tolerance {
            return Err(SessionError::Other("The proxy's duration doesn't match the clip.".into()));
        }
        let p = Probed { path, info, fp, cfp };
        let pj = probe_json(&p)?;
        let pid = db.add_media(&new_media(&p, &pj))?;
        db.set_proxy(m.id, Some(pid))?; // also marks the new row as a proxy
        let m = db.media(m.id)?.ok_or_else(|| SessionError::Other("The clip's media is missing".into()))?;
        self.queue_media(&db, &m, &params, self.opts.analyze.clone())?;
        Ok(())
    }

    /// Drains pending events, mirrors progress into the project, and returns them.
    pub fn poll_events(&self) -> Vec<Event> {
        let events: Vec<Event> = lock(&self.events).try_iter().collect();
        let db = lock(&self.db);
        for e in &events {
            if let Event::Progress { job, done, total } = e {
                let _ = db.set_run_progress(clippro_types::RunId(*job as i64), *done as f64 / (*total).max(1) as f64);
            }
        }
        events
    }

    /// Blocks until analysis is idle, calling `on_event` for each event.
    pub fn wait(&self, mut on_event: impl FnMut(&Event)) {
        loop {
            let first = lock(&self.events).recv_timeout(Duration::from_millis(200)).ok();
            if let Some(e) = first {
                on_event(&e);
                if let Event::Progress { job, done, total } = &e {
                    let _ = lock(&self.db).set_run_progress(clippro_types::RunId(*job as i64), *done as f64 / (*total).max(1) as f64);
                }
            }
            for e in self.poll_events() {
                on_event(&e);
            }
            if self.sched.is_idle() {
                for e in self.poll_events() {
                    on_event(&e);
                }
                return;
            }
        }
    }

    pub fn pause(&self) {
        self.sched.pause();
    }

    pub fn resume(&self) {
        self.sched.resume();
    }

    pub fn clip_statuses(&self) -> Result<Vec<ClipStatus>> {
        let params = self.run_params();
        let key = params.key();
        let db = lock(&self.db);
        let mut out = Vec::new();
        for c in db.clips()? {
            let Some(m) = db.media(c.media_id)? else { continue };
            let run = db.run_by_key(m.id, &key)?;
            let (mut state, progress, error) =
                run.as_ref().map(|r| (r.state, r.progress, r.error.clone())).unwrap_or((RunState::Queued, 0.0, None));
            if let Some(r) = &run {
                if let Some(s) = self.sched.status(r.id.0 as u64) {
                    if s == JobStatus::Running && state != RunState::Done {
                        state = RunState::Running;
                    }
                }
            }
            let profile: ColourProfile = serde_json::from_str(&format!("\"{}\"", m.profile)).unwrap_or_default();
            let info: Option<MediaInfo> = serde_json::from_str(&m.probe_json).ok();
            let camera = info.as_ref().and_then(|i| {
                let s = [i.camera_make.as_deref(), i.camera_model.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" ");
                (!s.trim().is_empty()).then_some(s)
            });
            let created = info.as_ref().and_then(|i| i.creation_time.clone());
            let codec = info.as_ref().and_then(|i| i.video.as_ref().map(|v| v.codec.clone())).unwrap_or_default();
            let missing = !Path::new(&m.path).exists();
            out.push(ClipStatus { clip: c.id, media: m.id, name: c.name, path: m.path, state, progress, error, profile, bin: c.bin_id, camera, created, codec, missing });
        }
        Ok(out)
    }

    // ---------- Scoring ----------

    /// Engine inputs for every clip with finished measurements (clip order).
    pub fn clip_inputs(&self) -> Result<Vec<ClipInput>> {
        self.clip_inputs_for(&[])
    }

    /// Engine inputs with the AI Content columns, flags and snap points; `shot_types` drives the
    /// Shot-type match sub-metric.
    pub fn clip_inputs_for(&self, shot_types: &[String]) -> Result<Vec<ClipInput>> {
        let mut out = self.base_inputs()?;
        let media: HashMap<ClipId, i64> = lock(&self.db).clips()?.into_iter().map(|c| (c.id, c.media_id.0)).collect();
        for c in &mut out {
            let Some(&m) = media.get(&c.id) else { continue };
            let (cols, flags, snaps) = self.ai_inputs(MediaId(m), &c.ms.time_us, shot_types);
            if cols.is_empty() && flags.is_none() && snaps.is_none() {
                continue;
            }
            let mut h = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(&self.ai_data(MediaId(m)).map(|d| d.version).unwrap_or(0), &mut h);
            std::hash::Hash::hash(shot_types, &mut h);
            c.version ^= std::hash::Hasher::finish(&h) & 0x00FF_FFFF_0000_0000;
            for (k, v) in cols {
                c.extra.insert(k, Arc::new(v));
            }
            if let Some(f) = flags {
                let merged = match &c.extra_flags {
                    Some(a) => a.iter().zip(&f).map(|(x, y)| x | y).collect(),
                    None => f,
                };
                c.extra_flags = Some(Arc::new(merged));
            }
            c.snaps = snaps.map(Arc::new);
        }
        Ok(out)
    }

    fn base_inputs(&self) -> Result<Vec<ClipInput>> {
        let key = self.run_params().key();
        let db = lock(&self.db);
        let mut out = Vec::new();
        for c in db.clips()? {
            let Some(m) = db.media(c.media_id)? else { continue };
            let Some(run) = db.run_by_key(m.id, &key)? else { continue };
            if run.state != RunState::Done {
                continue;
            }
            let ms = {
                let mut cache = lock(&self.loaded);
                match cache.get(&run.id.0) {
                    Some(ms) => ms.clone(),
                    None => {
                        let Some(blob) = db.measurements(run.id)? else { continue };
                        let ms = Arc::new(codec::decode(&blob).map_err(|e| SessionError::Other(e.to_string()))?);
                        cache.insert(run.id.0, ms.clone());
                        ms
                    }
                }
            };
            // Audio (P10): aligned to this run's samples as the Audio sub-metrics and flags.
            let audio = {
                let mut cache = lock(&self.loaded_audio);
                match cache.get(&run.id.0) {
                    Some(a) => a.clone(),
                    None => {
                        let a = db.audio(run.id)?.and_then(|b| clippro_audio::AudioSet::decode(&b)).map(Arc::new);
                        cache.insert(run.id.0, a.clone());
                        a
                    }
                }
            };
            let mut extra = BTreeMap::new();
            let mut extra_flags = None;
            if let Some(a) = audio {
                let al = a.align(&ms.time_us, ms.rate_hz);
                extra.insert(clippro_score::SubMetric::Loudness, Arc::new(al.loudness));
                extra.insert(clippro_score::SubMetric::Clean, Arc::new(al.clean));
                extra.insert(clippro_score::SubMetric::NoiseFloor, Arc::new(al.noise_floor));
                extra.insert(clippro_score::SubMetric::SpeechPresence, Arc::new(al.speech));
                extra.insert(clippro_score::SubMetric::Wind, Arc::new(al.wind));
                extra.insert(clippro_score::SubMetric::Hum, Arc::new(al.hum));
                extra_flags = Some(Arc::new(al.flags));
            }
            let profile: ColourProfile = serde_json::from_str(&format!("\"{}\"", m.profile)).unwrap_or_default();
            out.push(ClipInput {
                id: c.id,
                // The profile changes the Luma sub-metric, so it's part of the version.
                version: run.id.0 as u64 | (profile as u64) << 56,
                ms,
                duration_us: m.duration_us,
                profile,
                extra,
                extra_flags,
                snaps: None,
            });
        }
        Ok(out)
    }

    pub fn overrides(&self) -> Result<HashMap<ClipId, Vec<Override>>> {
        let mut map: HashMap<ClipId, Vec<Override>> = HashMap::new();
        for o in lock(&self.db).overrides()? {
            let kind = match o.kind.as_str() {
                "force_in" => OverrideKind::ForceIn,
                "force_out" => OverrideKind::ForceOut,
                "trim" => OverrideKind::Trim,
                "manual" | "manual_segment" => OverrideKind::ManualSegment,
                _ => continue, // annotations and future kinds don't change scoring
            };
            map.entry(o.clip_id).or_default().push(Override { kind, start_us: o.start_us, end_us: o.end_us });
        }
        Ok(map)
    }

    /// The decoded audio set of each clip that has one (waveform lane, meters).
    pub fn clip_audio(&self) -> Result<Vec<(ClipId, Arc<clippro_audio::AudioSet>)>> {
        let key = self.run_params().key();
        let db = lock(&self.db);
        let mut out = Vec::new();
        for c in db.clips()? {
            let Some(m) = db.media(c.media_id)? else { continue };
            let Some(run) = db.run_by_key(m.id, &key)? else { continue };
            let cached = lock(&self.loaded_audio).get(&run.id.0).cloned();
            let a = match cached {
                Some(a) => a,
                None => {
                    let a = db.audio(run.id)?.and_then(|b| clippro_audio::AudioSet::decode(&b)).map(Arc::new);
                    lock(&self.loaded_audio).insert(run.id.0, a.clone());
                    a
                }
            };
            if let Some(a) = a {
                out.push((c.id, a));
            }
        }
        Ok(out)
    }

    /// Scores every analysed clip with `settings`.
    pub fn score(&self, settings: &Settings) -> Result<Selects> {
        let clips = self.clip_inputs_for(&settings.shot_types)?;
        let overrides = self.overrides()?;
        let mut sel = {
            let mut engine = lock(&self.engine);
            engine.retain(&clips.iter().map(|c| c.id).collect::<Vec<_>>());
            engine
                .score(&clips, settings, &overrides, &AtomicBool::new(false))
                .ok_or_else(|| SessionError::Other("scoring was cancelled".into()))?
        };
        if settings.best_take_only {
            self.apply_best_take(&mut sel)?;
        }
        Ok(sel)
    }

    /// Builds an export timeline from the current selects (SPEC §16). Exports always point at
    /// the original media, even when a proxy was analysed.
    pub fn export_timeline(&self, settings: &Settings, sort: SortOrder, opts: &ExportOptions) -> Result<Timeline> {
        let (media, selects, _) = self.export_inputs(settings, sort)?;
        Ok(clippro_interchange::build(media, &selects, opts))
    }

    /// The selects in `sort` order as export inputs, with the media they point at and, per
    /// select, its clip and media metadata (for stringout grouping).
    pub(crate) fn export_inputs(&self, settings: &Settings, sort: SortOrder) -> Result<(Vec<MediaRef>, Vec<SelectInput>, Vec<(ClipId, MediaInfo)>)> {
        let sel = self.score(settings)?;
        let annotations = self.annotations()?;
        let refs = sel.sorted(sort);
        let db = lock(&self.db);
        let mut media: Vec<MediaRef> = Vec::new();
        let mut infos: Vec<MediaInfo> = Vec::new();
        let mut index: HashMap<ClipId, usize> = HashMap::new();
        let clips = db.clips()?;
        for r in &refs {
            if index.contains_key(&r.clip) {
                continue;
            }
            let Some(c) = clips.iter().find(|c| c.id == r.clip) else { continue };
            let Some(m) = db.media(c.media_id)? else { continue };
            let info: MediaInfo = serde_json::from_str(&m.probe_json).map_err(|e| SessionError::Other(e.to_string()))?;
            index.insert(r.clip, media.len());
            media.push(media_ref(&info, m.id.0, &c.name, PathBuf::from(&m.path)));
            infos.push(info);
        }
        drop(db);
        let mut selects = Vec::new();
        let mut meta = Vec::new();
        let people: HashMap<i64, String> = self.people().map(|v| v.into_iter().map(|p| (p.id, p.name)).collect()).unwrap_or_default();
        let clip_media: HashMap<ClipId, MediaId> = self.clip_statuses()?.into_iter().map(|c| (c.clip, c.media)).collect();
        for r in &refs {
            let Some(&mi) = index.get(&r.clip) else { continue };
            meta.push((r.clip, infos[mi].clone()));
            let flags = sel
                .clips
                .iter()
                .find(|c| c.clip == r.clip)
                .map(|c| {
                    let fl = &c.scores.flags;
                    let (a, b) = (r.segment.first_sample.min(fl.len()), (r.segment.last_sample + 1).min(fl.len()));
                    let bits = fl[a..b].iter().fold(0u32, |acc, f| acc | f);
                    // Only flags that affect scoring with these settings (neutral ones are noise here).
                    FlagKind::ALL
                        .iter()
                        .filter(|f| bits & f.bit() != 0 && settings.tier(**f) != clippro_types::Tier::Neutral)
                        .map(|f| f.label().to_string())
                        .collect()
                })
                .unwrap_or_default();
            let ann = crate::annotate::best_match(&annotations, r.clip, r.segment.start_us, r.segment.end_us);
            let ai = clip_media.get(&r.clip).and_then(|m| self.ai_data(*m).ok()).filter(|d| !d.is_empty());
            selects.push(SelectInput {
                media: mi,
                start_us: r.segment.start_us,
                end_us: r.segment.end_us,
                in_us: r.segment.in_us,
                out_us: r.segment.out_us,
                score: r.segment.mean,
                flags,
                rating: ann.map_or(0, |a| a.rating),
                label: ann.and_then(|a| a.label.as_deref().and_then(clippro_interchange::MarkerColor::parse)),
                note: ann.map(|a| a.note.clone()).unwrap_or_default(),
                transcript: ai.as_ref().and_then(|d| d.transcript.as_ref()).map(|t| t.words_in(r.segment.start_us, r.segment.end_us).map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")).unwrap_or_default(),
                people: ai.as_ref().map(|d| d.people_in(r.segment.start_us, r.segment.end_us).iter().filter_map(|p| people.get(p).cloned()).collect()).unwrap_or_default(),
                shot: ai.as_ref().and_then(|d| d.shot_in(r.segment.start_us, r.segment.end_us)).map(|s| s.0.label().to_string()).unwrap_or_default(),
            });
        }
        Ok((media, selects, meta))
    }

    /// Renders and writes an export file; records it in the project's export history.
    pub fn export_to(&self, settings: &Settings, sort: SortOrder, opts: &ExportOptions, format: Format, path: &Path) -> Result<usize> {
        let tl = self.export_timeline(settings, sort, opts)?;
        self.write_timeline(&tl, format, path, &serde_json::to_string(opts).unwrap_or_default())
    }

    /// Renders and writes any timeline, recording it in the export history; returns the number
    /// of clips on V1.
    pub fn write_timeline(&self, tl: &Timeline, format: Format, path: &Path, settings_json: &str) -> Result<usize> {
        let bytes = format.render_bytes(tl).map_err(SessionError::Other)?;
        std::fs::write(path, &bytes).map_err(|e| SessionError::Other(format!("{}: {e}", path.display())))?;
        let n = tl.video.first().map(|t| t.items.iter().filter(|i| matches!(i, clippro_interchange::model::Item::Clip(_))).count()).unwrap_or(0);
        lock(&self.db).record_export(&format!("{format:?}").to_lowercase(), &path.to_string_lossy(), settings_json)?;
        Ok(n)
    }

    pub fn clip_names(&self) -> Result<HashMap<ClipId, (String, String)>> {
        let db = lock(&self.db);
        let mut out = HashMap::new();
        for c in db.clips()? {
            if let Some(m) = db.media(c.media_id)? {
                out.insert(c.id, (c.name, m.path));
            }
        }
        Ok(out)
    }
}

fn probe_json(p: &Probed) -> Result<String> {
    serde_json::to_string(&p.info).map_err(|e| SessionError::Other(e.to_string()))
}

fn new_media<'a>(p: &'a Probed, probe_json: &'a str) -> NewMedia<'a> {
    let v = p.info.video.as_ref();
    let fps = p.info.fps().0;
    NewMedia {
        path: &p.path,
        fingerprint: &p.fp,
        content_fingerprint: &p.cfp,
        size: p.info.size,
        probe_json,
        duration_us: p.info.duration_us,
        width: v.map(|v| v.width).unwrap_or(0),
        height: v.map(|v| v.height).unwrap_or(0),
        fps: (fps.num, fps.den),
        reel: p.info.reel.as_deref(),
        camera: p.info.camera_model.as_deref(),
        start_tc: p.info.start_timecode.as_deref(),
    }
}

/// Interchange media reference from probed info.
pub fn media_ref(info: &MediaInfo, id: i64, name: &str, path: PathBuf) -> MediaRef {
    let rate = info.fps();
    let v = info.video.as_ref();
    let (start_tc_frames, drop_frame) = match &info.start_timecode {
        Some(tc) => match Timecode::parse(tc, rate.timecode_base()) {
            Ok(t) => (t.frames, t.drop),
            Err(_) => (0, false),
        },
        None => (0, false),
    };
    MediaRef {
        id,
        name: name.to_string(),
        path,
        rate,
        duration_frames: ((info.duration_us as f64 / 1e6) * rate.fps()).round() as i64,
        start_tc_frames,
        drop_frame,
        width: v.map(|v| v.width).unwrap_or(1920),
        height: v.map(|v| v.height).unwrap_or(1080),
        audio_channels: info.audio.first().map(|a| a.channels).unwrap_or(0),
        reel: info.reel.clone(),
    }
}
