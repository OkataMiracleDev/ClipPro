//! P2.6 / P2.7 acceptance on decoded footage: every truth expectation in testdata/truth.
//! Needs the corpus (`clippro-testgen --out testdata/gen --mini`). Skips when missing.

use clippro_analysis::clip::{analyze_clip, AnalyzeOptions};
use clippro_measure::calibration::Calibration;
use clippro_measure::columns::{Col, MeasurementSet};
use clippro_media::probe;
use clippro_types::{FlagKind, SampleRate};
use rayon::prelude::*;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

fn corpus() -> Option<PathBuf> {
    let d = std::env::var("CLIPPRO_CORPUS").map(PathBuf::from).unwrap_or_else(|_| root().join("gen"));
    d.join("blur_ramp.mp4").exists().then_some(d)
}

fn truth(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(root().join("truth").join(format!("{name}.json"))).unwrap()).unwrap()
}

fn analyse(dir: &Path, t: &Value) -> MeasurementSet {
    let p = dir.join(t["file"].as_str().unwrap());
    let info = probe(&p).unwrap();
    let opts = AnalyzeOptions { rate: SampleRate::Standard, ..Default::default() };
    analyze_clip(&p, &info, &opts, 7, Calibration::embedded(), &AtomicBool::new(false), |_, _| {}).unwrap()
}

fn samples_in(ms: &MeasurementSet, iv: &Value) -> Vec<usize> {
    let a = (iv["t"][0].as_f64().unwrap() * 1e6) as i64;
    let b = (iv["t"][1].as_f64().unwrap() * 1e6) as i64;
    (0..ms.len()).filter(|&k| ms.time_us[k] >= a && ms.time_us[k] <= b).collect()
}

fn med(v: &[f32]) -> f32 {
    clippro_measure::util::median(v)
}

fn col_med(ms: &MeasurementSet, c: Col, ks: &[usize]) -> f32 {
    med(&ks.iter().map(|&k| ms.get(c, k)).collect::<Vec<_>>())
}

/// Per-flag sample counts: (true positive, false negative, false positive, true negative).
type Confusion = BTreeMap<String, [usize; 4]>;

fn check_clip(name: &str, ms: &MeasurementSet, t: &Value, conf: &Mutex<Confusion>) -> Vec<String> {
    let mut errs = Vec::new();
    let mut fail = |m: String| errs.push(format!("{name}: {m}"));
    let flag_frac = |ks: &[usize], f: FlagKind| ks.iter().filter(|&&k| ms.has_flag(k, f)).count() as f32 / ks.len().max(1) as f32;
    for iv in t["intervals"].as_array().cloned().unwrap_or_default() {
        let ks = samples_in(ms, &iv);
        if ks.is_empty() {
            fail(format!("no samples in {}", iv["t"]));
            continue;
        }
        let e = &iv["expect"];
        let span = format!("{}", iv["t"]);
        for (key, want) in [("flags", true), ("no_flags", false)] {
            for f in e.get(key).and_then(|v| v.as_array()).cloned().unwrap_or_default() {
                let fk = FlagKind::from_key(f.as_str().unwrap()).unwrap();
                let hit = ks.iter().filter(|&&k| ms.has_flag(k, fk)).count();
                {
                    let mut c = conf.lock().unwrap();
                    let row = c.entry(fk.key().to_string()).or_default();
                    if want {
                        row[0] += hit;
                        row[1] += ks.len() - hit;
                    } else {
                        row[2] += hit;
                        row[3] += ks.len() - hit;
                    }
                }
                let frac = flag_frac(&ks, fk);
                if want && frac < 0.95 {
                    fail(format!("{span}: {} on {:.0}% of samples (want ≥95%)", fk.key(), frac * 100.0));
                }
                if !want && frac > 0.05 {
                    fail(format!("{span}: {} on {:.0}% of samples (want ≤5%)", fk.key(), frac * 100.0));
                }
            }
        }
        if let Some(dir) = e.get("cam_dir_deg").and_then(|v| v.as_f64()) {
            let d = col_med(ms, Col::CamDir, &ks) as f64;
            let diff = ((d - dir).rem_euclid(360.0)).min((dir - d).rem_euclid(360.0));
            if diff > 10.0 {
                fail(format!("{span}: direction {d:.1}° vs {dir}°"));
            }
        }
        if let Some(sp) = e.get("cam_speed_fw_s").and_then(|v| v.as_f64()) {
            let ux: Vec<f32> = ks.iter().map(|&k| ms.get(Col::CamUx, k).hypot(ms.get(Col::CamUy, k))).collect();
            let got = med(&ux) as f64;
            if ((got - sp) / sp).abs() > 0.15 {
                fail(format!("{span}: speed {got:.3} vs {sp} fw/s"));
            }
        }
        if let Some(sp) = e.get("cam_speed_min").and_then(|v| v.as_f64()) {
            let got = col_med(ms, Col::CamMag, &ks) as f64;
            if got < sp {
                fail(format!("{span}: cam_mag {got:.2} < {sp}"));
            }
        }
        for (key, col) in [("zr", Col::CamZr), ("wr", Col::CamWr)] {
            if let Some(want) = e.get(key).and_then(|v| v.as_f64()) {
                let got = col_med(ms, col, &ks) as f64;
                if ((got - want) / want).abs() > 0.2 {
                    fail(format!("{span}: {key} {got:.3} vs {want:.3}"));
                }
            }
        }
        if let Some(m) = e.get("stab_min").and_then(|v| v.as_f64()) {
            let got = col_med(ms, Col::StabRaw, &ks) as f64;
            if got < m {
                fail(format!("{span}: stability {got:.2} < {m}"));
            }
        }
        if let Some(m) = e.get("stab_max").and_then(|v| v.as_f64()) {
            let got = col_med(ms, Col::StabRaw, &ks) as f64;
            if got > m {
                fail(format!("{span}: stability {got:.2} > {m}"));
            }
        }
        if let Some(r) = e.get("subj_gt_cam").and_then(|v| v.as_f64()) {
            let (s, c) = (col_med(ms, Col::SubjRaw, &ks) as f64, col_med(ms, Col::CamMag, &ks) as f64);
            if s <= r * c {
                fail(format!("{span}: subject {s:.3} not > {r}× camera {c:.3}"));
            }
        }
        if let Some(m) = e.get("flicker_min").and_then(|v| v.as_f64()) {
            let got = col_med(ms, Col::FlickerRaw, &ks) as f64;
            if got < m {
                fail(format!("{span}: flicker {got:.2} < {m}"));
            }
        }
        if let Some(m) = e.get("row_band_min").and_then(|v| v.as_f64()) {
            let got = col_med(ms, Col::RowBand, &ks) as f64;
            if got < m {
                fail(format!("{span}: row_band {got:.2} < {m}"));
            }
        }
        if e.get("centre_ge_full").is_some() && col_med(ms, Col::SharpCentre, &ks) < col_med(ms, Col::SharpFull, &ks) {
            fail(format!("{span}: centre sharpness below full-frame"));
        }
        if let Some(m) = e.get("noise_min").and_then(|v| v.as_f64()) {
            let got = col_med(ms, Col::NoiseSigma, &ks) as f64;
            if got < m {
                fail(format!("{span}: noise {got:.4} < {m}"));
            }
        }
    }
    // Monotonic metric over the listed intervals or whole seconds.
    if let (Some(metric), Some(dir)) = (t.get("metric").and_then(|v| v.as_str()), t.get("monotonic").and_then(|v| v.as_str())) {
        let col = Col::from_key(metric).unwrap();
        let dur = t["duration_s"].as_f64().unwrap() as i64;
        let means: Vec<f32> = (0..dur)
            .map(|s| {
                let ks: Vec<usize> = (0..ms.len()).filter(|&k| ms.time_us[k] >= s * 1_000_000 + 200_000 && ms.time_us[k] <= s * 1_000_000 + 800_000).collect();
                col_med(ms, col, &ks)
            })
            .collect();
        let ok = means.windows(2).all(|w| if dir == "decreasing" { w[1] < w[0] } else { w[1] > w[0] });
        if !ok {
            fail(format!("{metric} not {dir}: {means:?}"));
        }
    }
    if let Some(cuts) = t.get("cut_times").and_then(|v| v.as_array()) {
        let want: Vec<usize> = cuts
            .iter()
            .map(|c| {
                let us = (c.as_f64().unwrap() * 1e6) as i64;
                (0..ms.len()).find(|&k| ms.time_us[k] >= us).unwrap()
            })
            .collect();
        let got: Vec<usize> = (0..ms.len()).filter(|&k| ms.cut_before[k]).collect();
        if got != want {
            fail(format!("cuts at samples {got:?}, want {want:?}"));
        }
    }
    if let Some(a) = t.get("active_rel").and_then(|v| v.as_array()) {
        let (w, h) = (ms.working.0 as f64, ms.working.1 as f64);
        let r = ms.active;
        let want = [a[0].as_f64().unwrap() * w, a[1].as_f64().unwrap() * h, a[2].as_f64().unwrap() * w, a[3].as_f64().unwrap() * h];
        let got = [r.x as f64, r.y as f64, r.w as f64, r.h as f64];
        if got.iter().zip(want).any(|(g, w)| (g - w).abs() > 2.0) {
            fail(format!("active {got:?} vs {want:?}"));
        }
    }
    errs
}

#[test]
fn corpus_expectations() {
    let Some(dir) = corpus() else {
        eprintln!("corpus missing: skipping");
        return;
    };
    let names: Vec<String> = std::fs::read_dir(root().join("truth"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path().file_stem().unwrap().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with("long_clip"))
        .collect();
    let conf = Mutex::new(Confusion::new());
    let errs: Vec<String> = names
        .par_iter()
        .flat_map(|n| {
            let t = truth(n);
            if !dir.join(t["file"].as_str().unwrap()).exists() {
                return vec![];
            }
            let ms = analyse(&dir, &t);
            check_clip(n, &ms, &t, &conf)
        })
        .collect();
    eprintln!("flag            TP    FN    FP    TN   recall precision");
    for (f, [tp, fneg, fp, tn]) in conf.lock().unwrap().iter() {
        let rec = *tp as f32 / (*tp + *fneg).max(1) as f32;
        let prec = *tp as f32 / (*tp + *fp).max(1) as f32;
        eprintln!("{f:<14} {tp:>4} {fneg:>5} {fp:>5} {tn:>5}   {rec:>5.2}  {prec:>5.2}");
    }
    for e in &errs {
        eprintln!("FAIL {e}");
    }
    assert!(errs.is_empty(), "{} expectation(s) failed", errs.len());
}
