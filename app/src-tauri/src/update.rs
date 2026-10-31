//! Auto-update (SPEC OTH-4: signed, user-approved). The app asks GitHub for the latest published
//! release's `latest.json`, verifies the installer against the public key in tauri.conf.json, and
//! installs only when the user says so. Drafts and pre-releases are never offered.

use crate::{lock, AppState};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_plugin_updater::UpdaterExt;
use tauri_specta::Event;

/// Also broadcast as an event, so a check from the Settings window shows the main window's bar.
#[derive(Serialize, Deserialize, Type, Clone, Debug, Event)]
pub struct UpdateInfo {
    pub version: String,
    pub current: String,
    /// Release notes (Markdown).
    pub notes: String,
    /// Publication date, ISO 8601.
    pub date: Option<String>,
}

/// Download progress of an update being installed.
#[derive(Serialize, Deserialize, Type, Clone, Debug, Event)]
pub struct UpdateProgress {
    /// downloading | installing | failed
    pub kind: String,
    pub downloaded: f64,
    pub total: Option<f64>,
    pub message: Option<String>,
}

/// The update endpoint: `CLIPPRO_UPDATE_ENDPOINT` (testing against a local server) or the one in
/// tauri.conf.json.
fn updater(app: &tauri::AppHandle, endpoint: Option<&str>) -> Result<tauri_plugin_updater::Updater, String> {
    let mut b = app.updater_builder();
    let over = endpoint.map(str::to_string).or_else(|| std::env::var("CLIPPRO_UPDATE_ENDPOINT").ok()).filter(|e| !e.is_empty());
    if let Some(e) = over {
        let url = tauri::Url::parse(&e).map_err(|e| e.to_string())?;
        b = b.endpoints(vec![url]).map_err(|e| e.to_string())?;
    }
    b.build().map_err(|e| e.to_string())
}

fn info(u: &tauri_plugin_updater::Update) -> UpdateInfo {
    UpdateInfo {
        version: u.version.clone(),
        current: u.current_version.clone(),
        notes: u.body.clone().unwrap_or_default(),
        date: u.date.map(|d| d.to_string()),
    }
}

/// Asks for a newer published release. `None` when ClipPro is up to date.
#[tauri::command]
#[specta::specta]
pub async fn update_check(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<Option<UpdateInfo>, String> {
    let u = updater(&app, None)?.check().await.map_err(|e| e.to_string())?;
    let out = u.as_ref().map(info);
    *lock(&state.pending_update) = u;
    if let Some(i) = &out {
        let _ = i.emit(&app);
    }
    Ok(out)
}

/// Downloads, verifies and installs the update found by `update_check`. The installer closes
/// ClipPro, updates it and starts it again.
#[tauri::command]
#[specta::specta]
pub async fn update_install(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let Some(u) = lock(&state.pending_update).clone() else { return Err("No update to install. Check for updates first.".into()) };
    // Close the project first so nothing is lost when the installer replaces the app.
    if let Some(s) = lock(&state.session).take() {
        crate::close_session(s);
    }
    let mut downloaded = 0f64;
    let mut last = std::time::Instant::now();
    let (a1, a2) = (app.clone(), app.clone());
    let r = u
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as f64;
                if last.elapsed().as_millis() >= 150 {
                    last = std::time::Instant::now();
                    let _ = UpdateProgress { kind: "downloading".into(), downloaded, total: total.map(|t| t as f64), message: None }.emit(&a1);
                }
            },
            move || {
                let _ = UpdateProgress { kind: "installing".into(), downloaded: 0.0, total: None, message: None }.emit(&a2);
            },
        )
        .await;
    if let Err(e) = r {
        crate::diag::log("WARN", &format!("update failed: {e}"));
        let _ = UpdateProgress { kind: "failed".into(), downloaded: 0.0, total: None, message: Some(e.to_string()) }.emit(&app);
        return Err(e.to_string());
    }
    crate::diag::log("INFO", &format!("updated to {}", u.version));
    // On Windows the installer has already taken over; elsewhere, start the new version.
    app.restart();
}

/// Opens an https link (release notes, the releases page) in the default browser.
#[tauri::command]
#[specta::specta]
pub fn open_url(url: String) -> Result<(), String> {
    let u = tauri::Url::parse(&url).map_err(|e| e.to_string())?;
    if u.scheme() != "https" {
        return Err("Only https links open from ClipPro.".into());
    }
    std::process::Command::new("explorer.exe").arg(u.as_str()).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// `ClipPro --update-selftest <endpoint>`: checks, downloads and verifies the update offered by
/// `endpoint` without installing it, prints the result and exits (release testing).
pub async fn selftest(app: tauri::AppHandle, endpoint: String) -> i32 {
    let run = async {
        let u = updater(&app, Some(&endpoint))?.check().await.map_err(|e| e.to_string())?;
        let Some(u) = u else { return Ok::<String, String>("up to date".into()) };
        let bytes = u.download(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
        Ok(format!("update {} -> {}: {} bytes downloaded, signature verified", u.current_version, u.version, bytes.len()))
    };
    match run.await {
        Ok(m) => {
            println!("update-selftest: {m}");
            0
        }
        Err(e) => {
            eprintln!("update-selftest failed: {e}");
            1
        }
    }
}
