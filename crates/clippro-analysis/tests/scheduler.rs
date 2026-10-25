//! P3.3: scheduler equivalence (split, pause/resume, stop/restart), failure isolation, ordering.
//! Needs the corpus (`clippro-testgen --out testdata/gen --mini`); skips when missing.

use clippro_analysis::clip::{analyze_clip, AnalyzeOptions};
use clippro_analysis::jobs::*;
use clippro_measure::calibration::Calibration;
use clippro_measure::columns::{MeasurementSet, NCOLS};
use clippro_media::probe;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

fn corpus() -> Option<PathBuf> {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/gen");
    if d.join("long_clip.mp4").exists() {
        Some(d)
    } else {
        eprintln!("corpus missing: skipping");
        None
    }
}

fn spec(id: JobId, path: &Path) -> JobSpec {
    JobSpec { id, path: path.to_path_buf(), info: probe(path).unwrap(), seed_base: 7, opts: AnalyzeOptions::default() }
}

fn direct(path: &Path) -> MeasurementSet {
    let info = probe(path).unwrap();
    analyze_clip(path, &info, &AnalyzeOptions::default(), 7, Calibration::embedded(), &AtomicBool::new(false), |_, _| {}).unwrap()
}

fn assert_same(a: &MeasurementSet, b: &MeasurementSet) {
    assert_eq!(a.len(), b.len());
    assert_eq!(a.time_us, b.time_us);
    assert_eq!(a.flags, b.flags, "flags differ");
    assert_eq!(a.cut_before, b.cut_before);
    assert_eq!(a.active, b.active);
    for c in 0..NCOLS {
        for (k, (x, y)) in a.cols[c].iter().zip(&b.cols[c]).enumerate() {
            assert!(x.to_bits() == y.to_bits() || (x.is_nan() && y.is_nan()), "col {c} sample {k}: {x} vs {y}");
        }
    }
    assert_eq!(a.tiles.len(), b.tiles.len());
    assert_eq!(a.mvs, b.mvs);
    assert_eq!(a.phash, b.phash);
    assert_eq!(a.hist64, b.hist64);
}

fn small_sections() -> SchedulerConfig {
    SchedulerConfig { workers: 4, split_threshold_us: 30_000_000, section_us: 25_000_000, chunk: 32, ..SchedulerConfig::default() }
}

#[test]
fn split_sections_equal_single_pass() {
    let Some(dir) = corpus() else { return };
    let p = dir.join("long_clip.mp4");
    let store = Arc::new(MemStore::default());
    let s = Scheduler::new(small_sections(), Calibration::embedded().clone(), store.clone(), None);
    let t = std::time::Instant::now();
    s.add(spec(1, &p));
    s.wait_idle();
    eprintln!("scheduler (4 workers, 5 sections): {:?}", t.elapsed());
    assert_eq!(s.status(1), Some(JobStatus::Done), "{:?}", store.failure(1));
    let t = std::time::Instant::now();
    let d = direct(&p);
    eprintln!("direct single pass: {:?}", t.elapsed());
    assert_same(&store.result(1).unwrap(), &d);
}

#[test]
fn pause_resume_and_restart_equal_single_pass() {
    let Some(dir) = corpus() else { return };
    let p = dir.join("long_clip.mp4");
    let store = Arc::new(MemStore::default());
    let (tx, rx) = mpsc::channel();
    {
        let s = Scheduler::new(small_sections(), Calibration::embedded().clone(), store.clone(), Some(tx));
        s.add(spec(1, &p));
        // Wait for some checkpoints, pause, check it's not finished, resume briefly, then
        // drop the scheduler mid-run (like closing the app).
        let mut seen = 0;
        while store.chunk_count(1) < 4 && seen < 2000 {
            let _ = rx.recv_timeout(Duration::from_millis(50));
            seen += 1;
        }
        s.pause();
        s.wait_idle();
        assert_ne!(s.status(1), Some(JobStatus::Done));
        let (done, total) = s.progress(1).unwrap();
        assert!(done > 0 && done < total, "{done}/{total}");
        s.resume();
        std::thread::sleep(Duration::from_millis(300));
    }
    assert!(store.result(1).is_none(), "finished too early for the restart check");
    // A new scheduler resumes from the checkpoints.
    let s = Scheduler::new(small_sections(), Calibration::embedded().clone(), store.clone(), None);
    s.add(spec(1, &p));
    s.wait_idle();
    assert_eq!(s.status(1), Some(JobStatus::Done), "{:?}", store.failure(1));
    assert_same(&store.result(1).unwrap(), &direct(&p));
}

#[test]
fn bad_file_fails_alone_and_bump_reorders() {
    let Some(dir) = corpus() else { return };
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-sched-test");
    std::fs::create_dir_all(&tmp).unwrap();
    // A file that probes fine but whose video data is cut off.
    let src = std::fs::read(dir.join("pan_right.mp4")).unwrap();
    let bad = tmp.join("bad.mp4");
    std::fs::write(&bad, &src).unwrap();
    let bad_spec = spec(10, &bad);
    std::fs::write(&bad, &src[..src.len() / 4]).unwrap();

    let store = Arc::new(MemStore::default());
    let (tx, rx) = mpsc::channel();
    let cfg = SchedulerConfig { workers: 1, ..SchedulerConfig::default() };
    let s = Scheduler::new(cfg, Calibration::embedded().clone(), store.clone(), Some(tx));
    s.pause();
    s.add(spec(1, &dir.join("tripod_static.mp4")));
    s.add(bad_spec);
    s.add(spec(2, &dir.join("pan_left.mp4")));
    s.add(spec(3, &dir.join("tilt_up.mp4")));
    s.bump(3);
    assert_eq!(s.queue(), vec![3, 1, 10, 2]);
    s.resume();
    s.wait_idle();
    let finished: Vec<JobId> = rx.try_iter().filter_map(|e| if let Event::Finished { job } = e { Some(job) } else { None }).filter(|j| *j != 10).collect();
    assert_eq!(finished, vec![3, 1, 2]);
    // The truncated file either analyses what's left or fails; it must not stop the queue.
    let st = s.status(10).unwrap();
    assert!(matches!(st, JobStatus::Done | JobStatus::Failed), "{st:?}");
    for id in [1, 2, 3] {
        assert!(store.result(id).is_some());
    }
    drop(s);
    std::fs::remove_dir_all(&tmp).unwrap();
}
