//! Offload (SPEC §19, PLAN P13.1): copy a card or folder to several destinations at once,
//! verify every copy by re-reading it, write ASC MHL history, and resume from a log.
//!
//! The source is read once in 8 MiB chunks and hashed as it's read. Each destination has its own
//! writer thread. Files are written as `*.clippro-part`, re-read and hashed, and renamed only
//! when the hash matches. Each destination gets `<dest>/<source folder>/` with an `ascmhl/`
//! history and a `.clippro-offload.jsonl` log that a later run uses to skip finished files.

use crate::hash::HashKind;
use crate::mhl::{self, Entry};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const CHUNK: usize = 8 << 20;
pub const LOG: &str = ".clippro-offload.jsonl";
const PART: &str = ".clippro-part";

#[derive(Debug, thiserror::Error)]
pub enum OffloadError {
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, OffloadError>;

fn io<T>(p: &Path, r: std::io::Result<T>) -> Result<T> {
    r.map_err(|e| OffloadError::Io(p.display().to_string(), e))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OffloadJob {
    pub source: PathBuf,
    pub destinations: Vec<PathBuf>,
    pub hash: HashKind,
    /// File or folder names to leave out; `*.ext` matches a suffix.
    pub ignore: Vec<String>,
}

impl OffloadJob {
    pub fn new(source: impl Into<PathBuf>, destinations: Vec<PathBuf>) -> OffloadJob {
        OffloadJob {
            source: source.into(),
            destinations,
            hash: HashKind::Xxh64,
            ignore: [".DS_Store", "Thumbs.db", "desktop.ini", mhl::DIR, LOG, "*.clippro-part"].iter().map(|s| s.to_string()).collect(),
        }
    }

    /// `<destination>/<source folder name>`.
    pub fn dest_root(&self, d: &Path) -> PathBuf {
        d.join(self.source.file_name().unwrap_or_else(|| std::ffi::OsStr::new("Offload")))
    }

    fn ignored(&self, name: &str) -> bool {
        self.ignore.iter().any(|p| match p.strip_prefix('*') {
            Some(suffix) => name.to_lowercase().ends_with(&suffix.to_lowercase()),
            None => name.eq_ignore_ascii_case(p),
        })
    }
}

#[derive(Debug)]
pub enum OffloadEvent<'a> {
    Started { files: usize, bytes: u64 },
    File { index: usize, rel: &'a str, size: u64 },
    /// Bytes read from the source so far (whole job).
    Progress { done: u64, total: u64 },
    /// A destination copy is written and about to be verified.
    Copied { dest_file: &'a Path },
    Verified { rel: &'a str, destination: usize, ok: bool },
    Skipped { rel: &'a str },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OffloadReport {
    pub files: usize,
    pub bytes: u64,
    pub copied: usize,
    pub skipped: usize,
    /// (relative path, destination index, reason)
    pub failed: Vec<(String, usize, String)>,
    /// The ASC MHL manifest written in each destination.
    pub manifests: Vec<PathBuf>,
    pub seconds: f64,
}

impl OffloadReport {
    pub fn ok(&self) -> bool {
        self.failed.is_empty()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LogLine {
    path: String,
    size: u64,
    mtime: u64,
    kind: HashKind,
    hash: String,
    ok: bool,
    time: String,
}

fn mtime_secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Relative paths (with `/`) and metadata of every file under `root`, sorted.
fn walk(job: &OffloadJob) -> Result<Vec<(String, PathBuf, u64, SystemTime)>> {
    let mut out = Vec::new();
    let mut stack = vec![job.source.clone()];
    while let Some(dir) = stack.pop() {
        for e in io(&dir, std::fs::read_dir(&dir))? {
            let e = io(&dir, e)?;
            let name = e.file_name().to_string_lossy().into_owned();
            if job.ignored(&name) {
                continue;
            }
            let p = e.path();
            let md = io(&p, e.metadata())?;
            if md.is_dir() {
                stack.push(p);
            } else if md.is_file() {
                let rel = p.strip_prefix(&job.source).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                out.push((rel, p, md.len(), md.modified().unwrap_or(UNIX_EPOCH)));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn read_log(root: &Path) -> HashMap<String, LogLine> {
    let Ok(text) = std::fs::read_to_string(root.join(LOG)) else { return HashMap::new() };
    // Later lines win: a re-copy after a failure replaces the earlier record.
    text.lines().filter_map(|l| serde_json::from_str::<LogLine>(l).ok()).map(|l| (l.path.clone(), l)).collect()
}

fn hash_file(kind: HashKind, p: &Path, cancel: &AtomicBool) -> Result<String> {
    let mut f = io(p, File::open(p))?;
    let mut h = kind.hasher();
    let mut buf = vec![0u8; CHUNK];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(OffloadError::Cancelled);
        }
        let n = io(p, f.read(&mut buf))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finish())
}

/// Copies one source file to every destination file at once; returns the source hash and the
/// per-destination write errors.
fn copy_to_all(kind: HashKind, src: &Path, dests: &[PathBuf], cancel: &AtomicBool, mut progress: impl FnMut(u64)) -> Result<(String, Vec<Option<String>>)> {
    let mut f = io(src, File::open(src))?;
    let mut h = kind.hasher();
    let results: Vec<Option<String>> = std::thread::scope(|s| -> Result<Vec<Option<String>>> {
        let mut txs = Vec::new();
        let mut handles = Vec::new();
        for d in dests {
            let (tx, rx) = sync_channel::<Arc<Vec<u8>>>(3);
            txs.push(tx);
            handles.push(s.spawn(move || -> Option<String> {
                let mut out = match File::create(d) {
                    Ok(o) => std::io::BufWriter::with_capacity(CHUNK, o),
                    Err(e) => return Some(e.to_string()),
                };
                let mut err = None;
                for chunk in rx {
                    if err.is_none() {
                        if let Err(e) = out.write_all(&chunk) {
                            err = Some(e.to_string());
                        }
                    }
                }
                if err.is_none() {
                    if let Err(e) = out.flush().and_then(|_| out.get_ref().sync_all()) {
                        err = Some(e.to_string());
                    }
                }
                err
            }));
        }
        loop {
            if cancel.load(Ordering::Relaxed) {
                drop(txs);
                for hd in handles {
                    let _ = hd.join();
                }
                return Err(OffloadError::Cancelled);
            }
            let mut buf = vec![0u8; CHUNK];
            let n = io(src, f.read(&mut buf))?;
            if n == 0 {
                break;
            }
            buf.truncate(n);
            h.update(&buf);
            let chunk = Arc::new(buf);
            for tx in &txs {
                // A writer that failed has hung up; its error is reported when joined.
                let _ = tx.send(chunk.clone());
            }
            progress(n as u64);
        }
        drop(txs);
        Ok(handles.into_iter().map(|hd| hd.join().unwrap_or_else(|_| Some("writer panicked".into()))).collect())
    })?;
    Ok((h.finish(), results))
}

/// Runs an offload. Already-verified files (same size and modification time, per each
/// destination's log, and present at the destination) are skipped.
pub fn run(job: &OffloadJob, cancel: &AtomicBool, mut on: impl FnMut(&OffloadEvent)) -> Result<OffloadReport> {
    let t0 = Instant::now();
    if job.destinations.is_empty() {
        return Err(OffloadError::Other("Choose at least one destination.".into()));
    }
    let src_meta = io(&job.source, std::fs::metadata(&job.source))?;
    if !src_meta.is_dir() {
        return Err(OffloadError::Other(format!("{} is not a folder.", job.source.display())));
    }
    let roots: Vec<PathBuf> = job.destinations.iter().map(|d| job.dest_root(d)).collect();
    for r in &roots {
        if r.starts_with(&job.source) || job.source.starts_with(r) {
            return Err(OffloadError::Other(format!("{} overlaps the source.", r.display())));
        }
        io(r, std::fs::create_dir_all(r))?;
    }
    let files = walk(job)?;
    let total: u64 = files.iter().map(|f| f.2).sum();
    let mut rep = OffloadReport { files: files.len(), bytes: total, ..OffloadReport::default() };
    on(&OffloadEvent::Started { files: files.len(), bytes: total });
    let logs: Vec<HashMap<String, LogLine>> = roots.iter().map(|r| read_log(r)).collect();
    let mut log_files: Vec<File> = roots
        .iter()
        .map(|r| io(&r.join(LOG), std::fs::OpenOptions::new().create(true).append(true).open(r.join(LOG))))
        .collect::<Result<_>>()?;
    // Per destination: the manifest entries.
    let mut entries: Vec<Vec<Entry>> = vec![Vec::new(); roots.len()];
    let mut done = 0u64;
    for (index, (rel, src, size, modified)) in files.iter().enumerate() {
        on(&OffloadEvent::File { index, rel, size: *size });
        let mt = mtime_secs(*modified);
        let finished = |k: usize| -> Option<&LogLine> {
            let l = logs[k].get(rel)?;
            let dest = roots[k].join(rel);
            let ok = l.ok && l.size == *size && l.mtime == mt && l.kind == job.hash && std::fs::metadata(&dest).map(|m| m.len() == *size).unwrap_or(false);
            ok.then_some(l)
        };
        if let Some(prev) = (0..roots.len()).map(finished).collect::<Option<Vec<_>>>() {
            let hash = prev[0].hash.clone();
            for e in entries.iter_mut() {
                e.push(Entry { path: rel.clone(), size: *size, modified: *modified, kind: job.hash, hash: hash.clone(), action: "original" });
            }
            rep.skipped += 1;
            done += size;
            on(&OffloadEvent::Skipped { rel });
            on(&OffloadEvent::Progress { done, total });
            continue;
        }
        let finals: Vec<PathBuf> = roots.iter().map(|r| r.join(rel)).collect();
        let parts: Vec<PathBuf> = finals.iter().map(|p| PathBuf::from(format!("{}{PART}", p.display()))).collect();
        for p in &parts {
            if let Some(dir) = p.parent() {
                io(dir, std::fs::create_dir_all(dir))?;
            }
        }
        let base = done;
        let copied = copy_to_all(job.hash, src, &parts, cancel, |n| {
            done += n;
            on(&OffloadEvent::Progress { done, total });
        });
        let (src_hash, write_errs) = match copied {
            Ok(v) => v,
            Err(e) => {
                for p in &parts {
                    let _ = std::fs::remove_file(p);
                }
                return Err(e);
            }
        };
        done = base + size;
        on(&OffloadEvent::Progress { done, total });
        let now = mhl::iso(SystemTime::now());
        let mut all_ok = true;
        for k in 0..roots.len() {
            let ok = match &write_errs[k] {
                Some(e) => Err(e.clone()),
                None => {
                    on(&OffloadEvent::Copied { dest_file: &parts[k] });
                    match hash_file(job.hash, &parts[k], cancel) {
                        Ok(h) if h == src_hash => Ok(()),
                        Ok(h) => Err(format!("checksum mismatch ({} {h}, source {src_hash})", job.hash.mhl_name())),
                        Err(OffloadError::Cancelled) => {
                            for p in &parts {
                                let _ = std::fs::remove_file(p);
                            }
                            return Err(OffloadError::Cancelled);
                        }
                        Err(e) => Err(e.to_string()),
                    }
                }
            };
            let ok = ok.and_then(|_| {
                let _ = std::fs::remove_file(&finals[k]);
                std::fs::rename(&parts[k], &finals[k]).map_err(|e| e.to_string())?;
                // Keep the source's modification time.
                if let Ok(f) = File::options().write(true).open(&finals[k]) {
                    let _ = f.set_modified(*modified);
                }
                Ok(())
            });
            on(&OffloadEvent::Verified { rel, destination: k, ok: ok.is_ok() });
            let line = LogLine { path: rel.clone(), size: *size, mtime: mt, kind: job.hash, hash: src_hash.clone(), ok: ok.is_ok(), time: now.clone() };
            let _ = writeln!(log_files[k], "{}", serde_json::to_string(&line).unwrap_or_default());
            match ok {
                Ok(()) => entries[k].push(Entry { path: rel.clone(), size: *size, modified: *modified, kind: job.hash, hash: src_hash.clone(), action: "original" }),
                Err(reason) => {
                    all_ok = false;
                    let _ = std::fs::remove_file(&parts[k]);
                    rep.failed.push((rel.clone(), k, reason));
                }
            }
        }
        if all_ok {
            rep.copied += 1;
        }
    }
    let now = SystemTime::now();
    for (k, r) in roots.iter().enumerate() {
        rep.manifests.push(io(r, mhl::write_generation(r, &entries[k], "transfer", &job.ignore, now))?);
    }
    rep.seconds = t0.elapsed().as_secs_f64();
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target").join(format!("tmp-offload-{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A small card: clips in nested folders (one bigger than a chunk) and junk to ignore.
    fn card(root: &Path) -> PathBuf {
        let c = root.join("A002R2EC");
        std::fs::create_dir_all(c.join("Clips")).unwrap();
        std::fs::create_dir_all(c.join("Sub/Deep")).unwrap();
        let big: Vec<u8> = (0..(CHUNK + 12_345)).map(|i| (i * 31 % 251) as u8).collect();
        std::fs::write(c.join("Clips/A002C006.mov"), &big).unwrap();
        std::fs::write(c.join("Clips/A002C007.mov"), b"tiny clip").unwrap();
        std::fs::write(c.join("Sub/Deep/meta.xml"), b"<x/>").unwrap();
        std::fs::write(c.join("Sub/empty.bin"), b"").unwrap();
        std::fs::write(c.join(".DS_Store"), b"junk").unwrap();
        c
    }

    fn manifest_hashes(p: &Path) -> Vec<(String, String)> {
        use quick_xml::events::Event;
        let text = std::fs::read_to_string(p).unwrap();
        let mut r = quick_xml::Reader::from_str(&text);
        let (mut path, mut out, mut cur) = (String::new(), Vec::new(), String::new());
        loop {
            match r.read_event().unwrap() {
                Event::Start(e) => cur = String::from_utf8_lossy(e.name().as_ref().as_bytes()).into_owned(),
                Event::Text(t) => {
                    let v = t.xml10_content().into_owned();
                    if cur == "path" {
                        path = v;
                    } else if cur == "xxh64" {
                        out.push((path.clone(), v));
                    }
                }
                Event::End(_) => cur.clear(),
                Event::Eof => break,
                _ => {}
            }
        }
        out
    }

    #[test]
    fn copies_verifies_and_writes_history() {
        let t = tmp("copy");
        let src = card(&t);
        let job = OffloadJob::new(&src, vec![t.join("Shuttle"), t.join("RAID")]);
        let mut events = (0, 0);
        let rep = run(&job, &AtomicBool::new(false), |e| match e {
            OffloadEvent::Verified { ok: true, .. } => events.0 += 1,
            OffloadEvent::Progress { .. } => events.1 += 1,
            _ => {}
        })
        .unwrap();
        assert!(rep.ok(), "{rep:?}");
        assert_eq!((rep.files, rep.copied, rep.skipped), (4, 4, 0), ".DS_Store ignored");
        assert_eq!(events.0, 8, "4 files × 2 destinations verified");
        assert!(events.1 >= 8, "chunk progress plus one per file");
        for d in ["Shuttle", "RAID"] {
            let root = t.join(d).join("A002R2EC");
            assert_eq!(std::fs::read(root.join("Clips/A002C006.mov")).unwrap(), std::fs::read(src.join("Clips/A002C006.mov")).unwrap());
            assert!(!root.join(".DS_Store").exists());
            let mt = |p: &Path| std::fs::metadata(p).unwrap().modified().unwrap();
            assert_eq!(mtime_secs(mt(&root.join("Clips/A002C007.mov"))), mtime_secs(mt(&src.join("Clips/A002C007.mov"))));
            // History: one generation, chained by C4.
            let gens = mhl::generations(&root);
            assert_eq!(gens, vec![1]);
            let chain = std::fs::read_to_string(root.join("ascmhl").join(mhl::CHAIN)).unwrap();
            assert!(chain.contains("<hashlist sequencenr=\"1\">") && chain.contains("<c4>c4"));
        }
        let h = manifest_hashes(&rep.manifests[0]);
        assert_eq!(h.len(), 4);
        assert!(h.iter().any(|(p, v)| p == "Clips/A002C007.mov" && *v == crate::hash::hash_bytes(HashKind::Xxh64, b"tiny clip")));
        assert!(h.iter().all(|(p, _)| !p.contains('\\')));
        std::fs::remove_dir_all(&t).unwrap();
    }

    /// PLAN P13.1: a corrupted byte in one destination is caught by verification.
    #[test]
    fn detects_a_corrupted_copy() {
        let t = tmp("corrupt");
        let src = card(&t);
        let job = OffloadJob::new(&src, vec![t.join("A"), t.join("B")]);
        let rep = run(&job, &AtomicBool::new(false), |e| {
            if let OffloadEvent::Copied { dest_file } = e {
                let s = dest_file.to_string_lossy();
                if s.contains(&format!("{}B{}", std::path::MAIN_SEPARATOR, std::path::MAIN_SEPARATOR)) && s.contains("A002C006") {
                    let mut b = std::fs::read(dest_file).unwrap();
                    b[4_000_000] ^= 0x01;
                    std::fs::write(dest_file, b).unwrap();
                }
            }
        })
        .unwrap();
        assert_eq!(rep.failed.len(), 1, "{rep:?}");
        assert_eq!((rep.failed[0].0.as_str(), rep.failed[0].1), ("Clips/A002C006.mov", 1));
        assert!(rep.failed[0].2.contains("checksum mismatch"));
        assert!(!t.join("B/A002R2EC/Clips/A002C006.mov").exists(), "a bad copy is never left under its real name");
        assert!(t.join("A/A002R2EC/Clips/A002C006.mov").exists());
        assert_eq!(manifest_hashes(&rep.manifests[1]).len(), 3, "the failed file is not in B's history");
        // A second run re-copies only the failed file.
        let rep2 = run(&job, &AtomicBool::new(false), |_| {}).unwrap();
        assert!(rep2.ok());
        assert_eq!((rep2.copied, rep2.skipped), (1, 3));
        assert_eq!(mhl::generations(&t.join("B/A002R2EC")), vec![1, 2]);
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn resumes_and_recopies_changed_files() {
        let t = tmp("resume");
        let src = card(&t);
        let job = OffloadJob::new(&src, vec![t.join("A")]);
        run(&job, &AtomicBool::new(false), |_| {}).unwrap();
        let again = run(&job, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!((again.copied, again.skipped), (0, 4));
        std::fs::write(src.join("Clips/A002C007.mov"), b"tiny clip, re-recorded").unwrap();
        std::fs::remove_file(t.join("A/A002R2EC/Sub/Deep/meta.xml")).unwrap();
        let third = run(&job, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!((third.copied, third.skipped), (2, 2), "changed size and missing copy");
        assert_eq!(std::fs::read(t.join("A/A002R2EC/Clips/A002C007.mov")).unwrap(), b"tiny clip, re-recorded");
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn cancel_leaves_no_partial_files_and_rejects_overlaps() {
        let t = tmp("cancel");
        let src = card(&t);
        let cancel = AtomicBool::new(false);
        let job = OffloadJob::new(&src, vec![t.join("A")]);
        let r = run(&job, &cancel, |e| {
            if matches!(e, OffloadEvent::Progress { .. }) {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        assert!(matches!(r, Err(OffloadError::Cancelled)));
        let mut stack = vec![t.join("A")];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                assert!(!e.file_name().to_string_lossy().ends_with(PART), "{:?}", e.path());
                if e.path().is_dir() {
                    stack.push(e.path());
                }
            }
        }
        assert!(run(&OffloadJob::new(&src, vec![src.clone()]), &AtomicBool::new(false), |_| {}).is_err());
        assert!(run(&OffloadJob::new(&src, vec![]), &AtomicBool::new(false), |_| {}).is_err());
        std::fs::remove_dir_all(&t).unwrap();
    }
}
