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

#[derive(Default)]
pub struct AppState {
    session: Mutex<Option<Arc<Session>>>,
    path: Mutex<Option<PathBuf>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl AppState {
    fn session(&self) -> Result<Arc<Session>, String> {
        lock(&self.session).clone().ok_or_else(|| "No project is open.".to_string())
    }
}

fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> impl std::future::Future<Output = Result<T, String>> {
    async move { tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())? }
}

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
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct PresetInfo {
    pub name: String,
    pub description: String,
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

// ---------- Commands ----------

fn options() -> SessionOptions {
    SessionOptions::default()
}

#[tauri::command]
#[specta::specta]
async fn project_new(state: State<'_, AppState>, path: String, name: String) -> Result<ProjectInfo, String> {
    let p = PathBuf::from(&path);
    let s = blocking(move || Session::create(&p, &name, options()).map_err(|e| e.to_string())).await?;
    let name = lock(&s.db).name().map_err(|e| e.to_string())?;
    replace_session(&state, Arc::new(s), PathBuf::from(&path));
    Ok(ProjectInfo { path, name, recovered: false })
}

#[tauri::command]
#[specta::specta]
async fn project_open(state: State<'_, AppState>, path: String) -> Result<ProjectInfo, String> {
    let p = PathBuf::from(&path);
    let (s, rep) = blocking(move || Session::open(&p, options()).map_err(|e| e.to_string())).await?;
    let name = lock(&s.db).name().map_err(|e| e.to_string())?;
    replace_session(&state, Arc::new(s), PathBuf::from(&path));
    Ok(ProjectInfo { path, name, recovered: rep.recovered })
}

fn replace_session(state: &AppState, s: Arc<Session>, path: PathBuf) {
    let old = lock(&state.session).replace(s);
    *lock(&state.path) = Some(path);
    if let Some(old) = old {
        close_session(old);
    }
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
    let s = lock(&state.session).take();
    *lock(&state.path) = None;
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
            })
            .collect())
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn presets_list() -> Result<Vec<PresetInfo>, String> {
    Ok(builtins().into_iter().map(|p| PresetInfo { name: p.preset.name, description: p.preset.description }).collect())
}

#[tauri::command]
#[specta::specta]
async fn preset_settings(name: String) -> Result<Settings, String> {
    let p = clippro_score::builtin(&name).ok_or_else(|| format!("Unknown preset “{name}”."))?;
    Settings::from_preset(&p).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
async fn score(state: State<'_, AppState>, settings: Settings, sort: SortOrder) -> Result<ScoreResult, String> {
    let s = state.session()?;
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
        Ok(ScoreResult { clips, selects: sel.sorted(sort), summary: sel.summary.clone() })
    })
    .await
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct ExportRequest {
    /// xmeml | fcpxml | otio | edl | ale | avid-markers | csv | json
    pub format: String,
    pub path: String,
    pub name: String,
    /// selects | with_rejects
    pub layout: String,
    pub handles: bool,
    pub gap_s: f64,
    pub with_audio: bool,
    pub markers: bool,
    pub marker_name: String,
    pub marker_comment: String,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct ExportResult {
    pub path: String,
    pub selects: u32,
}

#[tauri::command]
#[specta::specta]
async fn export_selects(state: State<'_, AppState>, settings: Settings, sort: SortOrder, request: ExportRequest) -> Result<ExportResult, String> {
    let s = state.session()?;
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
        let n = s.export_to(&settings, sort, &opts, format, &path).map_err(|e| e.to_string())?;
        Ok(ExportResult { path: request.path, selects: n as u32 })
    })
    .await
}

// ---------- Wiring ----------

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
        ])
        .events(collect_events![AnalysisEvent])
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
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let Some(state) = app.try_state::<AppState>() else { continue };
        let Some(s) = lock(&state.session).clone() else { continue };
        for e in s.poll_events() {
            use clippro_analysis::jobs::Event as E;
            let ev = match e {
                E::Progress { job, done, total } => AnalysisEvent { kind: "progress".into(), job: Some(job as f64), done: Some(done as u32), total: Some(total as u32), error: None },
                E::Finished { job } => AnalysisEvent { kind: "finished".into(), job: Some(job as f64), done: None, total: None, error: None },
                E::Failed { job, error } => AnalysisEvent { kind: "failed".into(), job: Some(job as f64), done: None, total: None, error: Some(error) },
                E::Paused => AnalysisEvent { kind: "paused".into(), job: None, done: None, total: None, error: None },
                E::Resumed => AnalysisEvent { kind: "resumed".into(), job: None, done: None, total: None, error: None },
            };
            let _ = ev.emit(&app);
        }
        drop(s);
    });
}

pub fn run() {
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
        .setup(move |app| {
            builder.mount_events(app);
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
