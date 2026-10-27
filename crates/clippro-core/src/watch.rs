//! Watch folders (SPEC §19, P9.5). New media in a watched folder is handed over once it is
//! stable: size and mtime unchanged for `stable_for` and the file opens for reading. Growing
//! MXF/MOV recordings are therefore picked up only when the camera or copy has finished.
//!
//! `notify` (ReadDirectoryChangesW) reports changes; a periodic rescan catches anything it
//! missed (network shares, buffer overflows). Each path is handed over at most once per runner.

use crate::import::is_media;
use notify::{RecursiveMode, Watcher as _};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WatchFolder {
    pub path: String,
    #[serde(default)]
    pub subfolders: bool,
    /// Lower-case extensions without the dot; empty = every media type ClipPro imports.
    #[serde(default)]
    pub extensions: Vec<String>,
}

impl WatchFolder {
    pub fn accepts(&self, p: &Path) -> bool {
        if !is_media(p) {
            return false;
        }
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.starts_with('.') || name.starts_with('~') {
            return false;
        }
        let root = Path::new(&self.path);
        let Ok(rel) = p.strip_prefix(root) else { return false };
        if !self.subfolders && rel.components().count() > 1 {
            return false;
        }
        self.extensions.is_empty() || p.extension().is_some_and(|e| self.extensions.iter().any(|x| x.eq_ignore_ascii_case(&e.to_string_lossy())))
    }
}

#[derive(Clone, Copy)]
struct Seen {
    size: u64,
    mtime: Option<SystemTime>,
    since: Instant,
}

fn snapshot(p: &Path) -> Option<(u64, Option<SystemTime>)> {
    let m = std::fs::metadata(p).ok()?;
    m.is_file().then(|| (m.len(), m.modified().ok()))
}

/// Can be opened for reading while others may still hold it (read-shared).
fn openable(p: &Path) -> bool {
    std::fs::File::open(p).is_ok()
}

fn scan(f: &WatchFolder, out: &mut Vec<PathBuf>) {
    fn walk(dir: &Path, deep: bool, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if deep {
                    walk(&p, deep, out);
                }
            } else {
                out.push(p);
            }
        }
    }
    walk(Path::new(&f.path), f.subfolders, out);
}

pub struct WatchRunner {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for WatchRunner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl WatchRunner {
    /// Starts watching. `on_stable` receives batches of newly stable files; `skip` lists paths
    /// already in the project (they are never handed over).
    pub fn start(folders: Vec<WatchFolder>, stable_for: Duration, skip: HashSet<PathBuf>, on_stable: impl Fn(Vec<PathBuf>) + Send + 'static) -> WatchRunner {
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let thread = std::thread::Builder::new()
            .name("watch-folders".into())
            .spawn(move || run(folders, stable_for, skip, stop2, on_stable))
            .ok();
        WatchRunner { stop, thread }
    }
}

fn run(folders: Vec<WatchFolder>, stable_for: Duration, mut done: HashSet<PathBuf>, stop: Arc<AtomicBool>, on_stable: impl Fn(Vec<PathBuf>)) {
    let (tx, rx) = mpsc::channel::<PathBuf>();
    let mut watchers = Vec::new();
    for f in &folders {
        let tx = tx.clone();
        let w = notify::recommended_watcher(move |ev: notify::Result<notify::Event>| {
            if let Ok(ev) = ev {
                for p in ev.paths {
                    let _ = tx.send(p);
                }
            }
        });
        if let Ok(mut w) = w {
            let mode = if f.subfolders { RecursiveMode::Recursive } else { RecursiveMode::NonRecursive };
            if w.watch(Path::new(&f.path), mode).is_ok() {
                watchers.push(w);
            }
        }
    }
    let mut pending: HashMap<PathBuf, Seen> = HashMap::new();
    let mut last_scan = Instant::now() - Duration::from_secs(3600);
    let rescan = Duration::from_secs(30);
    let tick = Duration::from_millis(250).min(stable_for / 4).max(Duration::from_millis(20));
    let consider = |p: PathBuf, pending: &mut HashMap<PathBuf, Seen>, done: &HashSet<PathBuf>| {
        let p = std::path::absolute(&p).unwrap_or(p);
        if done.contains(&p) || !folders.iter().any(|f| f.accepts(&p)) {
            return;
        }
        if let Some((size, mtime)) = snapshot(&p) {
            pending.entry(p).or_insert(Seen { size, mtime, since: Instant::now() });
        }
    };
    while !stop.load(Ordering::Acquire) {
        if last_scan.elapsed() >= rescan {
            let mut found = Vec::new();
            for f in &folders {
                scan(f, &mut found);
            }
            for p in found {
                consider(p, &mut pending, &done);
            }
            last_scan = Instant::now();
        }
        while let Ok(p) = rx.try_recv() {
            consider(p, &mut pending, &done);
        }
        let mut ready = Vec::new();
        pending.retain(|p, s| {
            let Some((size, mtime)) = snapshot(p) else { return false }; // deleted
            if size != s.size || mtime != s.mtime {
                *s = Seen { size, mtime, since: Instant::now() };
                return true;
            }
            if s.since.elapsed() >= stable_for && size > 0 && openable(p) {
                ready.push(p.clone());
                return false;
            }
            true
        });
        if !ready.is_empty() {
            ready.sort();
            done.extend(ready.iter().cloned());
            on_stable(ready);
        }
        std::thread::sleep(tick);
    }
    drop(watchers);
}

impl crate::session::Session {
    /// The project's watch folders (stored in the project settings).
    pub fn watch_folders(&self) -> crate::session::Result<Vec<WatchFolder>> {
        let json = crate::store::lock(&self.db).settings_json()?;
        let v: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
        Ok(v.get("watch_folders").and_then(|w| serde_json::from_value(w.clone()).ok()).unwrap_or_default())
    }

    pub fn set_watch_folders(&self, folders: &[WatchFolder]) -> crate::session::Result<()> {
        let db = crate::store::lock(&self.db);
        let mut v: serde_json::Value = serde_json::from_str(&db.settings_json()?).unwrap_or_else(|_| serde_json::json!({}));
        if !v.is_object() {
            v = serde_json::json!({});
        }
        v["watch_folders"] = serde_json::to_value(folders).map_err(|e| crate::session::SessionError::Other(e.to_string()))?;
        db.set_settings_json(&v.to_string())?;
        Ok(())
    }

    /// Paths of every media item in the project (watch folders skip them).
    pub fn media_paths(&self) -> crate::session::Result<HashSet<PathBuf>> {
        Ok(crate::store::lock(&self.db).media_all()?.into_iter().map(|m| PathBuf::from(m.path)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Mutex;

    #[test]
    fn accepts_by_folder_depth_and_extension() {
        let f = WatchFolder { path: "D:\\Cards".into(), subfolders: false, extensions: vec!["mov".into()] };
        assert!(f.accepts(Path::new("D:\\Cards\\A001.MOV")));
        assert!(!f.accepts(Path::new("D:\\Cards\\A001.mp4")), "extension filter");
        assert!(!f.accepts(Path::new("D:\\Cards\\sub\\A001.mov")), "no subfolders");
        assert!(!f.accepts(Path::new("D:\\Cards\\.A001.mov")), "hidden");
        assert!(!f.accepts(Path::new("D:\\Elsewhere\\A001.mov")));
        let deep = WatchFolder { subfolders: true, extensions: vec![], ..f };
        assert!(deep.accepts(Path::new("D:\\Cards\\sub\\A001.mp4")));
        assert!(!deep.accepts(Path::new("D:\\Cards\\notes.txt")));
    }

    /// PLAN P9.5: a file copied in slowly is handed over once, and only after it is stable.
    #[test]
    fn slow_copy_is_picked_up_once_when_stable() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-watch-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let root = std::path::absolute(&root).unwrap();
        std::fs::write(root.join("already.mp4"), b"x").unwrap();
        let got: Arc<Mutex<Vec<(PathBuf, Instant)>>> = Arc::default();
        let g2 = got.clone();
        let folder = WatchFolder { path: root.to_string_lossy().into_owned(), subfolders: true, extensions: vec![] };
        let skip: HashSet<PathBuf> = [root.join("already.mp4")].into_iter().collect();
        let runner = WatchRunner::start(vec![folder], Duration::from_millis(800), skip, move |ps| {
            let now = Instant::now();
            g2.lock().unwrap().extend(ps.into_iter().map(|p| (p, now)));
        });

        // A throttled writer: 12 chunks over ~2.4 s (each gap shorter than the stability window).
        let target = root.join("sub").join("A002C001.mov");
        let mut f = std::fs::File::create(&target).unwrap();
        for _ in 0..12 {
            f.write_all(&[7u8; 64 * 1024]).unwrap();
            f.flush().unwrap();
            std::thread::sleep(Duration::from_millis(200));
        }
        drop(f);
        let finished = Instant::now();
        std::fs::write(root.join("notes.txt"), b"not media").unwrap();
        std::thread::sleep(Duration::from_millis(2500));
        drop(runner);

        let got = got.lock().unwrap();
        assert_eq!(got.len(), 1, "handed over exactly once: {got:?}");
        assert_eq!(got[0].0, target);
        assert!(got[0].1 >= finished, "only after the copy finished");
        drop(got);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
