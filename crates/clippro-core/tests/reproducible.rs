//! P8.2 / ANA-12 reproducible mode: with CPU decoding, measurements are bit-identical across
//! runs, worker counts, decoder thread counts and long-clip splitting (sections stitched).
//! Needs the corpus; skips when missing.

use clippro_analysis::jobs::SchedulerConfig;
use clippro_core::{Session, SessionOptions};
use clippro_media::video::HwMode;
use std::path::Path;

fn blobs(name: &str, workers: usize, threads: i32, split: bool, hw: HwMode) -> Vec<(String, Vec<u8>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tmp = root.join(format!("target/tmp-repro-{name}"));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("media")).unwrap();
    for f in ["handheld_shake.mp4", "pan_left.mp4", "prores_hq.mov", "long_clip.mp4"] {
        std::fs::copy(root.join("testdata/gen").join(f), tmp.join("media").join(f)).unwrap();
    }
    let mut o = SessionOptions { cache_path: None, ..SessionOptions::default() };
    o.scheduler = SchedulerConfig { workers, ..SchedulerConfig::default() };
    if split {
        // long_clip (2 min) is analysed as four 30 s sections on different workers.
        o.scheduler.split_threshold_us = 60_000_000;
        o.scheduler.section_us = 30_000_000;
    }
    o.analyze.decode.threads = threads;
    o.analyze.decode.hw = hw;
    let s = Session::create(&tmp.join("p.clippro"), name, o).unwrap();
    s.import(&[tmp.join("media")]).unwrap();
    s.queue_analysis().unwrap();
    s.wait(|_| {});
    let names = s.clip_names().unwrap();
    let cal = clippro_measure::calibration::Calibration::embedded();
    let mut out: Vec<(String, Vec<u8>)> = s.clip_inputs().unwrap().iter().map(|c| (names[&c.id].0.clone(), clippro_measure::codec::encode(&c.ms, cal).unwrap())).collect();
    out.sort();
    s.close().unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
    out
}

#[test]
fn cpu_measurements_are_bit_identical() {
    let gen = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/gen");
    if !gen.join("long_clip.mp4").exists() {
        eprintln!("corpus missing: skipping");
        return;
    }
    let base = blobs("a", 4, 2, false, HwMode::Off);
    assert_eq!(base.len(), 4);
    for (name, workers, threads, split) in [("b", 4, 2, false), ("c", 1, 1, false), ("d", 3, 0, true)] {
        let other = blobs(name, workers, threads, split, HwMode::Off);
        for (x, y) in base.iter().zip(&other) {
            assert_eq!(x.0, y.0);
            assert!(x.1 == y.1, "{} differs with {workers} worker(s), {threads} decode thread(s), split {split}", x.0);
        }
    }
    // Hardware decoding is allowed to differ (driver colour paths); report it.
    let hw = blobs("hw", 4, 2, false, HwMode::Auto);
    let same = base.iter().zip(&hw).filter(|(x, y)| x.1 == y.1).count();
    eprintln!("hardware decode: {same} of {} clips bit-identical to the CPU path", base.len());
}
