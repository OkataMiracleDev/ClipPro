//! Runs one file through the scheduler and prints progress (dev diagnostic).
use clippro_analysis::jobs::*;
use clippro_analysis::AnalyzeOptions;
use clippro_measure::calibration::Calibration;
use std::sync::{mpsc, Arc};
fn main() {
    let p = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let workers: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    let store = Arc::new(MemStore::default());
    let (tx, rx) = mpsc::channel();
    let cfg = SchedulerConfig { workers, split_threshold_us: 30_000_000, section_us: 25_000_000, chunk: 32, ..SchedulerConfig::default() };
    let s = Scheduler::new(cfg, Calibration::embedded().clone(), store.clone(), Some(tx));
    let t = std::time::Instant::now();
    s.add(JobSpec { id: 1, path: p.clone(), info: clippro_media::probe(&p).unwrap(), seed_base: 7, opts: AnalyzeOptions::default() });
    for e in rx.iter() {
        println!("{:>6.1}s {:?}", t.elapsed().as_secs_f32(), e);
        if matches!(e, Event::Finished { .. } | Event::Failed { .. }) {
            break;
        }
    }
}
