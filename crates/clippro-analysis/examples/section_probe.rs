//! Diagnostic: measures given sections of a file concurrently, printing progress with times, to
//! reproduce analysis stalls outside the app.
//!
//!     section_probe <file> <hw:auto|off> <start_s>[,<start_s>...] [len_s]

use clippro_analysis::clip::{clip_context, detect_active, measure_section_each, AnalyzeOptions};
use clippro_media::video::{sample_plan, HwMode};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let path = PathBuf::from(&a[1]);
    let hw = if a[2] == "off" { HwMode::Off } else { HwMode::Auto };
    let starts: Vec<f64> = a[3].split(',').map(|s| s.parse().unwrap()).collect();
    let len: f64 = a.get(4).map(|s| s.parse().unwrap()).unwrap_or(300.0);
    let info = clippro_media::probe(&path).unwrap();
    let mut opts = AnalyzeOptions::default();
    opts.decode.hw = hw;
    opts.decode.threads = 2;
    let plan = sample_plan(info.duration_us, info.fps(), opts.rate);
    let active = detect_active(&path, &info, &plan, &opts).unwrap();
    let ctx = clip_context(&info, active, 1, &opts);
    let t0 = Instant::now();
    std::thread::scope(|sc| {
        for &s in &starts {
            let (plan, ctx, opts, info, path) = (&plan, &ctx, &opts, &info, &path);
            sc.spawn(move || {
                let a = plan.partition_point(|t| *t < (s * 1e6) as i64);
                let b = plan.partition_point(|t| *t < ((s + len) * 1e6) as i64);
                let cancel = AtomicBool::new(false);
                let mut last = Instant::now();
                let mut n = 0;
                let r = measure_section_each(path, info, plan, a, b, ctx, opts, &cancel, |raw| {
                    n += 1;
                    if last.elapsed().as_secs_f64() > 5.0 {
                        last = Instant::now();
                        println!("[{:6.1}s] section {s}: sample {} ({:.1}s), {n} done", t0.elapsed().as_secs_f64(), raw.index, plan[raw.index] as f64 / 1e6);
                    }
                });
                println!("[{:6.1}s] section {s} finished: {:?}, {n} samples of {}", t0.elapsed().as_secs_f64(), r.err(), b - a);
            });
        }
    });
}
