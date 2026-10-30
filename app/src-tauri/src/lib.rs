//! ClipPro desktop app (Tauri 2). Commands are thin wrappers over `clippro_core::Session`;
//! TypeScript bindings are generated into `app/ui/src/bindings.ts` (tauri-specta).

use base64::Engine as _;
use clippro_core::{Session, SessionOptions};
use clippro_score::{builtins, Segment, SelectRef, Settings, SortOrder, Summary};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};
use tauri_specta::{collect_commands, collect_events, Builder, Event};

pub mod ai;
pub mod audio_out;
pub mod data;
pub mod diag;
pub mod presets;
pub mod preview;
pub mod settings;
#[cfg(windows)]
mod shared;

#[derive(Default)]
pub struct AppState {
    session: Mutex<Option<Arc<Session>>>,
    path: Mutex<Option<PathBuf>>,
    preview: preview::PreviewCache,
    settings: Mutex<settings::AppSettings>,
    settings_path: Mutex<Option<PathBuf>>,
    presets_dir: Mutex<Option<PathBuf>>,
    watch: Mutex<Option<clippro_core::watch::WatchRunner>>,
    app: Mutex<Option<tauri::AppHandle>>,
    audio: audio_out::AudioOut,
    /// `--bench-preview` report file (ADR-002); `None` in normal runs.
    bench_out: Mutex<Option<PathBuf>>,
    /// The imported timeline under review (P11.3) and the project clips it was scored against.
    review: Mutex<Option<(clippro_core::review::TimelineReview, Vec<clippro_core::review::ClipData>)>>,
    /// Cancel flag of the running offload (P13.1), if any.
    offload: Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>,
    /// Cancel flag of the running proxy or render job (P13.2/P13.3), if any.
    job: Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>,
    /// Cancel flag of the running AI queue (P12.7), if any.
    ai: Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>,
    /// Model downloads in progress: id → cancel flag.
    ai_downloads: Mutex<std::collections::HashMap<String, Arc<std::sync::atomic::AtomicBool>>>,
    /// Clips whose AI pass failed this run; the automatic queue skips them until "Run now".
    ai_failed: Mutex<std::collections::HashSet<i64>>,
}

/// Self-test runs. `--bench-preview <out.json> <clip>...` measures the preview transports;
/// `--pdf-check <out.pdf> <clip>...` analyses the clips and prints the contact sheet to a PDF.
pub struct Bench {
    pub out: PathBuf,
    pub clips: Vec<PathBuf>,
    pub pdf: bool,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct BenchClip {
    pub media: f64,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub duration_us: f64,
    pub fps: f64,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl AppState {
    fn session(&self) -> Result<Arc<Session>, String> {
        lock(&self.session).clone().ok_or_else(|| "No project is open.".to_string())
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> { tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())? }

// ---------- DTOs ----------

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct ProjectInfo {
    pub path: String,
    pub name: String,
    /// The previous session didn't close cleanly; the last saved state was restored.
    pub recovered: bool,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct ImportResult {
    pub added: u32,
    pub duplicates: u32,
    pub proxies_linked: u32,
    pub cache_hits: u32,
    pub failed: Vec<ImportFailure>,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct ImportFailure {
    pub path: String,
    pub error: String,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct ClipRow {
    pub clip: f64,
    pub media: f64,
    pub name: String,
    pub path: String,
    /// queued | running | paused | done | failed
    pub state: String,
    pub progress: f64,
    pub error: Option<String>,
    /// Log/colour profile used for preview and log-aware exposure.
    pub profile: clippro_types::ColourProfile,
    pub bin: Option<f64>,
    pub camera: Option<String>,
    /// Recording date (ISO 8601) from metadata.
    pub created: Option<String>,
    pub codec: String,
    pub missing: bool,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct PresetInfo {
    pub name: String,
    pub description: String,
    /// Shipped with ClipPro (can't be renamed or deleted).
    pub builtin: bool,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct ClipScore {
    pub clip: f64,
    pub duration_us: f64,
    pub rate_hz: f64,
    /// Final score per sample, base64 u8 (0–254 = 0–1, 255 = no data).
    pub bar: String,
    /// Flag bitset per sample, base64 little-endian u32.
    pub flags: String,
    pub segments: Vec<Segment>,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct ScoreResult {
    pub clips: Vec<ClipScore>,
    pub selects: Vec<SelectRef>,
    pub summary: Summary,
    /// Take groups of two or more selects (P12.6), when local AI is on.
    pub takes: Vec<TakeRow>,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct TakeRow {
    pub clip: f64,
    pub start_us: f64,
    pub group: u32,
    /// 1 = best take.
    pub rank: u32,
    pub size: u32,
}

/// Media a watch folder added (P9.5); the UI refreshes its clip list.
#[derive(Serialize, Deserialize, Type, Clone, Debug, Event)]
pub struct MediaAdded {
    pub added: u32,
    pub names: Vec<String>,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug, Event)]
pub struct AnalysisEvent {
    /// progress | finished | failed | paused | resumed
    pub kind: String,
    pub job: Option<f64>,
    pub done: Option<u32>,
    pub total: Option<u32>,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct OffloadSummary {
    pub files: u32,
    pub bytes: f64,
    pub copied: u32,
    pub skipped: u32,
    /// "Clips/A001.mov → D:/Shuttle: checksum mismatch …"
    pub failed: Vec<String>,
    pub manifests: Vec<String>,
    pub seconds: f64,
    /// Clips added to the project from the first destination.
    pub imported: u32,
}

/// Offload progress (P13.1). Progress is sent at most every 100 ms.
#[derive(Serialize, Deserialize, Type, Clone, Debug, Event)]
pub struct OffloadProgress {
    /// started | progress | finished | failed | cancelled
    pub kind: String,
    pub done: f64,
    pub total: f64,
    pub files: u32,
    pub index: u32,
    pub file: Option<String>,
    pub message: Option<String>,
    pub summary: Option<OffloadSummary>,
}

/// Proxy and render jobs (P13.2/P13.3). Progress is sent at most every 150 ms.
#[derive(Serialize, Deserialize, Type, Clone, Debug, Event)]
pub struct JobProgress {
    /// proxy | render
    pub job: String,
    /// progress | finished | failed | cancelled
    pub kind: String,
    pub index: u32,
    pub count: u32,
    pub fraction: f32,
    pub name: Option<String>,
    pub message: Option<String>,
    pub outputs: Vec<String>,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct CodecChoice {
    pub value: String,
    pub label: String,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct RenderRequest {
    pub out_dir: String,
    pub template: String,
    pub stream_copy: bool,
    pub allow_nearest_keyframe: bool,
    /// prores422 | prores_hq | dnxhr_hq | h264
    pub codec: String,
    pub handles: bool,
}

// ---------- Commands ----------

/// Session options from the app settings (analysis and performance defaults).
fn options(state: &AppState) -> SessionOptions {
    lock(&state.settings).session_options()
}

/// Remembers the open project for "reopen at start-up".
fn remember_project(state: &AppState, path: &std::path::Path) {
    let mut st = lock(&state.settings);
    let p = path.to_string_lossy().into_owned();
    st.last_project = Some(p.clone());
    st.recent_projects.retain(|x| !x.eq_ignore_ascii_case(&p));
    st.recent_projects.insert(0, p);
    st.recent_projects.truncate(10);
    if let Some(p) = lock(&state.settings_path).as_ref() {
        let _ = st.save(p);
    }
}

#[tauri::command]
#[specta::specta]
async fn project_new(state: State<'_, AppState>, path: String, name: String) -> Result<ProjectInfo, String> {
    let p = PathBuf::from(&path);
    let o = options(&state);
    let s = blocking(move || Session::create(&p, &name, o).map_err(|e| e.to_string())).await?;
    let name = lock(&s.db).name().map_err(|e| e.to_string())?;
    replace_session(&state, Arc::new(s), PathBuf::from(&path));
    Ok(ProjectInfo { path, name, recovered: false })
}

#[tauri::command]
#[specta::specta]
async fn project_open(state: State<'_, AppState>, path: String) -> Result<ProjectInfo, String> {
    let p = PathBuf::from(&path);
    let o = options(&state);
    let (s, rep) = blocking(move || Session::open(&p, o).map_err(|e| e.to_string())).await?;
    let name = lock(&s.db).name().map_err(|e| e.to_string())?;
    replace_session(&state, Arc::new(s), PathBuf::from(&path));
    Ok(ProjectInfo { path, name, recovered: rep.recovered })
}

/// (Re)starts the watch-folder runner for the open project.
fn restart_watch(state: &AppState) {
    lock(&state.watch).take();
    let (Ok(s), Some(app)) = (state.session(), lock(&state.app).clone()) else { return };
    let folders = s.watch_folders().unwrap_or_default();
    if folders.is_empty() {
        return;
    }
    let skip = s.media_paths().unwrap_or_default();
    let runner = clippro_core::watch::WatchRunner::start(folders, std::time::Duration::from_secs(5), skip, move |paths| {
        let Ok(s) = app.state::<AppState>().session() else { return };
        match s.import(&paths) {
            Ok(rep) if !rep.added.is_empty() => {
                let _ = s.queue_analysis();
                let names = rep.added.iter().filter_map(|(p, _)| p.file_stem().map(|n| n.to_string_lossy().into_owned())).collect();
                let _ = MediaAdded { added: rep.added.len() as u32, names }.emit(&app);
            }
            _ => {}
        }
    });
    *lock(&state.watch) = Some(runner);
}

fn replace_session(state: &AppState, s: Arc<Session>, path: PathBuf) {
    diag::log("INFO", &format!("project opened: {}", path.display()));
    *lock(&state.review) = None;
    remember_project(state, &path);
    state.preview.clear();
    state.audio.stop();
    lock(&state.watch).take();
    let old = lock(&state.session).replace(s);
    *lock(&state.path) = Some(path);
    if let Some(old) = old {
        close_session(old);
    }
    restart_watch(state);
}

fn close_session(s: Arc<Session>) {
    // Wait briefly for the event pump to drop its clone, then close cleanly.
    let mut s = s;
    for _ in 0..50 {
        match Arc::try_unwrap(s) {
            Ok(owned) => {
                let _ = owned.close();
                return;
            }
            Err(back) => {
                s = back;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
    }
}

#[tauri::command]
#[specta::specta]
async fn project_close(state: State<'_, AppState>) -> Result<(), String> {
    lock(&state.watch).take();
    let s = lock(&state.session).take();
    *lock(&state.path) = None;
    *lock(&state.review) = None;
    state.preview.clear();
    state.audio.stop();
    if let Some(s) = s {
        blocking(move || {
            close_session(s);
            Ok(())
        })
        .await?;
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
async fn project_info(state: State<'_, AppState>) -> Result<Option<ProjectInfo>, String> {
    let Some(s) = lock(&state.session).clone() else { return Ok(None) };
    let path = lock(&state.path).clone().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let name = lock(&s.db).name().map_err(|e| e.to_string())?;
    Ok(Some(ProjectInfo { path, name, recovered: false }))
}

#[tauri::command]
#[specta::specta]
async fn media_import(state: State<'_, AppState>, paths: Vec<String>) -> Result<ImportResult, String> {
    let s = state.session()?;
    blocking(move || {
        let inputs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        let r = s.import(&inputs).map_err(|e| e.to_string())?;
        Ok(ImportResult {
            added: r.added.len() as u32,
            duplicates: r.duplicates.len() as u32,
            proxies_linked: r.proxies_linked.len() as u32,
            cache_hits: r.cache_hits as u32,
            failed: r.failed.into_iter().map(|(p, e)| ImportFailure { path: p.to_string_lossy().into_owned(), error: e }).collect(),
        })
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn analysis_start(state: State<'_, AppState>) -> Result<u32, String> {
    let s = state.session()?;
    blocking(move || s.queue_analysis().map(|n| n as u32).map_err(|e| e.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn analysis_pause(state: State<'_, AppState>) -> Result<(), String> {
    state.session()?.pause();
    Ok(())
}

#[tauri::command]
#[specta::specta]
async fn analysis_resume(state: State<'_, AppState>) -> Result<(), String> {
    state.session()?.resume();
    Ok(())
}

#[tauri::command]
#[specta::specta]
async fn clips_list(state: State<'_, AppState>) -> Result<Vec<ClipRow>, String> {
    let s = state.session()?;
    blocking(move || {
        Ok(s.clip_statuses()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|c| ClipRow {
                clip: c.clip.0 as f64,
                media: c.media.0 as f64,
                name: c.name,
                path: c.path,
                state: format!("{:?}", c.state).to_lowercase(),
                progress: c.progress,
                error: c.error,
                profile: c.profile,
                bin: c.bin.map(|b| b as f64),
                camera: c.camera,
                created: c.created,
                codec: c.codec,
                missing: c.missing,
            })
            .collect())
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn presets_list(state: State<'_, AppState>) -> Result<Vec<PresetInfo>, String> {
    let mut out: Vec<PresetInfo> = builtins().into_iter().map(|p| PresetInfo { name: p.preset.name, description: p.preset.description, builtin: true }).collect();
    if let Some(up) = user_presets(&state) {
        out.extend(up.list().into_iter().map(|p| PresetInfo { name: p.preset.name, description: p.preset.description, builtin: false }));
    }
    Ok(out)
}

fn user_presets(state: &AppState) -> Option<presets::UserPresets> {
    lock(&state.presets_dir).clone().map(|dir| presets::UserPresets { dir })
}

fn find_preset(state: &AppState, name: &str) -> Result<clippro_score::Preset, String> {
    clippro_score::builtin(name).or_else(|| user_presets(state).and_then(|u| u.get(name))).ok_or_else(|| format!("Unknown preset “{name}”."))
}

#[tauri::command]
#[specta::specta]
async fn preset_settings(state: State<'_, AppState>, name: String) -> Result<Settings, String> {
    Settings::from_preset(&find_preset(&state, &name)?).map_err(|e| e.to_string())
}

/// Saves the current settings as a custom preset (`overwrite` replaces one with that name).
#[tauri::command]
#[specta::specta]
async fn preset_save(state: State<'_, AppState>, name: String, description: String, settings: Settings, overwrite: bool) -> Result<(), String> {
    let up = user_presets(&state).ok_or("No preset folder")?;
    up.save(&clippro_score::Preset::from_settings(&settings, name.trim(), description.trim()), overwrite)
}

#[tauri::command]
#[specta::specta]
async fn preset_delete(state: State<'_, AppState>, name: String) -> Result<(), String> {
    user_presets(&state).ok_or("No preset folder")?.delete(&name)
}

#[tauri::command]
#[specta::specta]
async fn preset_rename(state: State<'_, AppState>, old: String, name: String) -> Result<(), String> {
    user_presets(&state).ok_or("No preset folder")?.rename(&old, &name)
}

/// Imports a `.toml` preset file; returns its (possibly de-duplicated) name.
#[tauri::command]
#[specta::specta]
async fn preset_import(state: State<'_, AppState>, path: String) -> Result<String, String> {
    Ok(user_presets(&state).ok_or("No preset folder")?.import(std::path::Path::new(&path))?.preset.name)
}

/// Writes `settings` as a preset TOML file (export of the current tuning or any preset).
#[tauri::command]
#[specta::specta]
async fn preset_export(name: String, description: String, settings: Settings, path: String) -> Result<(), String> {
    let text = clippro_score::Preset::from_settings(&settings, name.trim(), description.trim()).to_toml().map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
async fn score(state: State<'_, AppState>, settings: Settings, sort: SortOrder) -> Result<ScoreResult, String> {
    let s = state.session()?;
    let ai = lock(&state.settings).ai_consent;
    blocking(move || {
        let sel = s.score(&settings).map_err(|e| e.to_string())?;
        let inputs = s.clip_inputs().map_err(|e| e.to_string())?;
        let b64 = base64::engine::general_purpose::STANDARD;
        let clips = sel
            .clips
            .iter()
            .map(|c| {
                let input = inputs.iter().find(|i| i.id == c.clip);
                let bar: Vec<u8> =
                    c.scores.score.iter().map(|v| if v.is_nan() { 255 } else { (v.clamp(0.0, 1.0) * 254.0).round() as u8 }).collect();
                let flags: Vec<u8> = c.scores.flags.iter().flat_map(|f| f.to_le_bytes()).collect();
                ClipScore {
                    clip: c.clip.0 as f64,
                    duration_us: input.map(|i| i.duration_us as f64).unwrap_or(0.0),
                    rate_hz: input.map(|i| i.ms.rate_hz).unwrap_or(4.0),
                    bar: b64.encode(bar),
                    flags: b64.encode(flags),
                    segments: c.segments.clone(),
                }
            })
            .collect();
        let takes = if ai { s.take_ranking(&sel).unwrap_or_default() } else { Vec::new() };
        let takes = takes.into_iter().map(|t| TakeRow { clip: t.0 .0 as f64, start_us: t.1 as f64, group: t.2 as u32, rank: t.3 as u32, size: t.4 as u32 }).collect();
        Ok(ScoreResult { clips, selects: sel.sorted(sort), summary: sel.summary.clone(), takes })
    })
    .await
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct ExportRequest {
    /// xmeml | fcpxml | otio | edl | ale | avid-markers | csv | json | html | pdf
    pub format: String,
    pub path: String,
    pub name: String,
    /// selects | with_rejects | stringout
    pub layout: String,
    /// Stringouts: clip | camera | date | reel | rating | label.
    pub group_by: Option<String>,
    /// Stringouts: key | best_first.
    pub group_order: Option<String>,
    /// Stringouts: none | gap | slug, and its length in seconds.
    pub separator: Option<String>,
    pub separator_s: Option<f64>,
    pub handles: bool,
    pub gap_s: f64,
    pub with_audio: bool,
    pub markers: bool,
    pub marker_name: String,
    pub marker_comment: String,
    /// Shown in report headers (the UI's local time); reports only.
    pub generated: Option<String>,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct ExportResult {
    pub path: String,
    pub selects: u32,
}

#[tauri::command]
#[specta::specta]
async fn export_selects(app: tauri::AppHandle, state: State<'_, AppState>, settings: Settings, sort: SortOrder, request: ExportRequest) -> Result<ExportResult, String> {
    let s = state.session()?;
    if request.format == "html" || request.format == "pdf" {
        return export_report(app, s, settings, sort, request).await;
    }
    blocking(move || {
        let format = clippro_interchange::Format::parse(&request.format).ok_or_else(|| format!("Unknown export format “{}”.", request.format))?;
        let opts = clippro_interchange::ExportOptions {
            name: request.name.clone(),
            layout: if request.layout == "with_rejects" { clippro_interchange::Layout::SelectsOverRejects } else { clippro_interchange::Layout::SelectsOnly },
            handles: request.handles,
            gap_s: request.gap_s.max(0.0),
            with_audio: request.with_audio,
            markers: request.markers,
            marker_name: request.marker_name.clone(),
            marker_comment: request.marker_comment.clone(),
            rate: None,
        };
        let path = PathBuf::from(&request.path);
        if request.layout == "stringout" {
            use clippro_core::stringout::{GroupBy, GroupOrder};
            let by = serde_json::from_value::<GroupBy>(serde_json::Value::String(request.group_by.clone().unwrap_or_else(|| "clip".into()))).map_err(|e| e.to_string())?;
            let order = if request.group_order.as_deref() == Some("best_first") { GroupOrder::BestFirst } else { GroupOrder::Key };
            let secs = request.separator_s.unwrap_or(2.0).clamp(0.0, 30.0);
            let sep = match request.separator.as_deref() {
                Some("gap") => clippro_interchange::Separator::Gap { seconds: secs },
                Some("slug") => clippro_interchange::Separator::Slug { seconds: secs },
                _ => clippro_interchange::Separator::None,
            };
            let tl = s.stringout_timeline(&settings, sort, &opts, by, order, sep).map_err(|e| e.to_string())?;
            let json = serde_json::json!({ "options": opts, "group_by": by, "group_order": order, "separator": sep }).to_string();
            let n = s.write_timeline(&tl, format, &path, &json).map_err(|e| e.to_string())?;
            return Ok(ExportResult { path: request.path, selects: n as u32 });
        }
        let n = s.export_to(&settings, sort, &opts, format, &path).map_err(|e| e.to_string())?;
        Ok(ExportResult { path: request.path, selects: n as u32 })
    })
    .await
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct ReviewAlt {
    pub clip: f64,
    pub clip_name: String,
    pub start_us: f64,
    pub end_us: f64,
    pub mean: f32,
    /// "same_clip" | "take_group"
    pub kind: String,
    pub similarity: f32,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct ReviewItem {
    pub track: u32,
    pub name: String,
    /// Timeline frames from the start.
    pub record_in: f64,
    pub record_out: f64,
    pub clip: Option<f64>,
    pub src_in_us: f64,
    pub src_out_us: f64,
    pub mean: Option<f32>,
    pub bars: Vec<f32>,
    pub flags: Vec<String>,
    pub excluded: bool,
    pub weak: bool,
    pub alternatives: Vec<ReviewAlt>,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct ReviewResult {
    pub name: String,
    /// Detected format ("xmeml", "fcpxml", "otio", "edl").
    pub format: String,
    pub fps: f64,
    pub start_tc_frames: f64,
    pub drop_frame: bool,
    pub duration: f64,
    pub tracks: u32,
    pub items: Vec<ReviewItem>,
    pub linked: u32,
    /// Timeline media nothing in the project matched.
    pub missing: Vec<String>,
}

#[derive(Serialize, Deserialize, Type, Clone, Copy, Debug)]
pub struct ReviewSwap {
    pub item: u32,
    pub alternative: u32,
}

fn review_result(rv: &clippro_core::review::TimelineReview, format: clippro_interchange::Format) -> ReviewResult {
    let tl = &rv.timeline;
    ReviewResult {
        name: tl.name.clone(),
        format: serde_json::to_value(format).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(),
        fps: tl.rate.fps(),
        start_tc_frames: tl.start_tc_frames as f64,
        drop_frame: tl.drop_frame,
        duration: tl.duration() as f64,
        tracks: tl.video.len() as u32,
        items: rv
            .items
            .iter()
            .map(|i| ReviewItem {
                track: i.track as u32,
                name: i.name.clone(),
                record_in: i.record_in as f64,
                record_out: i.record_out as f64,
                clip: i.clip.map(|c| c.0 as f64),
                src_in_us: i.src_in_us as f64,
                src_out_us: i.src_out_us as f64,
                mean: i.mean,
                bars: i.bars.clone(),
                flags: i.flags.iter().map(|f| f.label().to_string()).collect(),
                excluded: i.excluded,
                weak: i.weak,
                alternatives: i
                    .alternatives
                    .iter()
                    .map(|a| ReviewAlt {
                        clip: a.clip.0 as f64,
                        clip_name: a.clip_name.clone(),
                        start_us: a.segment.start_us as f64,
                        end_us: a.segment.end_us as f64,
                        mean: a.segment.mean,
                        kind: if a.kind == clippro_core::review::AltKind::SameClip { "same_clip".into() } else { "take_group".into() },
                        similarity: a.similarity,
                    })
                    .collect(),
            })
            .collect(),
        linked: rv.relink.linked.len() as u32,
        missing: rv.relink.missing.iter().map(|&i| tl.media.get(i).map(|m| if m.name.is_empty() { m.path.display().to_string() } else { m.name.clone() }).unwrap_or_default()).collect(),
    }
}

/// Imports an NLE timeline, relinks it to the project and reviews every item (SPEC EXP-10).
#[tauri::command]
#[specta::specta]
async fn timeline_review(state: State<'_, AppState>, settings: Settings, path: String, weak_below: f64) -> Result<ReviewResult, String> {
    let s = state.session()?;
    let (rv, clips, format) = blocking(move || {
        let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
        let format = clippro_interchange::import::detect_bytes(&bytes).ok_or("Not a timeline ClipPro can read (AAF, FCP7 XML, FCPXML, OTIO or EDL).")?;
        let clips = s.review_data(&settings).map_err(|e| e.to_string())?;
        // EDLs carry no frame rate: assume the project's most common one.
        let mut rates: Vec<(clippro_types::FrameRate, usize)> = Vec::new();
        for c in &clips {
            match rates.iter_mut().find(|r| r.0 == c.media.rate) {
                Some(r) => r.1 += 1,
                None => rates.push((c.media.rate, 1)),
            }
        }
        let rate = rates.iter().max_by_key(|r| r.1).map(|r| r.0).unwrap_or_default();
        let tl = clippro_interchange::import::import_bytes(&bytes, rate)?;
        let opts = clippro_core::review::ReviewOptions { weak_below: weak_below.clamp(0.0, 1.0) as f32, ..Default::default() };
        let rv = clippro_core::review::review(tl, &clips, |f| settings.tier(f), &opts);
        Ok((rv, clips, format))
    })
    .await?;
    let out = review_result(&rv, format);
    *lock(&state.review) = Some((rv, clips));
    Ok(out)
}

/// Writes the reviewed timeline back out (P11.4): same tracks, accepted swaps, score markers.
#[tauri::command]
#[specta::specta]
async fn timeline_export_back(state: State<'_, AppState>, swaps: Vec<ReviewSwap>, markers: bool, format: String, path: String) -> Result<ExportResult, String> {
    let Some((rv, clips)) = lock(&state.review).clone() else { return Err("No timeline is being reviewed.".into()) };
    blocking(move || {
        let format = clippro_interchange::Format::parse(&format).ok_or_else(|| format!("Unknown export format “{format}”."))?;
        let swaps: Vec<clippro_core::review::Swap> = swaps.iter().map(|s| clippro_core::review::Swap { item: s.item as usize, alternative: s.alternative as usize }).collect();
        let tl = clippro_core::review::export_back(&rv, &clips, &swaps, markers);
        std::fs::write(&path, format.render_bytes(&tl)?).map_err(|e| format!("{path}: {e}"))?;
        let n = tl.video.iter().flat_map(|t| &t.items).filter(|i| matches!(i, clippro_interchange::model::Item::Clip(_))).count();
        Ok(ExportResult { path, selects: n as u32 })
    })
    .await
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct SyncMemberDto {
    pub clip: f64,
    pub name: String,
    pub angle: String,
    pub offset_us: Option<f64>,
    pub psr: Option<f32>,
    pub tc_offset_us: Option<f64>,
    pub disagreement_us: Option<f64>,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct SyncGroupDto {
    pub id: Option<f64>,
    pub name: String,
    /// "audio" | "timecode"
    pub method: String,
    pub reference: f64,
    pub members: Vec<SyncMemberDto>,
    pub warnings: Vec<String>,
}

fn sync_dto(g: &clippro_core::multicam::SyncGroup) -> SyncGroupDto {
    use clippro_core::multicam::SyncMethod;
    SyncGroupDto {
        id: g.id.map(|i| i as f64),
        name: g.name.clone(),
        method: if g.method == SyncMethod::Audio { "audio".into() } else { "timecode".into() },
        reference: g.reference.0 as f64,
        members: g
            .members
            .iter()
            .map(|m| SyncMemberDto {
                clip: m.clip.0 as f64,
                name: m.name.clone(),
                angle: m.angle.clone(),
                offset_us: m.offset_us.map(|v| v as f64),
                psr: m.psr,
                tc_offset_us: m.tc_offset_us.map(|v| v as f64),
                disagreement_us: m.disagreement_us.map(|v| v as f64),
            })
            .collect(),
        warnings: g.warnings.clone(),
    }
}

fn sync_from_dto(d: &SyncGroupDto) -> clippro_core::multicam::SyncGroup {
    use clippro_core::multicam::{SyncGroup, SyncMember, SyncMethod};
    SyncGroup {
        id: d.id.map(|i| i as i64),
        name: d.name.clone(),
        method: if d.method == "timecode" { SyncMethod::Timecode } else { SyncMethod::Audio },
        reference: clippro_types::ClipId(d.reference as i64),
        members: d
            .members
            .iter()
            .map(|m| SyncMember {
                clip: clippro_types::ClipId(m.clip as i64),
                name: m.name.clone(),
                angle: m.angle.clone(),
                offset_us: m.offset_us.map(|v| v as i64),
                psr: m.psr,
                tc_offset_us: m.tc_offset_us.map(|v| v as i64),
                disagreement_us: m.disagreement_us.map(|v| v as i64),
            })
            .collect(),
        warnings: d.warnings.clone(),
    }
}

/// Syncs clips by audio or start timecode (P11.5). Not saved until `sync_group_save`.
#[tauri::command]
#[specta::specta]
async fn sync_clips(state: State<'_, AppState>, name: String, clips: Vec<f64>, method: String) -> Result<SyncGroupDto, String> {
    let s = state.session()?;
    blocking(move || {
        let ids: Vec<clippro_types::ClipId> = clips.iter().map(|c| clippro_types::ClipId(*c as i64)).collect();
        let m = if method == "timecode" { clippro_core::multicam::SyncMethod::Timecode } else { clippro_core::multicam::SyncMethod::Audio };
        let g = s.sync_clips(&name, &ids, m, &Default::default()).map_err(|e| e.to_string())?;
        Ok(sync_dto(&g))
    })
    .await
}

#[tauri::command]
#[specta::specta]
fn sync_groups_list(state: State<'_, AppState>) -> Result<Vec<SyncGroupDto>, String> {
    let s = state.session()?;
    Ok(s.sync_groups().map_err(|e| e.to_string())?.iter().map(sync_dto).collect())
}

#[tauri::command]
#[specta::specta]
fn sync_group_save(state: State<'_, AppState>, group: SyncGroupDto) -> Result<f64, String> {
    let s = state.session()?;
    Ok(s.save_sync_group(&sync_from_dto(&group)).map_err(|e| e.to_string())? as f64)
}

#[tauri::command]
#[specta::specta]
fn sync_group_delete(state: State<'_, AppState>, id: f64) -> Result<(), String> {
    state.session()?.delete_sync_group(id as i64).map_err(|e| e.to_string())
}

/// Writes a sync group as a multicam: an FCPXML mc-clip, or stacked tracks in XML / OTIO.
#[tauri::command]
#[specta::specta]
async fn multicam_export(state: State<'_, AppState>, group: SyncGroupDto, format: String, path: String) -> Result<ExportResult, String> {
    let s = state.session()?;
    blocking(move || {
        let g = sync_from_dto(&group);
        let tl = s.multicam_timeline(&g).map_err(|e| e.to_string())?;
        let text = match format.as_str() {
            "fcpxml" => clippro_interchange::fcpxml::export_multicam(&tl),
            f => clippro_interchange::Format::parse(f).filter(|f| matches!(f, clippro_interchange::Format::Xmeml | clippro_interchange::Format::Otio)).ok_or_else(|| format!("Multicam can’t be written as “{f}”."))?.render(&tl),
        };
        std::fs::write(&path, text).map_err(|e| format!("{path}: {e}"))?;
        Ok(ExportResult { path, selects: tl.video.len() as u32 })
    })
    .await
}

/// Starts an offload in the background (P13.1); progress arrives as `OffloadProgress` events.
/// With `add_to_project`, the first destination's copy is imported and analysed when done.
#[tauri::command]
#[specta::specta]
fn offload_start(app: tauri::AppHandle, state: State<'_, AppState>, source: String, destinations: Vec<String>, hash: String, add_to_project: bool) -> Result<(), String> {
    use clippro_ingest::offload::{run, OffloadError, OffloadEvent, OffloadJob};
    let mut slot = lock(&state.offload);
    if slot.is_some() {
        return Err("An offload is already running.".into());
    }
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    *slot = Some(cancel.clone());
    drop(slot);
    let mut job = OffloadJob::new(PathBuf::from(&source), destinations.iter().map(PathBuf::from).collect());
    job.hash = clippro_ingest::hash::HashKind::parse(&hash).unwrap_or_default();
    let session = if add_to_project { state.session().ok() } else { None };
    std::thread::spawn(move || {
        let blank = |kind: &str| OffloadProgress { kind: kind.into(), done: 0.0, total: 0.0, files: 0, index: 0, file: None, message: None, summary: None };
        let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let (mut files, mut index, mut file) = (0u32, 0u32, String::new());
        let result = run(&job, &cancel, |e| match e {
            OffloadEvent::Started { files: n, bytes } => {
                files = *n as u32;
                let _ = OffloadProgress { files, total: *bytes as f64, ..blank("started") }.emit(&app);
            }
            OffloadEvent::File { index: i, rel, .. } => {
                index = *i as u32;
                file = rel.to_string();
            }
            OffloadEvent::Progress { done, total }
                if (last.elapsed().as_millis() >= 100 || done == total) => {
                    last = std::time::Instant::now();
                    let _ = OffloadProgress { done: *done as f64, total: *total as f64, files, index, file: Some(file.clone()), ..blank("progress") }.emit(&app);
                }
            _ => {}
        });
        let app_state = app.state::<AppState>();
        *lock(&app_state.offload) = None;
        match result {
            Ok(rep) => {
                diag::log(if rep.ok() { "INFO" } else { "WARN" }, &format!("offload: {} files, {} copied, {} skipped, {} failed", rep.files, rep.copied, rep.skipped, rep.failed.len()));
                let mut imported = 0;
                if let (Some(s), Some(d)) = (session, job.destinations.first()) {
                    if let Ok(r) = s.import(&[job.dest_root(d)]) {
                        imported = r.added.len() as u32;
                        if imported > 0 {
                            let _ = s.queue_analysis();
                            let names = r.added.iter().filter_map(|(p, _)| p.file_stem().map(|n| n.to_string_lossy().into_owned())).collect();
                            let _ = MediaAdded { added: imported, names }.emit(&app);
                        }
                    }
                }
                let summary = OffloadSummary {
                    files: rep.files as u32,
                    bytes: rep.bytes as f64,
                    copied: rep.copied as u32,
                    skipped: rep.skipped as u32,
                    failed: rep.failed.iter().map(|(p, k, why)| format!("{p} → {}: {why}", job.destinations.get(*k).map(|d| d.display().to_string()).unwrap_or_default())).collect(),
                    manifests: rep.manifests.iter().map(|m| m.display().to_string()).collect(),
                    seconds: rep.seconds,
                    imported,
                };
                let _ = OffloadProgress { done: rep.bytes as f64, total: rep.bytes as f64, files, index: files, summary: Some(summary), ..blank("finished") }.emit(&app);
            }
            Err(OffloadError::Cancelled) => {
                diag::log("INFO", "offload cancelled");
                let _ = blank("cancelled").emit(&app);
            }
            Err(e) => {
                diag::log("WARN", &format!("offload failed: {e}"));
                let _ = OffloadProgress { message: Some(e.to_string()), ..blank("failed") }.emit(&app);
            }
        }
    });
    Ok(())
}

#[tauri::command]
#[specta::specta]
fn offload_cancel(state: State<'_, AppState>) {
    if let Some(c) = lock(&state.offload).as_ref() {
        c.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

fn start_job(state: &AppState) -> Result<Arc<std::sync::atomic::AtomicBool>, String> {
    let mut slot = lock(&state.job);
    if slot.is_some() {
        return Err("Another proxy or render job is running.".into());
    }
    let c = Arc::new(std::sync::atomic::AtomicBool::new(false));
    *slot = Some(c.clone());
    Ok(c)
}

fn finish_job(app: &tauri::AppHandle, job: &str, result: Result<Vec<String>, String>, count: u32) {
    match &result {
        Ok(v) => diag::log("INFO", &format!("{job} job finished: {} output(s)", v.len())),
        Err(e) => diag::log("WARN", &format!("{job} job ended: {e}")),
    }
    *lock(&app.state::<AppState>().job) = None;
    let base = JobProgress { job: job.into(), kind: String::new(), index: count, count, fraction: 1.0, name: None, message: None, outputs: Vec::new() };
    let e = match result {
        Ok(outputs) => JobProgress { kind: "finished".into(), outputs, ..base },
        Err(m) if m == "cancelled" => JobProgress { kind: "cancelled".into(), ..base },
        Err(m) => JobProgress { kind: "failed".into(), message: Some(m), ..base },
    };
    let _ = e.emit(app);
}

/// Proxy codecs with a working encoder here (P13.2).
#[tauri::command]
#[specta::specta]
async fn proxy_codecs() -> Result<Vec<CodecChoice>, String> {
    blocking(|| {
        Ok(clippro_ingest::proxy::available()
            .into_iter()
            .map(|c| CodecChoice { value: serde_json::to_value(c).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(), label: c.label().into() })
            .collect())
    })
    .await
}

/// Makes and links proxies for `clips` in the background (`JobProgress`, job "proxy").
#[tauri::command]
#[specta::specta]
fn proxies_make(app: tauri::AppHandle, state: State<'_, AppState>, clips: Vec<f64>, codec: String, max_height: u32, out_dir: Option<String>) -> Result<(), String> {
    let s = state.session()?;
    let codec: clippro_ingest::proxy::ProxyCodec = serde_json::from_value(serde_json::Value::String(codec)).map_err(|e| e.to_string())?;
    let cancel = start_job(&state)?;
    let opts = clippro_ingest::proxy::ProxyOptions { codec, max_height: max_height.clamp(144, 4320), out_dir: out_dir.map(PathBuf::from) };
    std::thread::spawn(move || {
        let names = s.clip_names().unwrap_or_default();
        let count = clips.len() as u32;
        let mut outputs = Vec::new();
        let mut last = std::time::Instant::now();
        let mut result = Ok(());
        for (i, c) in clips.iter().enumerate() {
            let id = clippro_types::ClipId(*c as i64);
            let name = names.get(&id).map(|n| n.0.clone());
            let r = s.make_proxy(id, &opts, &cancel, |f| {
                if last.elapsed().as_millis() >= 150 {
                    last = std::time::Instant::now();
                    let _ = JobProgress { job: "proxy".into(), kind: "progress".into(), index: i as u32, count, fraction: f, name: name.clone(), message: None, outputs: Vec::new() }.emit(&app);
                }
            });
            match r {
                Ok(p) => outputs.push(p.display().to_string()),
                Err(e) => {
                    result = Err(e.to_string());
                    break;
                }
            }
        }
        finish_job(&app, "proxy", result.map(|_| outputs), count);
    });
    Ok(())
}

/// Renders the current selects to files in the background (`JobProgress`, job "render").
#[tauri::command]
#[specta::specta]
fn render_selects(app: tauri::AppHandle, state: State<'_, AppState>, settings: Settings, sort: SortOrder, request: RenderRequest) -> Result<(), String> {
    let s = state.session()?;
    let codec: clippro_ingest::render::RenderCodec = serde_json::from_value(serde_json::Value::String(request.codec.clone())).map_err(|e| e.to_string())?;
    let cancel = start_job(&state)?;
    let opts = clippro_ingest::render::RenderOptions {
        stream_copy: request.stream_copy,
        allow_nearest_keyframe: request.allow_nearest_keyframe,
        codec,
        out_dir: PathBuf::from(&request.out_dir),
        template: request.template.clone(),
    };
    std::thread::spawn(move || {
        let mut last = std::time::Instant::now();
        let mut count = 0u32;
        let r = s.render_selects(&settings, sort, &opts, request.handles, &cancel, |i, n, f| {
            count = n as u32;
            if last.elapsed().as_millis() >= 150 {
                last = std::time::Instant::now();
                let _ = JobProgress { job: "render".into(), kind: "progress".into(), index: i as u32, count: n as u32, fraction: f, name: None, message: None, outputs: Vec::new() }.emit(&app);
            }
        });
        let result = r.map(|v| v.iter().map(|o| o.path.display().to_string()).collect::<Vec<_>>()).map_err(|e| e.to_string());
        let n = result.as_ref().map(|v| v.len() as u32).unwrap_or(count);
        finish_job(&app, "render", result, n);
    });
    Ok(())
}

#[tauri::command]
#[specta::specta]
fn job_cancel(state: State<'_, AppState>) {
    if let Some(c) = lock(&state.job).as_ref() {
        c.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The last `lines` lines of the log (P14.5 log viewer).
#[tauri::command]
#[specta::specta]
fn diagnostics_log(lines: u32) -> Vec<String> {
    diag::dir().map(|d| diag::tail(&d, lines.clamp(1, 20_000) as usize)).unwrap_or_default()
}

/// Writes the diagnostics report (system, settings without paths, recent log and crashes).
#[tauri::command]
#[specta::specta]
async fn diagnostics_save(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let settings = serde_json::to_string_pretty(&*lock(&state.settings)).unwrap_or_default();
    let dir = diag::dir().ok_or("Logging isn't running.")?;
    blocking(move || {
        let encoders = ["prores_ks", "dnxhd", "h264_nvenc", "h264_amf", "h264_qsv", "h264_mf", "libopenh264"]
            .iter()
            .filter(|e| clippro_ingest::ffmpeg::encoder_works(e))
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        let system = [
            ("Version", env!("CARGO_PKG_VERSION").to_string()),
            ("OS", format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)),
            ("CPU threads", std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).to_string()),
            ("GPU decode (D3D11VA)", clippro_media::video::hw_decode_available().to_string()),
            ("ffmpeg.exe", clippro_ingest::ffmpeg::ffmpeg_exe().map(|_| "found".to_string()).unwrap_or_else(|| "missing".into())),
            ("Working encoders", if encoders.is_empty() { "none".into() } else { encoders }),
        ];
        std::fs::write(&path, diag::report(&dir, &system, &settings)).map_err(|e| format!("{path}: {e}"))
    })
    .await
}

/// Opens the log folder in Explorer.
#[tauri::command]
#[specta::specta]
fn diagnostics_open_folder() -> Result<(), String> {
    let dir = diag::dir().ok_or("Logging isn't running.")?;
    #[cfg(windows)]
    std::process::Command::new("explorer.exe").arg(&dir).spawn().map_err(|e| e.to_string())?;
    let _ = dir;
    Ok(())
}

/// HTML report, or the contact sheet printed to PDF in a hidden WebView2 window (P7.6).
async fn export_report(app: tauri::AppHandle, s: Arc<Session>, settings: Settings, sort: SortOrder, request: ExportRequest) -> Result<ExportResult, String> {
    use clippro_core::report::{render_contact_sheet, render_html, utc_now_string, ReportOptions};
    let opts = ReportOptions { title: request.name.clone(), generated: request.generated.clone().unwrap_or_else(utc_now_string), ..Default::default() };
    let pdf = request.format == "pdf";
    let (n, html) = blocking(move || {
        let d = s.report_data(&settings, sort, &opts).map_err(|e| e.to_string())?;
        Ok((d.rows.len(), if pdf { render_contact_sheet(&d) } else { render_html(&d) }))
    })
    .await?;
    let out = PathBuf::from(&request.path);
    if !pdf {
        std::fs::write(&out, html).map_err(|e| e.to_string())?;
        return Ok(ExportResult { path: request.path, selects: n as u32 });
    }
    #[cfg(windows)]
    {
        // The sheet is loaded from a temporary file beside the PDF (never the system temp dir).
        let tmp = out.with_extension("contact-sheet.tmp.html");
        std::fs::write(&tmp, html).map_err(|e| e.to_string())?;
        let result = print_sheet(&app, &tmp, &out).await;
        let _ = std::fs::remove_file(&tmp);
        result?;
        Ok(ExportResult { path: request.path, selects: n as u32 })
    }
    #[cfg(not(windows))]
    {
        let _ = (app, html, n);
        Err("PDF export needs WebView2".into())
    }
}

async fn pdf_check(app: &tauri::AppHandle, out: &std::path::Path, clips: &[PathBuf]) -> Result<usize, String> {
    let proj = out.with_extension("clippro");
    let _ = std::fs::remove_file(&proj);
    let clips = clips.to_vec();
    let p2 = proj.clone();
    let s = blocking(move || {
        let s = Session::create(&p2, "PDF check", SessionOptions { cache_path: None, ..settings::AppSettings::default().session_options() }).map_err(|e| e.to_string())?;
        s.import(&clips).map_err(|e| e.to_string())?;
        s.queue_analysis().map_err(|e| e.to_string())?;
        s.wait(|_| {});
        Ok(Arc::new(s))
    })
    .await?;
    let settings = Settings::from_preset(&clippro_score::builtin("Cleanup").ok_or("no preset")?).map_err(|e| e.to_string())?;
    let req = ExportRequest {
        format: "pdf".into(),
        path: out.to_string_lossy().into_owned(),
        name: "PDF check".into(),
        layout: "selects".into(),
        group_by: None,
        group_order: None,
        separator: None,
        separator_s: None,
        handles: true,
        gap_s: 0.0,
        with_audio: false,
        markers: false,
        marker_name: String::new(),
        marker_comment: String::new(),
        generated: None,
    };
    let r = export_report(app.clone(), s.clone(), settings, SortOrder::Timeline, req).await;
    close_session(s);
    for ext in ["clippro", "clippro-wal", "clippro-shm"] {
        let _ = std::fs::remove_file(out.with_extension(ext));
    }
    r.map(|r| r.selects as usize)
}

#[cfg(windows)]
async fn print_sheet(app: &tauri::AppHandle, html: &std::path::Path, pdf: &std::path::Path) -> Result<(), String> {
    use tauri::Manager as _;
    app.asset_protocol_scope().allow_file(html).map_err(|e| e.to_string())?;
    let encoded: String = html
        .to_string_lossy()
        .bytes()
        // Same encoding as `convertFileSrc` (encodeURIComponent).
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect();
    let url: tauri::Url = format!("http://asset.localhost/{encoded}").parse().map_err(|e| format!("{e}"))?;
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let label = format!("pdf-{}", SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let tx = Mutex::new(Some(tx));
    let window = tauri::WebviewWindowBuilder::new(app, &label, tauri::WebviewUrl::External(url))
        .visible(false)
        .inner_size(1123.0, 794.0)
        .on_page_load(move |_, p| {
            if p.event() == tauri::webview::PageLoadEvent::Finished {
                if let Some(tx) = lock(&tx).take() {
                    let _ = tx.send(());
                }
            }
        })
        .build()
        .map_err(|e| e.to_string())?;
    let pdf = pdf.to_path_buf();
    let w = window.clone();
    let r = tauri::async_runtime::spawn_blocking(move || {
        rx.recv_timeout(std::time::Duration::from_secs(30)).map_err(|_| "The contact sheet didn't load".to_string())?;
        shared::print_to_pdf(&w, &pdf)
    })
    .await
    .map_err(|e| e.to_string())?;
    let _ = window.destroy();
    r
}

// ---------- Wiring ----------

/// Frame inspector payload (SPEC VIE-2/4/5). Unavailable values are `null` (NaN in Rust).
#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct InspectResult {
    pub sample: u32,
    pub time_us: f64,
    pub pass: bool,
    /// 1-based select number within the clip.
    pub select: Option<u32>,
    pub reasons: Vec<String>,
    pub score: f32,
    pub overall: f32,
    pub tier_multiplier: f32,
    pub camera_factor: f32,
    pub force_in: bool,
    pub force_out: bool,
    pub flags: Vec<String>,
    pub groups: Vec<InspectGroup>,
    pub subs: Vec<InspectSub>,
    pub raw: Vec<InspectRaw>,
    pub tile_cols: u32,
    pub tile_rows: u32,
    pub tiles: Vec<f32>,
    pub mv_cols: u32,
    pub mv_rows: u32,
    /// Flattened (dx, dy) per motion cell, working-image pixels.
    pub mvs: Vec<i32>,
    pub hist: Vec<u32>,
    pub working_w: u32,
    pub working_h: u32,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct InspectGroup {
    pub key: String,
    pub label: String,
    pub score: f32,
    pub weight: f32,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct InspectSub {
    pub key: String,
    pub label: String,
    pub group: String,
    /// Normalised measurement (0–1).
    pub value: f32,
    /// Sub-metric score (0–1) after the preference/target curve.
    pub score: f32,
    pub weight: f32,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct InspectRaw {
    pub key: String,
    pub value: f32,
}

/// Inspector data for `sample` of `clip` with the current settings.
#[tauri::command]
#[specta::specta]
async fn inspect_sample(state: State<'_, AppState>, settings: Settings, clip: f64, sample: u32) -> Result<InspectResult, String> {
    let s = state.session()?;
    blocking(move || {
        let i = s.inspect(clippro_types::ClipId(clip as i64), &settings, sample as usize).map_err(|e| e.to_string())?;
        let b = &i.breakdown;
        let gkey = |g: clippro_types::MetricGroup| format!("{g:?}").to_lowercase();
        Ok(InspectResult {
            sample: i.sample as u32,
            time_us: i.time_us as f64,
            pass: i.pass,
            select: i.select.map(|x| x as u32 + 1),
            reasons: i.reasons.clone(),
            score: b.score,
            overall: b.overall,
            tier_multiplier: b.tier_multiplier,
            camera_factor: b.camera_factor,
            force_in: b.force_in,
            force_out: b.force_out,
            flags: b.flags.iter().map(|f| f.key().to_string()).collect(),
            groups: b.groups.iter().map(|(g, sc, w)| InspectGroup { key: gkey(*g), label: g.label().to_string(), score: *sc, weight: *w }).collect(),
            subs: b
                .subs
                .iter()
                .map(|(sm, v, sc, w)| InspectSub { key: sm.key().to_string(), label: sm.label().to_string(), group: gkey(sm.group()), value: *v, score: *sc, weight: *w })
                .collect(),
            raw: i.raw.iter().map(|(k, v)| InspectRaw { key: (*k).to_string(), value: *v }).collect(),
            tile_cols: i.tile_dims.0 as u32,
            tile_rows: i.tile_dims.1 as u32,
            tiles: i.tiles.clone(),
            mv_cols: i.mv_dims.0 as u32,
            mv_rows: i.mv_dims.1 as u32,
            mvs: i.mvs.iter().flat_map(|v| [v[0] as i32, v[1] as i32]).collect(),
            hist: i.hist.iter().map(|&h| h as u32).collect(),
            working_w: i.working.0,
            working_h: i.working.1,
        })
    })
    .await
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct CameraMatch {
    pub clip: f64,
    pub name: String,
    /// Mean camera-filter factor over the clip (0–1).
    pub score: f32,
}

/// The clips that best match the camera-movement filter in `settings` (live preview, TUN-6).
#[tauri::command]
#[specta::specta]
async fn camera_matches(state: State<'_, AppState>, settings: Settings, count: u32) -> Result<Vec<CameraMatch>, String> {
    let s = state.session()?;
    blocking(move || {
        let names = s.clip_names().map_err(|e| e.to_string())?;
        Ok(s.camera_matches(&settings, count as usize)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|(c, score)| CameraMatch { clip: c.0 as f64, name: names.get(&c).map(|n| n.0.clone()).unwrap_or_default(), score })
            .collect())
    })
    .await
}

/// One sub-metric, in `Settings::subs` order (SPEC TUN-5).
#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct SubMetricInfo {
    pub key: String,
    pub label: String,
    /// focus | exposure | motion | audio | content (`Settings::weights` order)
    pub group: String,
    /// Target-type (Luma, Contrast, Loudness): a target value and tolerance, not a direction.
    pub target: bool,
}

#[tauri::command]
#[specta::specta]
async fn submetrics_list() -> Result<Vec<SubMetricInfo>, String> {
    Ok(clippro_score::SubMetric::ALL
        .iter()
        .map(|m| SubMetricInfo { key: m.key().into(), label: m.label().into(), group: format!("{:?}", m.group()).to_lowercase(), target: m.is_target() })
        .collect())
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct BinDto {
    pub id: f64,
    pub name: String,
    /// manual | smart
    pub kind: String,
    /// Smart collections: the UI's query document.
    pub query: Option<String>,
}

#[tauri::command]
#[specta::specta]
async fn bins_list(state: State<'_, AppState>) -> Result<Vec<BinDto>, String> {
    let s = state.session()?;
    let rows = lock(&s.db).bins().map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|b| BinDto { id: b.id as f64, name: b.name, kind: b.kind, query: b.query_json }).collect())
}

/// Adds a bin (`kind` manual) or smart collection (`kind` smart, with `query`).
#[tauri::command]
#[specta::specta]
async fn bin_add(state: State<'_, AppState>, name: String, kind: String, query: Option<String>) -> Result<f64, String> {
    let s = state.session()?;
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Give it a name.".into());
    }
    if kind != "manual" && kind != "smart" {
        return Err("Unknown bin kind".into());
    }
    let r = lock(&s.db).add_bin(&name, &kind, query.as_deref()).map(|id| id as f64).map_err(|e| e.to_string());
    r
}

#[tauri::command]
#[specta::specta]
async fn bin_update(state: State<'_, AppState>, id: f64, name: String, query: Option<String>) -> Result<(), String> {
    let s = state.session()?;
    let r = lock(&s.db).update_bin(id as i64, name.trim(), query.as_deref()).map_err(|e| e.to_string());
    r
}

#[tauri::command]
#[specta::specta]
async fn bin_delete(state: State<'_, AppState>, id: f64) -> Result<(), String> {
    let s = state.session()?;
    let r = lock(&s.db).delete_bin(id as i64).map_err(|e| e.to_string());
    r
}

/// Moves clips into a bin (`bin` null = back to the project root).
#[tauri::command]
#[specta::specta]
async fn clips_set_bin(state: State<'_, AppState>, clips: Vec<f64>, bin: Option<f64>) -> Result<(), String> {
    let s = state.session()?;
    let db = lock(&s.db);
    for c in clips {
        db.set_clip_bin(clippro_types::ClipId(c as i64), bin.map(|b| b as i64)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct ProjectTemplate {
    pub name: String,
    /// The UI's template document (tuning, watch folders, export defaults).
    pub json: String,
}

fn templates_dir(state: &AppState) -> Option<PathBuf> {
    lock(&state.presets_dir).as_ref().and_then(|p| p.parent().map(|d| d.join("templates")))
}

#[tauri::command]
#[specta::specta]
async fn templates_list(state: State<'_, AppState>) -> Result<Vec<ProjectTemplate>, String> {
    let Some(dir) = templates_dir(&state) else { return Ok(Vec::new()) };
    let Ok(rd) = std::fs::read_dir(&dir) else { return Ok(Vec::new()) };
    let mut out: Vec<ProjectTemplate> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|t| serde_json::from_str::<ProjectTemplate>(&t).ok())
        .collect();
    out.sort_by_key(|t| t.name.to_lowercase());
    Ok(out)
}

#[tauri::command]
#[specta::specta]
async fn template_save(state: State<'_, AppState>, template: ProjectTemplate) -> Result<(), String> {
    let dir = templates_dir(&state).ok_or("No config folder")?;
    if template.name.trim().is_empty() {
        return Err("Give the template a name.".into());
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&template).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{}.json", presets::slug(&template.name))), text).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
async fn template_delete(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let dir = templates_dir(&state).ok_or("No config folder")?;
    std::fs::remove_file(dir.join(format!("{}.json", presets::slug(&name)))).map_err(|e| e.to_string())
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct OverrideDto {
    pub id: f64,
    pub clip: f64,
    /// force_in | force_out | trim | manual
    pub kind: String,
    pub start_us: f64,
    pub end_us: f64,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct AnnotationDto {
    pub clip: f64,
    pub start_us: f64,
    pub end_us: f64,
    pub rating: u8,
    /// A marker colour name (red, orange, yellow, green, cyan, blue, purple, magenta, white).
    pub label: Option<String>,
    pub note: String,
}

#[tauri::command]
#[specta::specta]
async fn overrides_list(state: State<'_, AppState>) -> Result<Vec<OverrideDto>, String> {
    let s = state.session()?;
    Ok(s.list_overrides()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|o| OverrideDto { id: o.id as f64, clip: o.clip.0 as f64, kind: o.kind, start_us: o.start_us as f64, end_us: o.end_us as f64 })
        .collect())
}

/// Adds a manual override (force in/out, trim, manual segment); rescore afterwards.
#[tauri::command]
#[specta::specta]
async fn override_add(state: State<'_, AppState>, clip: f64, kind: String, start_us: f64, end_us: f64) -> Result<f64, String> {
    let s = state.session()?;
    s.add_override(clippro_types::ClipId(clip as i64), &kind, start_us as i64, end_us as i64).map(|id| id as f64).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
async fn override_remove(state: State<'_, AppState>, id: f64) -> Result<(), String> {
    let s = state.session()?;
    s.remove_override(id as i64).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
async fn annotations_list(state: State<'_, AppState>) -> Result<Vec<AnnotationDto>, String> {
    let s = state.session()?;
    Ok(s.annotations()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|a| AnnotationDto { clip: a.clip.0 as f64, start_us: a.start_us as f64, end_us: a.end_us as f64, rating: a.rating, label: a.label, note: a.note })
        .collect())
}

/// Sets (or clears, when empty) the rating, colour label and note of a select.
#[tauri::command]
#[specta::specta]
async fn annotation_set(state: State<'_, AppState>, a: AnnotationDto) -> Result<(), String> {
    let s = state.session()?;
    s.set_annotation(clippro_types::ClipId(a.clip as i64), a.start_us as i64, a.end_us as i64, a.rating, a.label, &a.note).map(|_| ()).map_err(|e| e.to_string())
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct Snapshot {
    pub id: f64,
    pub name: String,
    /// The UI's tuning document (preset name + settings), opaque to the backend.
    pub json: String,
    pub created: String,
}

#[tauri::command]
#[specta::specta]
async fn snapshots_list(state: State<'_, AppState>) -> Result<Vec<Snapshot>, String> {
    let s = state.session()?;
    let rows = lock(&s.db).snapshots().map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|(id, name, json, created)| Snapshot { id: id as f64, name, json, created }).collect())
}

#[tauri::command]
#[specta::specta]
async fn snapshot_save(state: State<'_, AppState>, name: String, json: String) -> Result<(), String> {
    let s = state.session()?;
    let name = name.trim();
    if name.is_empty() {
        return Err("Give the snapshot a name.".into());
    }
    let r = lock(&s.db).save_snapshot(name, &json).map(|_| ()).map_err(|e| e.to_string());
    r
}

#[tauri::command]
#[specta::specta]
async fn snapshot_delete(state: State<'_, AppState>, id: f64) -> Result<(), String> {
    let s = state.session()?;
    let r = lock(&s.db).delete_snapshot(id as i64).map_err(|e| e.to_string());
    r
}

/// Saves the tuning in effect, restored when the project is reopened.
#[tauri::command]
#[specta::specta]
async fn tuning_save(state: State<'_, AppState>, json: String) -> Result<(), String> {
    let s = state.session()?;
    let r = lock(&s.db).set_current_tuning(&json).map_err(|e| e.to_string());
    r
}

#[tauri::command]
#[specta::specta]
async fn tuning_load(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let s = state.session()?;
    let r = lock(&s.db).current_tuning().map_err(|e| e.to_string());
    r
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct WatchFolderDto {
    pub path: String,
    pub subfolders: bool,
    /// Lower-case extensions without the dot; empty = all media.
    pub extensions: Vec<String>,
}

#[tauri::command]
#[specta::specta]
async fn watch_folders_get(state: State<'_, AppState>) -> Result<Vec<WatchFolderDto>, String> {
    let s = state.session()?;
    Ok(s.watch_folders().map_err(|e| e.to_string())?.into_iter().map(|f| WatchFolderDto { path: f.path, subfolders: f.subfolders, extensions: f.extensions }).collect())
}

/// Replaces the project's watch folders and restarts watching.
#[tauri::command]
#[specta::specta]
async fn watch_folders_set(state: State<'_, AppState>, folders: Vec<WatchFolderDto>) -> Result<(), String> {
    let s = state.session()?;
    let folders: Vec<clippro_core::watch::WatchFolder> = folders
        .into_iter()
        .map(|f| clippro_core::watch::WatchFolder {
            path: f.path,
            subfolders: f.subfolders,
            extensions: f.extensions.iter().map(|e| e.trim().trim_start_matches('.').to_lowercase()).filter(|e| !e.is_empty()).collect(),
        })
        .collect();
    for f in &folders {
        if !std::path::Path::new(&f.path).is_dir() {
            return Err(format!("{} isn't a folder.", f.path));
        }
    }
    s.set_watch_folders(&folders).map_err(|e| e.to_string())?;
    restart_watch(&state);
    Ok(())
}

/// Save As (PRJ-1): writes a consistent copy and continues in it.
#[tauri::command]
#[specta::specta]
async fn project_save_as(state: State<'_, AppState>, path: String) -> Result<ProjectInfo, String> {
    let s = state.session()?;
    let dest = PathBuf::from(&path);
    if lock(&state.path).as_deref() == Some(dest.as_path()) {
        return Err("That's the project's current file.".into());
    }
    let d2 = dest.clone();
    blocking(move || {
        let r = lock(&s.db).save_as(&d2).map_err(|e| e.to_string());
        r
    })
    .await?;
    let o = options(&state);
    let d3 = dest.clone();
    let (s, _) = blocking(move || Session::open(&d3, o).map_err(|e| e.to_string())).await?;
    let name = lock(&s.db).name().map_err(|e| e.to_string())?;
    replace_session(&state, Arc::new(s), dest);
    Ok(ProjectInfo { path, name, recovered: false })
}

/// Points a clip's media at a moved file (content must match).
#[tauri::command]
#[specta::specta]
async fn media_relink(state: State<'_, AppState>, media: f64, path: String) -> Result<(), String> {
    let s = state.session()?;
    state.preview.clear();
    blocking(move || s.relink(clippro_types::MediaId(media as i64), std::path::Path::new(&path)).map_err(|e| e.to_string())).await
}

/// Relinks every missing media file found under `folder`; returns how many were found.
#[tauri::command]
#[specta::specta]
async fn media_relink_folder(state: State<'_, AppState>, folder: String) -> Result<u32, String> {
    let s = state.session()?;
    state.preview.clear();
    blocking(move || s.relink_missing(&[PathBuf::from(folder)]).map(|v| v.len() as u32).map_err(|e| e.to_string())).await
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct AudioStreamDto {
    /// Container stream index.
    pub index: u32,
    pub codec: String,
    pub channels: u32,
    pub layout: String,
    pub language: String,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct AudioStreams {
    pub streams: Vec<AudioStreamDto>,
    /// The chosen stream's index; `null` = automatic (the best stream).
    pub selected: Option<u32>,
}

#[tauri::command]
#[specta::specta]
async fn audio_streams(state: State<'_, AppState>, clip: f64) -> Result<AudioStreams, String> {
    let s = state.session()?;
    let (streams, sel) = s.audio_streams(clippro_types::ClipId(clip as i64)).map_err(|e| e.to_string())?;
    Ok(AudioStreams {
        streams: streams.into_iter().map(|a| AudioStreamDto { index: a.index as u32, codec: a.codec, channels: a.channels, layout: a.layout, language: a.language }).collect(),
        selected: sel.map(|v| v as u32),
    })
}

/// Picks the audio stream to analyse and play for a clip (null = automatic); re-analyses it.
#[tauri::command]
#[specta::specta]
async fn audio_stream_set(state: State<'_, AppState>, clip: f64, stream: Option<u32>) -> Result<(), String> {
    let s = state.session()?;
    blocking(move || s.set_audio_stream(clippro_types::ClipId(clip as i64), stream.map(|v| v as usize)).map_err(|e| e.to_string())).await
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct AudioLevels {
    /// Peak per output channel since the last call (dBFS).
    pub levels: Vec<f32>,
    pub source_channels: u32,
}

/// Playback meters (P10.2); call at display rate during frame playback.
#[tauri::command]
#[specta::specta]
async fn audio_levels(state: State<'_, AppState>) -> Result<AudioLevels, String> {
    let (levels, n) = state.audio.take_levels();
    Ok(AudioLevels { levels, source_channels: n as u32 })
}

/// Per-source-channel playback gains: 0 mutes, 1 plays (solo = mute the others).
#[tauri::command]
#[specta::specta]
async fn audio_channel_gains(state: State<'_, AppState>, gains: Vec<f32>) -> Result<(), String> {
    state.audio.set_channel_gains(gains.into_iter().map(|g| g.clamp(0.0, 1.0)).collect());
    Ok(())
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct Waveform {
    pub clip: f64,
    pub start_us: f64,
    /// Peak amplitude per 100 ms block, 0–255 (linear, 255 = full scale), base64.
    pub peaks: String,
    /// Integrated loudness of the whole clip (LUFS), if not silent.
    pub lufs: Option<f32>,
}

/// Audio waveform envelopes for the timeline lane (P10.2).
#[tauri::command]
#[specta::specta]
async fn audio_waveforms(state: State<'_, AppState>) -> Result<Vec<Waveform>, String> {
    let s = state.session()?;
    blocking(move || {
        let b64 = base64::engine::general_purpose::STANDARD;
        Ok(s.clip_audio()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|(c, a)| {
                let peaks: Vec<u8> = a.cols.peak_dbfs.iter().map(|d| (10f32.powf(d / 20.0).clamp(0.0, 1.0) * 255.0).round() as u8).collect();
                Waveform { clip: c.0 as f64, start_us: a.start_us as f64, peaks: b64.encode(peaks), lufs: a.integrated_lufs(i64::MIN / 2, i64::MAX / 2) }
            })
            .collect())
    })
    .await
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct CacheInfo {
    pub path: String,
    pub entries: f64,
    pub bytes: f64,
}

/// The global measurement cache (Settings → Cache).
#[tauri::command]
#[specta::specta]
async fn cache_info() -> Result<CacheInfo, String> {
    blocking(|| {
        let path = clippro_project::MeasureCache::default_path();
        if !path.exists() {
            return Ok(CacheInfo { path: path.to_string_lossy().into_owned(), entries: 0.0, bytes: 0.0 });
        }
        let c = clippro_project::MeasureCache::open(&path).map_err(|e| e.to_string())?;
        let (n, b) = c.stats().map_err(|e| e.to_string())?;
        Ok(CacheInfo { path: path.to_string_lossy().into_owned(), entries: n as f64, bytes: b as f64 })
    })
    .await
}

/// Empties the global measurement cache; projects keep their own measurements.
#[tauri::command]
#[specta::specta]
async fn cache_clear() -> Result<f64, String> {
    blocking(|| {
        let path = clippro_project::MeasureCache::default_path();
        if !path.exists() {
            return Ok(0.0);
        }
        let c = clippro_project::MeasureCache::open(&path).map_err(|e| e.to_string())?;
        c.clear().map(|n| n as f64).map_err(|e| e.to_string())
    })
    .await
}

/// Writes the app settings to a JSON file (Settings → General → Export).
#[tauri::command]
#[specta::specta]
async fn settings_export(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let mut s = lock(&state.settings).clone();
    // Machine-specific: not part of a portable settings file.
    s.last_project = None;
    s.recent_projects.clear();
    std::fs::write(path, serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// Reads app settings from a JSON file, keeps this machine's recent projects, saves and
/// broadcasts them.
#[tauri::command]
#[specta::specta]
async fn settings_import(app: tauri::AppHandle, state: State<'_, AppState>, path: String) -> Result<settings::AppSettings, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut s: settings::AppSettings = serde_json::from_str(&text).map_err(|e| format!("Not a ClipPro settings file: {e}"))?;
    {
        let cur = lock(&state.settings);
        s.last_project = cur.last_project.clone();
        s.recent_projects = cur.recent_projects.clone();
        // The data folder belongs to this PC (it may not exist on the one the file came from).
        s.data_dir = cur.data_dir.clone();
        s.pending_cache_move = cur.pending_cache_move.clone();
    }
    let s = s.sanitised();
    if let Some(p) = lock(&state.settings_path).as_ref() {
        s.save(p)?;
    }
    *lock(&state.settings) = s.clone();
    let _ = s.emit(&app);
    Ok(s)
}

#[tauri::command]
#[specta::specta]
async fn settings_get(state: State<'_, AppState>) -> Result<settings::AppSettings, String> {
    Ok(lock(&state.settings).clone())
}

/// Saves the app settings and tells every window (`AppSettings` event).
#[tauri::command]
#[specta::specta]
async fn settings_set(app: tauri::AppHandle, state: State<'_, AppState>, settings: settings::AppSettings) -> Result<settings::AppSettings, String> {
    let mut s = settings.sanitised();
    {
        // Only `data_dir_set` changes the data folder, since it also moves the files.
        let cur = lock(&state.settings);
        s.data_dir = cur.data_dir.clone();
        s.pending_cache_move = cur.pending_cache_move.clone();
    }
    if let Some(p) = lock(&state.settings_path).as_ref() {
        s.save(p)?;
    }
    *lock(&state.settings) = s.clone();
    let _ = s.emit(&app);
    Ok(s)
}

/// Opens (or focuses) the Settings window.
#[tauri::command]
#[specta::specta]
async fn open_settings(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.set_focus();
        return Ok(());
    }
    let w = tauri::WebviewWindowBuilder::new(&app, "settings", tauri::WebviewUrl::App("index.html?window=settings".into()))
        .title("ClipPro Settings")
        .inner_size(620.0, 460.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .transparent(true)
        .center()
        .build()
        .map_err(|e| e.to_string())?;
    let material = apply_material(&w);
    let _ = w.eval(format!("document.body.classList.toggle('has-material', {material});"));
    Ok(())
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct SystemInfo {
    /// D3D11VA hardware decoding works on this machine.
    pub hw_decode: bool,
    pub version: String,
}

/// `CLIPPRO_E2E=1`: the WebDriver end-to-end test is driving the app (P5.7).
fn e2e_mode() -> bool {
    std::env::var("CLIPPRO_E2E").is_ok_and(|v| v == "1")
}

/// Whether the end-to-end test is driving the app; the UI then exposes its store to it.
#[tauri::command]
#[specta::specta]
fn e2e_enabled() -> bool {
    e2e_mode()
}

/// A project passed on the command line (double-clicking a `.clippro` file), if any.
#[tauri::command]
#[specta::specta]
fn startup_project() -> Option<String> {
    std::env::args().skip(1).find(|a| a.to_lowercase().ends_with(".clippro") && std::path::Path::new(a).is_file())
}

/// Machine capabilities for the status bar.
#[tauri::command]
#[specta::specta]
async fn system_info() -> Result<SystemInfo, String> {
    let hw = blocking(|| Ok(clippro_media::video::hw_decode_available())).await?;
    Ok(SystemInfo { hw_decode: hw, version: env!("CARGO_PKG_VERSION").to_string() })
}

/// Analyses a clip again; `cpu` decodes without the GPU (context menu). False if it can't be
/// analysed (no video / proxy required).
#[tauri::command]
#[specta::specta]
async fn clip_retry(state: State<'_, AppState>, clip: f64, cpu: bool) -> Result<bool, String> {
    let s = state.session()?;
    blocking(move || s.retry(clippro_types::ClipId(clip as i64), cpu).map_err(|e| e.to_string())).await
}

/// Moves a queued clip to the front of the analysis queue.
#[tauri::command]
#[specta::specta]
async fn clip_bump(state: State<'_, AppState>, clip: f64) -> Result<(), String> {
    let s = state.session()?;
    blocking(move || s.bump(clippro_types::ClipId(clip as i64)).map_err(|e| e.to_string())).await
}

/// Links a proxy file to a clip by hand and re-analyses from it.
#[tauri::command]
#[specta::specta]
async fn clip_link_proxy(state: State<'_, AppState>, clip: f64, path: String) -> Result<(), String> {
    let s = state.session()?;
    state.preview.clear();
    blocking(move || s.link_proxy(clippro_types::ClipId(clip as i64), std::path::Path::new(&path)).map_err(|e| e.to_string())).await
}

/// Shows a file in Explorer with it selected.
#[tauri::command]
#[specta::specta]
async fn reveal_in_explorer(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err("The file isn't there any more.".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // Explorer wants `/select,"path"` as one raw argument.
        std::process::Command::new("explorer.exe").raw_arg(format!("/select,\"{}\"", p.display())).spawn().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Assigns a log/colour profile to `media` (SPEC VIE-6). Rescore afterwards: it changes Luma.
#[tauri::command]
#[specta::specta]
async fn media_set_profile(state: State<'_, AppState>, media: f64, profile: clippro_types::ColourProfile) -> Result<(), String> {
    let s = state.session()?;
    blocking(move || s.set_profile(clippro_types::MediaId(media as i64), profile).map_err(|e| e.to_string())).await
}

/// Viewer colour transform for `profile`: a per-channel decode LUT (code value → scene linear,
/// `PREVIEW_LUT` entries) and the gamut → BT.709 matrix (row-major). The shader then applies
/// the display rendering, which matches `clippro_score::colour::display`.
#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct PreviewColour {
    pub lut: Vec<f32>,
    pub matrix: Vec<f32>,
}

const PREVIEW_LUT: usize = 1024;

/// Reads a `.cube` LUT the user picked (parsed in the UI). Only `.cube` files up to 32 MB.
#[tauri::command]
#[specta::specta]
async fn lut_read(path: String) -> Result<String, String> {
    let p = PathBuf::from(&path);
    if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cube")) {
        return Err("Choose a .cube file.".into());
    }
    let len = std::fs::metadata(&p).map_err(|e| e.to_string())?.len();
    if len > 32 << 20 {
        return Err("This LUT is larger than 32 MB.".into());
    }
    std::fs::read_to_string(&p).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
async fn preview_colour(profile: clippro_types::ColourProfile) -> Result<PreviewColour, String> {
    use clippro_score::colour;
    Ok(PreviewColour { lut: colour::preview_lut(profile, PREVIEW_LUT), matrix: colour::gamut_to_709(profile).iter().flatten().copied().collect() })
}

/// How to show `media` in the viewer (ADR-002). Grants the asset protocol access to the file
/// when it can play natively.
#[tauri::command]
#[specta::specta]
async fn media_source(app: tauri::AppHandle, state: State<'_, AppState>, media: f64) -> Result<preview::MediaSource, String> {
    let s = state.session()?;
    let (path, info) = blocking(move || s.preview_source(clippro_types::MediaId(media as i64)).map_err(|e| e.to_string())).await?.ok_or("Unknown media")?;
    let src = preview::media_source(&path, &info).ok_or("This media has no video stream.")?;
    if src.native_path.is_some() {
        use tauri::Manager as _;
        app.asset_protocol_scope().allow_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(src)
}

/// Decodes the frame shown at `us` as raw I420 and posts it to the page through a WebView2
/// SharedBuffer (`sharedbufferreceived`; `additionalData` is `{"media","us","tag"}`).
#[tauri::command]
#[specta::specta]
async fn preview_shared(window: tauri::WebviewWindow, state: State<'_, AppState>, media: f64, us: f64, width: u32, tag: u32, rate: Option<f64>) -> Result<(), String> {
    let s = state.session()?;
    let app = window.app_handle().clone();
    let (m, t) = (media as i64, us as i64);
    let rate = rate.filter(|r| r.is_finite() && *r != 0.0);
    let s2 = s.clone();
    let (pts, data) = blocking(move || app.state::<AppState>().preview.frame_at(&s2, preview::FrameKind::Raw, m, t, width, rate)).await?;
    let meta = format!(r#"{{"media":{m},"us":{t},"pts":{pts},"tag":{tag}}}"#);
    #[cfg(windows)]
    {
        let posted = shared::post(&window, data, meta);
        if let (Some(rate), Ok(())) = (rate, &posted) {
            let app = window.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || app.state::<AppState>().preview.prefetch(&s, preview::FrameKind::Raw, m, pts, width, rate));
        }
        posted
    }
    #[cfg(not(windows))]
    {
        let _ = (data, meta, s);
        Err("SharedBuffer needs WebView2".into())
    }
}

/// Starts audio for frame playback of `media` at `us`; the viewer follows `audio_clock`.
/// Returns false when the media has no audio.
#[tauri::command]
#[specta::specta]
async fn audio_play(state: State<'_, AppState>, media: f64, us: f64) -> Result<bool, String> {
    let s = state.session()?;
    let (path, info) = s.preview_source(clippro_types::MediaId(media as i64)).map_err(|e| e.to_string())?.ok_or("Unknown media")?;
    if info.audio.is_empty() {
        state.audio.stop();
        return Ok(false);
    }
    // The chosen stream applies to the original; a proxy plays its best stream.
    let stream = s.clip_statuses().ok().and_then(|cs| cs.into_iter().find(|c| c.media.0 == media as i64)).and_then(|c| s.audio_streams(c.clip).ok()).and_then(|(_, sel)| sel);
    let stream = stream.filter(|i| info.audio.iter().any(|a| a.index == *i));
    state.audio.play(path, stream, info.start_us, us.max(0.0) as i64, 1.0)?;
    Ok(true)
}

#[tauri::command]
#[specta::specta]
async fn audio_stop(state: State<'_, AppState>) -> Result<(), String> {
    state.audio.stop();
    Ok(())
}

/// Media time being heard (µs), `null` when no audio is playing.
#[tauri::command]
#[specta::specta]
async fn audio_clock(state: State<'_, AppState>) -> Result<Option<f64>, String> {
    Ok(state.audio.position_us().map(|v| v as f64))
}

fn frame_response(app: &tauri::AppHandle, req: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    use tauri::http::{header, Response, StatusCode};
    let fail = |code: StatusCode, msg: String| Response::builder().status(code).header(header::CONTENT_TYPE, "text/plain").header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*").body(msg.into_bytes()).unwrap_or_default();
    let Some(r) = preview::parse_frame_uri(req.uri().path(), req.uri().query()) else {
        return fail(StatusCode::NOT_FOUND, "Unknown preview request".into());
    };
    let state = app.state::<AppState>();
    let Ok(session) = state.session() else {
        return fail(StatusCode::SERVICE_UNAVAILABLE, "No project is open.".into());
    };
    let mime = match r.kind {
        preview::FrameKind::Jpeg => "image/jpeg",
        preview::FrameKind::Raw => "application/octet-stream",
    };
    match state.preview.frame_at(&session, r.kind, r.media, r.us, r.width, r.rate) {
        Ok((_, jpeg)) => Response::builder()
            .header(header::CONTENT_TYPE, mime)
            .header(header::CACHE_CONTROL, "no-store")
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(jpeg)
            .unwrap_or_default(),
        Err(e) => fail(StatusCode::UNPROCESSABLE_ENTITY, e),
    }
}

/// Clips to benchmark when started with `--bench-preview`, else `None`.
#[tauri::command]
#[specta::specta]
async fn bench_config(state: State<'_, AppState>) -> Result<Option<Vec<BenchClip>>, String> {
    if lock(&state.bench_out).is_none() {
        return Ok(None);
    }
    let s = state.session()?;
    let mut out = Vec::new();
    for c in s.clip_statuses().map_err(|e| e.to_string())? {
        let Some((_, info)) = s.preview_source(c.media).map_err(|e| e.to_string())? else { continue };
        let Some(v) = &info.video else { continue };
        out.push(BenchClip { media: c.media.0 as f64, name: c.name, width: v.width, height: v.height, duration_us: info.duration_us as f64, fps: v.fps.0.to_f64() });
    }
    Ok(Some(out))
}

/// Writes the benchmark report and exits.
#[tauri::command]
#[specta::specta]
async fn bench_report(app: tauri::AppHandle, state: State<'_, AppState>, report: String) -> Result<(), String> {
    let Some(out) = lock(&state.bench_out).clone() else { return Err("Not a benchmark run".into()) };
    std::fs::write(&out, report).map_err(|e| e.to_string())?;
    state.preview.clear();
    if let Some(s) = lock(&state.session).take() {
        close_session(s);
    }
    let _ = std::fs::remove_file(out.with_extension("clippro"));
    for ext in ["clippro-wal", "clippro-shm"] {
        let _ = std::fs::remove_file(out.with_extension(ext));
    }
    app.exit(0);
    Ok(())
}

pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            project_new,
            project_open,
            project_close,
            project_info,
            media_import,
            analysis_start,
            analysis_pause,
            analysis_resume,
            clips_list,
            presets_list,
            preset_settings,
            score,
            export_selects,
            timeline_review,
            timeline_export_back,
            sync_clips,
            sync_groups_list,
            sync_group_save,
            sync_group_delete,
            multicam_export,
            offload_start,
            offload_cancel,
            proxy_codecs,
            proxies_make,
            render_selects,
            job_cancel,
            diagnostics_log,
            diagnostics_save,
            diagnostics_open_folder,
            e2e_enabled,
            media_source,
            preview_shared,
            inspect_sample,
            audio_play,
            media_set_profile,
            clip_retry,
            system_info,
            settings_get,
            cache_info,
            audio_waveforms,
            audio_levels,
            audio_streams,
            audio_stream_set,
            audio_channel_gains,
            project_save_as,
            media_relink,
            media_relink_folder,
            cache_clear,
            settings_export,
            settings_import,
            watch_folders_get,
            snapshots_list,
            overrides_list,
            bins_list,
            bin_add,
            bin_update,
            bin_delete,
            clips_set_bin,
            templates_list,
            template_save,
            template_delete,
            override_add,
            override_remove,
            annotations_list,
            annotation_set,
            snapshot_save,
            snapshot_delete,
            tuning_save,
            tuning_load,
            watch_folders_set,
            submetrics_list,
            preset_save,
            preset_delete,
            preset_rename,
            preset_import,
            preset_export,
            camera_matches,
            settings_set,
            open_settings,
            clip_bump,
            clip_link_proxy,
            reveal_in_explorer,
            preview_colour,
            lut_read,
            audio_stop,
            audio_clock,
            bench_config,
            bench_report,
            startup_project,
            data::data_info,
            data::data_dir_set,
            ai::ai_models,
            ai::ai_model_download,
            ai::ai_download_cancel,
            ai::ai_model_delete,
            ai::ai_run,
            ai::ai_cancel,
            ai::ai_running,
            ai::ai_clip_info,
            ai::people_list,
            ai::person_rename,
            ai::person_merge,
            ai::transcript_get,
            ai::ai_search,
            ai::transcript_export,
            ai::ai_clear,
        ])
        .events(collect_events![AnalysisEvent, settings::AppSettings, MediaAdded, OffloadProgress, JobProgress, ai::AiProgress])
}

pub const BINDINGS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/src/bindings.ts");

pub fn export_bindings(path: &str) -> Result<(), String> {
    specta_builder()
        .export(specta_typescript::Typescript::default().header("// Generated by tauri-specta from app/src-tauri. Do not edit.\n// @ts-nocheck"), path)
        .map_err(|e| e.to_string())
}

fn apply_material(window: &tauri::WebviewWindow) -> bool {
    #[cfg(target_os = "windows")]
    {
        if window_vibrancy::apply_mica(window, None).is_ok() {
            return true;
        }
        if window_vibrancy::apply_acrylic(window, Some((30, 30, 30, 160))).is_ok() {
            return true;
        }
    }
    let _ = window;
    false
}

fn pump_events(app: tauri::AppHandle) {
    let mut tick = 0u32;
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let Some(state) = app.try_state::<AppState>() else { continue };
        let Some(s) = lock(&state.session).clone() else { continue };
        // Every 2 s: once analysis is idle, the AI queue picks up clips that need it.
        tick = tick.wrapping_add(1);
        if tick.is_multiple_of(20) && s.scheduler().is_idle() && lock(&state.ai).is_none() && lock(&state.bench_out).is_none() {
            let _ = ai::start_queue(&app, &state);
        }
        for e in s.poll_events() {
            use clippro_analysis::jobs::Event as E;
            let ev = match e {
                E::Progress { job, done, total } => AnalysisEvent { kind: "progress".into(), job: Some(job as f64), done: Some(done as u32), total: Some(total as u32), error: None },
                E::Finished { job } => AnalysisEvent { kind: "finished".into(), job: Some(job as f64), done: None, total: None, error: None },
                E::Failed { job, error } => {
                    diag::log("WARN", &format!("analysis job {job} failed: {error}"));
                    AnalysisEvent { kind: "failed".into(), job: Some(job as f64), done: None, total: None, error: Some(error) }
                }
                E::Paused => AnalysisEvent { kind: "paused".into(), job: None, done: None, total: None, error: None },
                E::Resumed => AnalysisEvent { kind: "resumed".into(), job: None, done: None, total: None, error: None },
            };
            let _ = ev.emit(&app);
        }
        drop(s);
    });
}

pub fn run(bench: Option<Bench>) {
    let builder = specta_builder();
    #[cfg(debug_assertions)]
    if let Err(e) = export_bindings(BINDINGS_PATH) {
        eprintln!("couldn't export TypeScript bindings: {e}");
    }
    let invoke = builder.invoke_handler();
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(invoke)
        .register_asynchronous_uri_scheme_protocol("clippro", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(frame_response(&app, &request));
                // Playback: render the next frame while this one travels to the page.
                if let Some(r) = preview::parse_frame_uri(request.uri().path(), request.uri().query()) {
                    if let (Some(rate), Ok(s)) = (r.rate, app.state::<AppState>().session()) {
                        let pts = r.us; // close enough: the ready frame is matched within one step
                        app.state::<AppState>().preview.prefetch(&s, r.kind, r.media, pts, r.width, rate);
                    }
                }
            });
        })
        .setup(move |app| {
            builder.mount_events(app);
            *lock(&app.state::<AppState>().app) = Some(app.handle().clone());
            // Self-test runs (bench, pdf-check) leave the user's settings alone.
            if let Ok(dir) = app.path().app_config_dir() {
                *lock(&app.state::<AppState>().presets_dir) = Some(dir.join("presets"));
            }
            if let (true, Ok(dir)) = (bench.is_none(), app.path().app_log_dir()) {
                diag::init(dir);
            }
            if e2e_mode() {
                // End-to-end runs (P5.7) leave the user's settings alone and skip the tour.
                *lock(&app.state::<AppState>().settings) = settings::AppSettings { tour_done: true, reopen_last: false, ..Default::default() };
            } else if let (true, Ok(dir)) = (bench.is_none(), app.path().app_config_dir()) {
                let p = settings::settings_path(dir);
                let state = app.state::<AppState>();
                *lock(&state.settings) = settings::AppSettings::load(&p);
                *lock(&state.settings_path) = Some(p);
                // The user's Data folder (models and measurement cache), before any project opens.
                let data_dir = lock(&state.settings).data_dir.clone();
                data::apply(data_dir.as_deref());
                data::finish_pending(&state);
            }
            if let Some(b) = bench.as_ref().filter(|b| b.pdf) {
                let (out, clips, app) = (b.out.clone(), b.clips.clone(), app.handle().clone());
                tauri::async_runtime::spawn(async move {
                    let code = match pdf_check(&app, &out, &clips).await {
                        Ok(n) => {
                            println!("pdf-check: {n} select(s) -> {}", out.display());
                            0
                        }
                        Err(e) => {
                            eprintln!("pdf-check failed: {e}");
                            1
                        }
                    };
                    app.exit(code);
                });
                return Ok(());
            }
            if let Some(b) = bench {
                let proj = b.out.with_extension("clippro");
                let _ = std::fs::remove_file(&proj);
                let s = Session::create(&proj, "bench", SessionOptions { cache_path: None, ..settings::AppSettings::default().session_options() })?;
                s.import(&b.clips)?;
                let state = app.state::<AppState>();
                replace_session(&state, Arc::new(s), proj);
                *lock(&state.bench_out) = Some(b.out);
                pump_events(app.handle().clone());
                return Ok(()); // the window stays hidden
            }
            if let Some(w) = app.get_webview_window("main") {
                let material = apply_material(&w);
                let script = format!("document.body.classList.toggle('has-material', {material});");
                let _ = w.eval(&script);
                let _ = w.show();
            }
            pump_events(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(state) = window.try_state::<AppState>() {
                    if let Some(s) = lock(&state.session).take() {
                        close_session(s);
                    }
                }
            }
        })
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("ClipPro failed to start: {e}");
    }
}
