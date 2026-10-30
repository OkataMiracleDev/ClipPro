//! Local AI (SPEC §18, PLAN P12.7): the model manager, the AI queue that runs after analysis,
//! People, transcripts and search. Nothing is downloaded or run until the user turns it on in
//! Settings → AI Models.

use crate::{lock, settings::AppSettings, AppState};
use clippro_core::ai::{AiOptions, AiStage};
use clippro_types::ClipId;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::State;
use tauri_specta::Event;

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct AiModelRow {
    pub id: String,
    /// faces | clip | transcripts | speakers
    pub module: String,
    pub name: String,
    pub size: f64,
    pub licence: String,
    pub source: String,
    pub installed: bool,
    pub downloading: bool,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct AiModels {
    pub models: Vec<AiModelRow>,
    pub dir: String,
    pub bytes: f64,
}

/// Model downloads and the AI queue. Progress is sent at most every 150 ms.
#[derive(Serialize, Deserialize, Type, Clone, Debug, Event)]
pub struct AiProgress {
    /// download | downloaded | download_failed | progress | finished | failed | cancelled
    pub kind: String,
    pub model: Option<String>,
    pub clip: Option<f64>,
    /// vision | transcript | speakers
    pub stage: Option<String>,
    pub fraction: f32,
    pub index: u32,
    pub count: u32,
    pub message: Option<String>,
}

impl AiProgress {
    fn new(kind: &str) -> AiProgress {
        AiProgress { kind: kind.into(), model: None, clip: None, stage: None, fraction: 0.0, index: 0, count: 0, message: None }
    }
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct ClipAiRow {
    pub clip: f64,
    pub people: Vec<f64>,
    /// wide | medium | close_up | extreme_close_up | insert | aerial | over_the_shoulder
    pub shot: Option<String>,
    pub transcript: bool,
    pub language: Option<String>,
    pub speakers: u32,
    pub faces_done: bool,
    pub clip_done: bool,
    /// Outdoor and night probabilities (AI-4).
    pub outdoor: Option<f32>,
    pub night: Option<f32>,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct PersonRow {
    pub id: f64,
    pub name: String,
    pub clips: Vec<f64>,
    pub cover_clip: Option<f64>,
    pub cover_us: Option<f64>,
    /// x, y, w, h as fractions of the frame.
    pub cover_box: Option<Vec<f32>>,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct WordRow {
    pub start_us: f64,
    pub end_us: f64,
    pub text: String,
    pub prob: f32,
    pub filler: bool,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct LineRow {
    pub start_us: f64,
    pub end_us: f64,
    pub text: String,
    pub speaker: Option<u32>,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct TranscriptRow {
    pub language: String,
    pub words: Vec<WordRow>,
    pub lines: Vec<LineRow>,
    pub flubs: Vec<(f64, f64)>,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug)]
pub struct SearchRow {
    pub clip: f64,
    pub start_us: f64,
    pub end_us: f64,
    pub text: String,
    pub score: f32,
}

/// What the AI queue runs with these settings (None when AI is off or not consented to).
pub fn ai_options(s: &AppSettings) -> Option<AiOptions> {
    let o = AiOptions {
        faces: s.ai_faces,
        clip: s.ai_shots,
        transcripts: s.ai_transcripts,
        speakers: s.ai_speakers,
        whisper: s.whisper_model.clone(),
        language: Some(s.ai_language.clone()).filter(|l| !l.is_empty()),
        deterministic: s.ai_deterministic,
    };
    (s.ai_consent && o.any()).then_some(o)
}

/// Modules of `o` whose models aren't installed are dropped.
fn runnable(mut o: AiOptions) -> AiOptions {
    let has = |ids: &[&str]| ids.iter().all(|i| clippro_ai::models::installed_path(i).is_some());
    o.faces &= has(&["yunet-2023mar", "sface-2021dec"]);
    o.clip &= has(&["clip-b32-vision", "clip-b32-text", "clip-b32-tokenizer"]);
    o.transcripts &= has(&[o.whisper.as_str()]);
    o.speakers &= has(&["ecapa-voxceleb-512"]);
    o
}

#[tauri::command]
#[specta::specta]
pub fn ai_models(state: State<'_, AppState>) -> AiModels {
    let downloading = lock(&state.ai_downloads).keys().cloned().collect::<Vec<_>>();
    AiModels {
        models: clippro_ai::models::manifest()
            .into_iter()
            .map(|m| AiModelRow {
                installed: clippro_ai::models::installed(&m),
                downloading: downloading.contains(&m.id),
                id: m.id,
                module: m.module,
                name: m.name,
                size: m.size as f64,
                licence: m.licence,
                source: m.source,
            })
            .collect(),
        dir: clippro_ai::models::models_dir().display().to_string(),
        bytes: clippro_ai::models::disk_use() as f64,
    }
}

/// Downloads models one after another in the background (`AiProgress` download events).
#[tauri::command]
#[specta::specta]
pub fn ai_model_download(app: tauri::AppHandle, state: State<'_, AppState>, ids: Vec<String>) -> Result<(), String> {
    if !lock(&state.settings).ai_consent {
        return Err("Turn on local AI first.".into());
    }
    let models: Vec<_> = ids.iter().filter_map(|i| clippro_ai::models::model(i)).collect();
    let mut todo = Vec::new();
    {
        let mut d = lock(&state.ai_downloads);
        for m in models {
            if !d.contains_key(&m.id) {
                let c = Arc::new(AtomicBool::new(false));
                d.insert(m.id.clone(), c.clone());
                todo.push((m, c));
            }
        }
    }
    std::thread::spawn(move || {
        for (m, cancel) in todo {
            let mut last = std::time::Instant::now();
            let r = clippro_ai::models::download(&m, &cancel, |done, total| {
                if last.elapsed().as_millis() >= 150 {
                    last = std::time::Instant::now();
                    let _ = AiProgress { model: Some(m.id.clone()), fraction: if total > 0 { done as f32 / total as f32 } else { 0.0 }, ..AiProgress::new("download") }.emit(&app);
                }
            });
            if let Some(st) = tauri::Manager::try_state::<AppState>(&app) {
                lock(&st.ai_downloads).remove(&m.id);
            }
            let ev = match r {
                Ok(_) => AiProgress { model: Some(m.id.clone()), fraction: 1.0, ..AiProgress::new("downloaded") },
                Err(e) => {
                    crate::diag::log("WARN", &format!("model {} download failed: {e}", m.id));
                    AiProgress { model: Some(m.id.clone()), message: Some(e), ..AiProgress::new("download_failed") }
                }
            };
            let _ = ev.emit(&app);
        }
    });
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn ai_download_cancel(state: State<'_, AppState>, id: String) {
    if let Some(c) = lock(&state.ai_downloads).get(&id) {
        c.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
#[specta::specta]
pub fn ai_model_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    if lock(&state.ai).is_some() {
        return Err("Wait for the AI pass to finish, or stop it first.".into());
    }
    let m = clippro_ai::models::model(&id).ok_or("Unknown model")?;
    clippro_ai::models::delete(&m).map_err(|e| e.to_string())
}

/// Starts the AI queue over every clip that still needs a module; returns how many clips.
#[tauri::command]
#[specta::specta]
pub fn ai_run(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<u32, String> {
    lock(&state.ai_failed).clear();
    start_queue(&app, &state)
}

pub(crate) fn start_queue(app: &tauri::AppHandle, state: &AppState) -> Result<u32, String> {
    let s = state.session()?;
    let Some(opts) = ai_options(&lock(&state.settings)).map(runnable).filter(AiOptions::any) else { return Ok(0) };
    let mut running = lock(&state.ai);
    if running.is_some() {
        return Ok(0);
    }
    let failed = lock(&state.ai_failed).clone();
    let clips: Vec<ClipId> = s.ai_pending(&opts).map_err(|e| e.to_string())?.into_iter().filter(|c| !failed.contains(&c.0)).collect();
    if clips.is_empty() {
        return Ok(0);
    }
    let cancel = Arc::new(AtomicBool::new(false));
    *running = Some(cancel.clone());
    drop(running);
    let n = clips.len() as u32;
    let app = app.clone();
    std::thread::Builder::new()
        .name("ai-queue".into())
        .spawn(move || {
            lower_priority();
            let mut result = Ok(());
            let mut last = std::time::Instant::now();
            for (i, c) in clips.iter().enumerate() {
                let r = s.run_ai(*c, &opts, &cancel, |stage, f| {
                    if last.elapsed().as_millis() >= 150 || f >= 1.0 {
                        last = std::time::Instant::now();
                        let stage = match stage {
                            AiStage::Vision => "vision",
                            AiStage::Transcript => "transcript",
                            AiStage::Speakers => "speakers",
                        };
                        let _ = AiProgress { clip: Some(c.0 as f64), stage: Some(stage.into()), fraction: f, index: i as u32, count: n, ..AiProgress::new("progress") }.emit(&app);
                    }
                });
                if cancel.load(Ordering::Relaxed) {
                    result = Err("cancelled".to_string());
                    break;
                }
                if let Err(e) = r {
                    // One bad clip doesn't stop the queue; it's retried on the next run.
                    crate::diag::log("WARN", &format!("AI pass on clip {} failed: {e}", c.0));
                    if let Some(st) = tauri::Manager::try_state::<AppState>(&app) {
                        lock(&st.ai_failed).insert(c.0);
                    }
                    result = Err(e.to_string());
                }
            }
            if let Some(st) = tauri::Manager::try_state::<AppState>(&app) {
                *lock(&st.ai) = None;
            }
            let ev = match result {
                Ok(()) => AiProgress { index: n, count: n, fraction: 1.0, ..AiProgress::new("finished") },
                Err(m) if m == "cancelled" => AiProgress { count: n, ..AiProgress::new("cancelled") },
                Err(m) => AiProgress { count: n, message: Some(m), ..AiProgress::new("failed") },
            };
            let _ = ev.emit(&app);
        })
        .map_err(|e| e.to_string())?;
    Ok(n)
}

/// The AI queue yields to analysis and playback.
fn lower_priority() {
    #[cfg(windows)]
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetCurrentThread() -> isize;
            fn SetThreadPriority(h: isize, p: i32) -> i32;
        }
        SetThreadPriority(GetCurrentThread(), -1); // THREAD_PRIORITY_BELOW_NORMAL
    }
}

#[tauri::command]
#[specta::specta]
pub fn ai_cancel(state: State<'_, AppState>) {
    if let Some(c) = lock(&state.ai).as_ref() {
        c.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
#[specta::specta]
pub fn ai_running(state: State<'_, AppState>) -> bool {
    lock(&state.ai).is_some()
}

#[tauri::command]
#[specta::specta]
pub async fn ai_clip_info(state: State<'_, AppState>) -> Result<Vec<ClipAiRow>, String> {
    let s = state.session()?;
    crate::blocking(move || {
        Ok(s.clip_ai()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|c| ClipAiRow {
                clip: c.clip.0 as f64,
                people: c.people.iter().map(|p| *p as f64).collect(),
                shot: c.shot.and_then(|t| serde_json::to_value(t).ok()).and_then(|v| v.as_str().map(str::to_string)),
                transcript: c.transcript,
                language: c.language,
                speakers: c.speakers as u32,
                faces_done: c.faces_done,
                clip_done: c.clip_done,
                outdoor: c.outdoor,
                night: c.night,
            })
            .collect())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn people_list(state: State<'_, AppState>) -> Result<Vec<PersonRow>, String> {
    let s = state.session()?;
    crate::blocking(move || {
        let statuses = s.clip_statuses().map_err(|e| e.to_string())?;
        Ok(s.people()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|p| {
                // Boxes were found on 640-wide frames: express them as fractions.
                let frac = p.cover.as_ref().and_then(|(c, _, b)| {
                    let st = statuses.iter().find(|x| x.clip == *c)?;
                    let src = s.preview_source(st.media).ok().flatten()?;
                    let v = src.1.video.as_ref()?;
                    // Same size rule as `VideoDecoder::to_preview` (even sizes, never upscaled).
                    let (w, h) = (v.width.max(1) as f64, v.height.max(1) as f64);
                    let scale = (640.0 / w).min(1.0);
                    let even = |x: f64| (((x * scale).round() as i32).max(2) & !1) as f32;
                    let (fw, fh) = (even(w), even(h));
                    Some(vec![b[0] / fw, b[1] / fh, b[2] / fw, b[3] / fh])
                });
                PersonRow {
                    id: p.id as f64,
                    name: p.name,
                    clips: p.clips.iter().map(|c| c.0 as f64).collect(),
                    cover_clip: p.cover.as_ref().map(|c| c.0 .0 as f64),
                    cover_us: p.cover.as_ref().map(|c| c.1 as f64),
                    cover_box: frac,
                }
            })
            .collect())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub fn person_rename(state: State<'_, AppState>, id: f64, name: String) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Names can't be empty.".into());
    }
    state.session()?.rename_person(id as i64, &name).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn person_merge(state: State<'_, AppState>, into: f64, from: f64) -> Result<(), String> {
    state.session()?.merge_people(into as i64, from as i64).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn transcript_get(state: State<'_, AppState>, clip: f64) -> Result<Option<TranscriptRow>, String> {
    let s = state.session()?;
    crate::blocking(move || {
        let id = ClipId(clip as i64);
        let Some(t) = s.transcript(id).map_err(|e| e.to_string())? else { return Ok(None) };
        let st = s.clip_statuses().map_err(|e| e.to_string())?;
        let turns = st.iter().find(|c| c.clip == id).and_then(|c| s.ai_data(c.media).ok()).map(|d| d.speakers.clone()).unwrap_or_default();
        let fillers = clippro_transcribe::fillers(&t);
        let speaker_at = |a: i64, b: i64| {
            turns.iter().filter(|x| x.end_us > a && x.start_us < b).max_by_key(|x| x.end_us.min(b) - x.start_us.max(a)).map(|x| x.speaker as u32)
        };
        Ok(Some(TranscriptRow {
            language: t.language.clone(),
            words: t.words.iter().enumerate().map(|(i, w)| WordRow { start_us: w.start_us as f64, end_us: w.end_us as f64, text: w.text.clone(), prob: w.prob, filler: fillers.contains(&i) }).collect(),
            lines: t.segments.iter().map(|g| LineRow { start_us: g.start_us as f64, end_us: g.end_us as f64, text: g.text.clone(), speaker: speaker_at(g.start_us, g.end_us) }).collect(),
            flubs: clippro_transcribe::flubs(&t).into_iter().map(|(a, b)| (a as f64, b as f64)).collect(),
        }))
    })
    .await
}

/// Transcript search (full text) or visual search (CLIP), best first.
#[tauri::command]
#[specta::specta]
pub async fn ai_search(state: State<'_, AppState>, query: String, visual: bool) -> Result<Vec<SearchRow>, String> {
    let s = state.session()?;
    let det = lock(&state.settings).ai_deterministic;
    crate::blocking(move || {
        let hits = if visual { s.search_visual(&query, det, 50) } else { s.search_transcripts(&query) }.map_err(|e| e.to_string())?;
        Ok(hits.into_iter().map(|h| SearchRow { clip: h.clip.0 as f64, start_us: h.start_us as f64, end_us: h.end_us as f64, text: h.text, score: h.score }).collect())
    })
    .await
}

/// Writes a clip's transcript (or the part within `start_us`–`end_us`) as SRT or WebVTT, by
/// the file extension.
#[tauri::command]
#[specta::specta]
pub fn transcript_export(state: State<'_, AppState>, clip: f64, path: String, start_us: Option<f64>, end_us: Option<f64>) -> Result<(), String> {
    let s = state.session()?;
    let t = s.transcript(ClipId(clip as i64)).map_err(|e| e.to_string())?.ok_or("This clip has no transcript yet.")?;
    let (a, b) = (start_us.map(|x| x as i64).unwrap_or(0), end_us.map(|x| x as i64).unwrap_or(i64::MAX));
    let text = if path.to_lowercase().ends_with(".vtt") { clippro_transcribe::vtt(&t, a, b) } else { clippro_transcribe::srt(&t, a, b) };
    std::fs::write(&path, text).map_err(|e| format!("{path}: {e}"))
}

/// Drops a clip's AI results so the next run redoes them.
#[tauri::command]
#[specta::specta]
pub fn ai_clear(state: State<'_, AppState>, clip: f64) -> Result<(), String> {
    state.session()?.clear_ai(ClipId(clip as i64)).map_err(|e| e.to_string())
}
