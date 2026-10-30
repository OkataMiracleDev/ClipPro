//! P14.6 memory soak: a 100 h synthetic project (40 clips × 2.5 h at 4 Hz) re-scored
//! continuously with changing settings. The working set may not grow more than 5% after the
//! first minute of warm-up (caches filled). On demand, for one hour:
//!
//!     CLIPPRO_SOAK_SECS=3600 cargo test --release -p clippro-score --test soak -- --ignored --nocapture

use clippro_measure::calibration::Calibration;
use clippro_measure::columns::{Col, MeasurementSet, NCOLS};
use clippro_score::*;
use clippro_types::{ClipId, ColourProfile, FlagKind};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn project() -> Vec<ClipInput> {
    let per = 36_000usize;
    (0..40)
        .map(|c| {
            let mut ms = MeasurementSet::new(per);
            ms.time_us = (0..per as i64).map(|k| k * 250_000).collect();
            ms.cols = vec![vec![0.5; per]; NCOLS];
            for k in 0..per {
                let x = ((k as f32 * 0.013 + c as f32).sin() * 0.5 + 0.5).clamp(0.01, 0.99);
                ms.cols[Col::SharpFull as usize][k] = -3.6 + 0.35 * (x / (1.0 - x)).ln();
                ms.cols[Col::SharpCentre as usize][k] = -3.5 + 0.35 * (x / (1.0 - x)).ln();
                ms.cols[Col::StabRaw as usize][k] = x;
                ms.cols[Col::CamMag as usize][k] = 1.0 - x;
                ms.cols[Col::FlickerRaw as usize][k] = 0.1 * x;
            }
            ms.flags = (0..per).map(|k| if k % 101 == 0 { FlagKind::Shaky.bit() } else { 0 }).collect();
            ms.cut_before = (0..per).map(|k| k % 1200 == 0 && k > 0).collect();
            ms.rate_hz = 4.0;
            ms.fps = 25.0;
            ClipInput {
                id: ClipId(c + 1),
                version: 1,
                ms: Arc::new(ms),
                duration_us: per as i64 * 250_000,
                profile: ColourProfile::Rec709,
                extra: BTreeMap::new(),
                extra_flags: None,
                snaps: None,
            }
        })
        .collect()
}

/// Working set of this process, in bytes.
fn working_set() -> u64 {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_faults: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_nonpaged: usize,
        quota_nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters { cb: std::mem::size_of::<Counters>() as u32, ..Default::default() };
    // SAFETY: `c` is a correctly sized PROCESS_MEMORY_COUNTERS for the current process.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    assert!(ok != 0);
    c.working_set as u64
}

#[test]
#[ignore]
fn rescoring_100h_does_not_grow_memory() {
    let secs: f64 = std::env::var("CLIPPRO_SOAK_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(120.0);
    let clips = project();
    let none = HashMap::new();
    let cancel = AtomicBool::new(false);
    let mut engine = Engine::new(Calibration::embedded().clone());
    let presets: Vec<Settings> = builtins().iter().filter_map(|p| Settings::from_preset(p).ok()).collect();
    let warmup = Duration::from_secs_f64((secs * 0.1).clamp(10.0, 60.0));
    let start = Instant::now();
    let (mut runs, mut base, mut peak) = (0u64, 0u64, 0u64);
    while start.elapsed().as_secs_f64() < secs {
        // Slider-like changes across every preset: weights, threshold, cap, length.
        let mut s = presets[(runs as usize) % presets.len()].clone();
        let f = (runs % 7) as f32 / 7.0;
        s.weights[0] = 20.0 + 30.0 * f;
        s.score_lo = 0.45 + 0.2 * f;
        s.max_per_clip = if runs % 3 == 0 { Some(3) } else { None };
        s.len_min_s = 1.0 + (runs % 5) as f64;
        let sel = engine.score(&clips, &s, &none, &cancel).unwrap();
        assert_eq!(sel.clips.len(), clips.len());
        runs += 1;
        let ws = working_set();
        if base == 0 && start.elapsed() >= warmup {
            base = ws;
        }
        peak = peak.max(ws);
    }
    let end = working_set();
    eprintln!("{runs} re-scores in {secs:.0} s; working set after warm-up {} MB, end {} MB, peak {} MB", base >> 20, end >> 20, peak >> 20);
    assert!(base > 0, "the run was shorter than the warm-up");
    assert!(end as f64 <= base as f64 * 1.05, "working set grew {:.1}%", (end as f64 / base as f64 - 1.0) * 100.0);
}
