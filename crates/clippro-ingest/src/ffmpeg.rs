//! Runs the bundled LGPL `ffmpeg.exe` for proxies and rendered selects (SPEC §19), with
//! progress from `-progress pipe:1` and cancellation.

use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// `CLIPPRO_FFMPEG`, else `ffmpeg.exe` beside the running program, else the first on PATH.
pub fn ffmpeg_exe() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("CLIPPRO_FFMPEG").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    if let Some(p) = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(name))).filter(|p| p.is_file()) {
        return Some(p);
    }
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(name)).find(|p| p.is_file())
}

fn command() -> Result<Command, String> {
    let exe = ffmpeg_exe().ok_or("ffmpeg.exe was not found next to ClipPro.")?;
    let mut c = Command::new(exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c.stdin(Stdio::null());
    Ok(c)
}

/// Encoders that actually work here: compiled in and, for hardware ones, able to open (a
/// one-frame test encode). Cached per process.
pub fn encoder_works(name: &str) -> bool {
    static CACHE: OnceLock<Mutex<Vec<(String, bool)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(Vec::new()));
    if let Some(v) = cache.lock().ok().and_then(|c| c.iter().find(|(n, _)| n == name).map(|x| x.1)) {
        return v;
    }
    let ok = command()
        .ok()
        .and_then(|mut c| {
            c.args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=c=black:s=320x180:d=0.2:r=25", "-frames:v", "2", "-pix_fmt"])
                .arg(if name.starts_with("prores") || name == "dnxhd" { "yuv422p10le" } else { "nv12" })
                .args(["-c:v", name])
                .args(if name == "dnxhd" { vec!["-profile:v", "dnxhr_lb"] } else { vec![] })
                .args(["-f", "null", "-"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .ok()
        })
        .is_some_and(|s| s.success());
    if let Ok(mut c) = cache.lock() {
        c.push((name.to_string(), ok));
    }
    ok
}

/// Runs ffmpeg with `args` (inputs and outputs), reporting progress 0–1 against
/// `duration_us`. On cancel the process is stopped and `Err("cancelled")` returned.
pub fn run(args: &[String], duration_us: i64, cancel: &AtomicBool, mut progress: impl FnMut(f32)) -> Result<(), String> {
    let mut c = command()?;
    c.args(["-hide_banner", "-nostats", "-loglevel", "error", "-y", "-progress", "pipe:1"]).args(args);
    let mut child = c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("ffmpeg: {e}"))?;
    let mut stderr = child.stderr.take();
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = stderr.as_mut() {
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines() {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                return Err("cancelled".into());
            }
            let Ok(line) = line else { break };
            if let Some(v) = line.strip_prefix("out_time_us=").and_then(|v| v.trim().parse::<i64>().ok()) {
                if duration_us > 0 {
                    progress((v as f64 / duration_us as f64).clamp(0.0, 1.0) as f32);
                }
            }
        }
    }
    let status = child.wait().map_err(|e| format!("ffmpeg: {e}"))?;
    let err = err_thread.join().unwrap_or_default();
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    if !status.success() {
        let msg = err.lines().filter(|l| !l.trim().is_empty()).last().unwrap_or("ffmpeg failed").to_string();
        return Err(msg);
    }
    progress(1.0);
    Ok(())
}
