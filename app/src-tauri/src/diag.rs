//! Diagnostics (SPEC OTH-5, PLAN P14.5): a rotating log file, crash reports from a panic hook,
//! and a plain-text diagnostics report (system, settings without paths, recent log and crashes;
//! never media) for bug reports.

use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_LOG: u64 = 5 << 20;

struct Log {
    dir: PathBuf,
    file: Option<File>,
}

static LOG: Mutex<Option<Log>> = Mutex::new(None);

pub fn log_path(dir: &Path) -> PathBuf {
    dir.join("clippro.log")
}

/// `2026-10-30 09:15:02Z`.
pub fn stamp(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z", rem / 3600, rem / 60 % 60, rem % 60)
}

fn open(dir: &Path) -> Option<File> {
    std::fs::create_dir_all(dir).ok()?;
    let p = log_path(dir);
    // Rotate: keep one previous log.
    if std::fs::metadata(&p).map(|m| m.len() > MAX_LOG).unwrap_or(false) {
        let _ = std::fs::rename(&p, dir.join("clippro.1.log"));
    }
    OpenOptions::new().create(true).append(true).open(p).ok()
}

/// Starts logging into `dir` and installs the panic hook.
pub fn init(dir: PathBuf) {
    if let Ok(mut g) = LOG.lock() {
        let file = open(&dir);
        *g = Some(Log { dir: dir.clone(), file });
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let report = format!(
            "ClipPro {} crashed at {}\nthread: {thread}\n{info}\n\nbacktrace:\n{bt}\n",
            env!("CARGO_PKG_VERSION"),
            stamp(SystemTime::now())
        );
        log("PANIC", &format!("{info}"));
        let name = format!("crash-{}.txt", SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
        let _ = std::fs::write(dir.join(name), report);
        previous(info);
    }));
    log("INFO", &format!("ClipPro {} started on {} ({} threads)", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)));
}

/// Appends a line to the log (no-op before `init`).
pub fn log(level: &str, msg: &str) {
    let Ok(mut g) = LOG.lock() else { return };
    let Some(l) = g.as_mut() else { return };
    if l.file.as_ref().and_then(|f| f.metadata().ok()).is_some_and(|m| m.len() > MAX_LOG) {
        l.file = open(&l.dir);
    }
    if let Some(f) = l.file.as_mut() {
        let _ = writeln!(f, "{} {level:<5} {}", stamp(SystemTime::now()), msg.replace('\n', " | "));
    }
}

pub fn dir() -> Option<PathBuf> {
    LOG.lock().ok().and_then(|g| g.as_ref().map(|l| l.dir.clone()))
}

/// The last `n` lines of the current log (and the previous one, if the current is short).
pub fn tail(dir: &Path, n: usize) -> Vec<String> {
    let read = |p: PathBuf| std::fs::read_to_string(p).unwrap_or_default();
    let mut lines: Vec<String> = read(dir.join("clippro.1.log")).lines().chain(read(log_path(dir)).lines()).map(str::to_string).collect();
    let skip = lines.len().saturating_sub(n);
    lines.drain(..skip);
    lines
}

/// Crash reports, newest first.
pub fn crashes(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("crash-"))).collect())
        .unwrap_or_default();
    v.sort();
    v.reverse();
    v
}

/// Replaces file paths with `<path>` (reports carry no file names). A path starts with a drive
/// (`D:\`), a share (`\\`) or a home folder and runs to the next quote, bracket, comma, `|`
/// or line end, so names with spaces go too.
pub fn scrub(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        let b = rest.as_bytes();
        let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/');
        let starts = drive || rest.starts_with("\\\\") || rest.starts_with("/Users/") || rest.starts_with("/home/");
        let at_token = s[..i].chars().last().is_none_or(|c| !c.is_alphanumeric());
        if starts && at_token {
            let end = rest.find(|c: char| matches!(c, '"' | '\'' | '\n' | ')' | ',' | '|')).unwrap_or(rest.len());
            out.push_str("<path>");
            i += end;
        } else if let Some(c) = rest.chars().next() {
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// The plain-text diagnostics report.
pub fn report(dir: &Path, system: &[(&str, String)], settings_json: &str) -> String {
    let mut r = String::new();
    let _ = writeln!(r, "ClipPro diagnostics report\nGenerated {}\n", stamp(SystemTime::now()));
    let _ = writeln!(r, "== System");
    for (k, v) in system {
        let _ = writeln!(r, "{k}: {v}");
    }
    let _ = writeln!(r, "\n== Settings (paths removed)\n{}", scrub(settings_json));
    let c = crashes(dir);
    let _ = writeln!(r, "\n== Crash reports ({})", c.len());
    for p in c.iter().take(3) {
        let _ = writeln!(r, "-- {}\n{}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), scrub(&std::fs::read_to_string(p).unwrap_or_default()));
    }
    let _ = writeln!(r, "\n== Log (last 2000 lines, paths removed)");
    for l in tail(dir, 2000) {
        let _ = writeln!(r, "{}", scrub(&l));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_and_scrubbing() {
        assert_eq!(stamp(UNIX_EPOCH + std::time::Duration::from_secs(1_579_166_102)), "2020-01-16 09:15:02Z");
        assert_eq!(scrub("opened D:\\Shoot\\Day 1\\a.mov"), "opened <path>");
        assert_eq!(scrub("{\"recent\":[\"D:/Projects/My job.clippro\",\"\\\\nas\\x.clippro\"],\"theme\":\"dark\"}"), "{\"recent\":[\"<path>\",\"<path>\"],\"theme\":\"dark\"}");
        assert_eq!(scrub("failed: /Users/me/clip.mov (no video) | AB:C"), "failed: <path>) | AB:C");
    }

    #[test]
    fn report_has_sections_and_no_paths() {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-diag-test");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(log_path(&d), "2026-10-30 09:00:00Z INFO  opened E:\\Footage\\A001.mov\n").unwrap();
        std::fs::write(d.join("crash-1.txt"), "ClipPro crashed\npanicked at src\\lib.rs:1\n").unwrap();
        let r = report(&d, &[("Version", "0.1.0".into())], "{\"theme\":\"dark\"}");
        assert!(r.contains("== System\nVersion: 0.1.0"));
        assert!(r.contains("== Crash reports (1)"));
        assert!(r.contains("opened <path>"));
        assert!(!r.contains("E:\\Footage"));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
