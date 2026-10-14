//! Times analysis of each given clip (dev diagnostic): `cargo run --release --example timeclip -- FILES…`
use clippro_analysis::clip::{analyze_clip, AnalyzeOptions};
use clippro_measure::calibration::Calibration;
use std::sync::atomic::AtomicBool;

fn main() {
    for p in std::env::args().skip(1) {
        let path = std::path::Path::new(&p);
        let info = match clippro_media::probe(path) {
            Ok(i) => i,
            Err(e) => {
                println!("{p}: {e}");
                continue;
            }
        };
        let t = std::time::Instant::now();
        let r = analyze_clip(path, &info, &AnalyzeOptions::default(), 7, Calibration::embedded(), &AtomicBool::new(false), |_, _| {});
        let secs = t.elapsed().as_secs_f32();
        println!(
            "{:<22} {:>7.2}s {:>6.2}x realtime  {}",
            path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            secs,
            info.duration_us as f32 / 1e6 / secs,
            r.map(|m| format!("{} samples", m.len())).unwrap_or_else(|e| e.to_string())
        );
    }
}
