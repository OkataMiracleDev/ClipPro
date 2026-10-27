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

pub mod audio_out;
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
    /// Log/colour profile used for preview and log-aware exposure.
    pub profile: clippro_types::ColourProfile,
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

// ---------- Commands ----------

/// Session options from the app settings (analysis and performance defaults).
fn options(state: &AppState) -> SessionOptions {
    lock(&state.settings).session_options()
}

/// Remembers the open project for "reopen at start-up".
fn remember_project(state: &AppState, path: &std::path::Path) {
    let mut st = lock(&state.settings);
    st.last_project = Some(path.to_string_lossy().into_owned());
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
    /// xmeml | fcpxml | otio | edl | ale | avid-markers | csv | json | html | pdf
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
        let n = s.export_to(&settings, sort, &opts, format, &path).map_err(|e| e.to_string())?;
        Ok(ExportResult { path: request.path, selects: n as u32 })
    })
    .await
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

#[tauri::command]
#[specta::specta]
async fn settings_get(state: State<'_, AppState>) -> Result<settings::AppSettings, String> {
    Ok(lock(&state.settings).clone())
}

/// Saves the app settings and tells every window (`AppSettings` event).
#[tauri::command]
#[specta::specta]
async fn settings_set(app: tauri::AppHandle, state: State<'_, AppState>, settings: settings::AppSettings) -> Result<settings::AppSettings, String> {
    let s = settings.sanitised();
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
    state.audio.play(path, info.start_us, us.max(0.0) as i64, 1.0)?;
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
            media_source,
            preview_shared,
            inspect_sample,
            audio_play,
            media_set_profile,
            clip_retry,
            system_info,
            settings_get,
            watch_folders_get,
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
        ])
        .events(collect_events![AnalysisEvent, settings::AppSettings, MediaAdded])
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
            if let (true, Ok(dir)) = (bench.is_none(), app.path().app_config_dir()) {
                let p = settings::settings_path(dir);
                let state = app.state::<AppState>();
                *lock(&state.settings) = settings::AppSettings::load(&p);
                *lock(&state.settings_path) = Some(p);
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
