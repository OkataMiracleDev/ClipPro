//! P4.5: re-score 100 h of footage at 4 Hz (1.44 M samples) after slider changes.

use clippro_measure::calibration::Calibration;
use clippro_measure::columns::{Col, MeasurementSet, NCOLS};
use clippro_score::*;
use clippro_types::{ClipId, ColourProfile, FlagKind};
use criterion::{criterion_group, criterion_main, Criterion};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn project() -> Vec<ClipInput> {
    // 40 clips × 2.5 h = 100 h at 4 Hz.
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
            }
        })
        .collect()
}

fn bench(c: &mut Criterion) {
    let clips = project();
    let none = HashMap::new();
    let cancel = AtomicBool::new(false);
    let mut engine = Engine::new(Calibration::embedded().clone());
    let base = Settings::from_preset(&builtin("All-Rounder").unwrap()).unwrap();
    engine.score(&clips, &base, &none, &cancel);
    let mut g = c.benchmark_group("rescore_100h");
    g.sample_size(10);
    let mut flip = false;
    g.bench_function("weights_change (S1-S7)", |b| {
        b.iter(|| {
            flip = !flip;
            let mut s = base.clone();
            s.weights[0] = if flip { 50.0 } else { 35.0 };
            engine.score(&clips, &s, &none, &cancel)
        })
    });
    g.bench_function("low_threshold_change (S5-S7)", |b| {
        b.iter(|| {
            flip = !flip;
            let mut s = base.clone();
            s.score_lo = if flip { 0.6 } else { 0.55 };
            engine.score(&clips, &s, &none, &cancel)
        })
    });
    g.bench_function("cap_change (S7)", |b| {
        b.iter(|| {
            flip = !flip;
            let mut s = base.clone();
            s.max_per_clip = if flip { Some(3) } else { None };
            engine.score(&clips, &s, &none, &cancel)
        })
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
