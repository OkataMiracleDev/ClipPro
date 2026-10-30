//! P4.1–P4.5 tests: golden S1–S4 values, S5–S7 fixtures, caching, cancellation, determinism
//! and the SPEC §11 invariants.

use clippro_measure::calibration::Calibration;
use clippro_measure::columns::{Col, MeasurementSet, NCOLS};
use clippro_score::frame::{explain_sample, frame_scores, FrameScores};
use clippro_score::normalize::normalise;
use clippro_score::segments::{candidate_runs, finalise, segments_from_runs, Timing};
use clippro_score::*;
use clippro_types::{ClipId, ColourProfile, FlagKind, MetricGroup, Tier};
use proptest::prelude::*;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const RATE: f64 = 4.0;
const STEP: i64 = 250_000;

fn cal() -> Calibration {
    Calibration::embedded().clone()
}

/// A measurement set with every column NaN, times at 4 Hz.
fn blank(n: usize) -> MeasurementSet {
    let mut ms = MeasurementSet::new(n);
    ms.time_us = (0..n as i64).map(|k| k * STEP).collect();
    ms.cols = vec![vec![f32::NAN; n]; NCOLS];
    ms.flags = vec![0; n];
    ms.cut_before = vec![false; n];
    ms.rate_hz = RATE;
    ms.fps = 25.0;
    ms
}

fn input(id: i64, ms: MeasurementSet) -> ClipInput {
    let dur = ms.time_us.last().copied().unwrap_or(0) + STEP / 2;
    ClipInput {
        id: ClipId(id),
        version: id as u64,
        ms: Arc::new(ms),
        duration_us: dur,
        profile: ColourProfile::Rec709,
        extra: BTreeMap::new(),
        extra_flags: None,
        snaps: None,
    }
}

/// Settings where only Sharpness (pref +1) in the Focus group counts.
fn sharp_only() -> Settings {
    let mut s = Settings::default();
    for sm in SubMetric::ALL {
        s.sub_mut(sm).enabled = false;
    }
    let sh = s.sub_mut(SubMetric::Sharpness);
    sh.enabled = true;
    sh.pref = 1.0;
    s.weights = [1.0, 0.0, 0.0, 0.0, 0.0];
    s.stability_emphasis = 0.0;
    s.centre_weighting = 0.0;
    s
}

/// Sharpness raw value giving n = q under the embedded calibration.
fn sharp_for(q: f32) -> f32 {
    let c = cal();
    c.focus.sharp_s0 + c.focus.sharp_k * (q / (1.0 - q)).ln()
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn scores(ms: MeasurementSet, s: &Settings) -> FrameScores {
    let c = cal();
    frame_scores(&normalise(&input(1, ms), s.centre_weighting, &c), s)
}

// ---------- S1–S4 golden values ----------

#[test]
fn golden_bipolar_positive_and_negative() {
    let mut ms = blank(3);
    ms.col_mut(Col::SharpFull).fill(sharp_for(0.75));
    ms.col_mut(Col::SharpCentre).fill(sharp_for(0.75));
    let s = sharp_only();
    assert!(approx(scores(ms.clone(), &s).score[0], 0.75));
    let mut s2 = s.clone();
    s2.sub_mut(SubMetric::Sharpness).pref = -0.4;
    assert!(approx(scores(ms, &s2).score[0], 0.25));
}

#[test]
fn golden_group_weights() {
    let mut ms = blank(2);
    ms.col_mut(Col::SharpFull).fill(sharp_for(0.8));
    ms.col_mut(Col::SharpCentre).fill(sharp_for(0.8));
    ms.col_mut(Col::FlickerRaw).fill(0.6); // Flicker pref −1 → s = 0.4
    let mut s = sharp_only();
    let fl = s.sub_mut(SubMetric::Flicker);
    fl.enabled = true;
    fl.pref = -1.0;
    s.weights = [3.0, 1.0, 0.0, 0.0, 0.0];
    let f = scores(ms, &s).score[0];
    assert!(approx(f, (3.0 * 0.8 + 1.0 * 0.4) / 4.0), "{f}");
}

#[test]
fn golden_within_group_blend() {
    let mut ms = blank(1);
    ms.col_mut(Col::SharpFull).fill(sharp_for(0.9));
    ms.col_mut(Col::SharpCentre).fill(sharp_for(0.9));
    ms.col_mut(Col::DetailRaw).fill(0.3);
    let mut s = sharp_only();
    s.sub_mut(SubMetric::Sharpness).pref = 0.75;
    let d = s.sub_mut(SubMetric::Detail);
    d.enabled = true;
    d.pref = 0.25;
    let f = scores(ms, &s).score[0];
    assert!(approx(f, (0.75 * 0.9 + 0.25 * 0.3) / 1.0), "{f}");
}

#[test]
fn golden_confidence_blending() {
    let mut ms = blank(1);
    let mag = -0.25 * (1.0f32 - 0.8).ln(); // n = 0.8
    ms.col_mut(Col::CamMag).fill(mag);
    ms.col_mut(Col::CamConf).fill(0.5);
    let mut s = Settings::default();
    for sm in SubMetric::ALL {
        s.sub_mut(sm).enabled = false;
    }
    let c = s.sub_mut(SubMetric::CameraMotion);
    c.enabled = true;
    c.pref = 1.0;
    s.weights = [0.0, 0.0, 1.0, 0.0, 0.0];
    s.stability_emphasis = 0.0;
    let f = scores(ms, &s).score[0];
    assert!(approx(f, 0.5 * 0.8 + 0.5 * 0.5), "{f}");
}

#[test]
fn golden_target_luma() {
    let mut ms = blank(1);
    ms.col_mut(Col::P50).fill(0.6);
    let mut s = Settings::default();
    for sm in SubMetric::ALL {
        s.sub_mut(sm).enabled = false;
    }
    let l = s.sub_mut(SubMetric::Luma);
    l.enabled = true;
    l.target = 0.45;
    l.tol = 0.15;
    s.weights = [0.0, 1.0, 0.0, 0.0, 0.0];
    s.stability_emphasis = 0.0;
    let f = scores(ms, &s).score[0];
    assert!(approx(f, (-0.5f32).exp()), "{f}");
}

#[test]
fn golden_tier_multipliers() {
    let mut ms = blank(3);
    ms.col_mut(Col::SharpFull).fill(sharp_for(0.5));
    ms.col_mut(Col::SharpCentre).fill(sharp_for(0.5));
    ms.flags[1] = FlagKind::Shaky.bit();
    ms.flags[2] = FlagKind::Shaky.bit() | FlagKind::HorizonTilt.bit();
    let mut s = sharp_only();
    s.tiers[FlagKind::Shaky as usize] = Tier::Disfavour;
    s.tiers[FlagKind::HorizonTilt as usize] = Tier::Favour;
    let fs = scores(ms.clone(), &s);
    assert!(approx(fs.score[0], 0.5));
    assert!(approx(fs.score[1], 0.3));
    assert!(approx(fs.score[2], 0.36));
    // Flags master switch off → flags are informational only.
    s.switches.flags = false;
    let fs = scores(ms, &s);
    assert!(approx(fs.score[1], 0.5));
}

#[test]
fn golden_precedence() {
    let mut ms = blank(4);
    ms.col_mut(Col::SharpFull).fill(sharp_for(0.5));
    ms.col_mut(Col::SharpCentre).fill(sharp_for(0.5));
    ms.flags[1] = FlagKind::Duplicate.bit(); // force exclude
    ms.flags[2] = FlagKind::HorizonTilt.bit(); // force include (set below)
    ms.flags[3] = FlagKind::Duplicate.bit() | FlagKind::HorizonTilt.bit();
    let mut s = sharp_only();
    s.tiers[FlagKind::Duplicate as usize] = Tier::ForceExclude;
    s.tiers[FlagKind::HorizonTilt as usize] = Tier::ForceInclude;
    let fs = scores(ms, &s);
    assert_eq!(fs.force_out, vec![false, true, false, false]);
    assert_eq!(fs.force_in, vec![false, false, true, true]);
}

#[test]
fn golden_unavailable_group_renormalises() {
    let mut ms = blank(1);
    ms.col_mut(Col::SharpFull).fill(sharp_for(0.7));
    ms.col_mut(Col::SharpCentre).fill(sharp_for(0.7));
    let mut s = sharp_only();
    s.weights = [1.0, 0.0, 0.0, 5.0, 0.0]; // Audio has weight but no data
    assert!(approx(scores(ms, &s).score[0], 0.7));
}

#[test]
fn golden_metrics_switch_and_stability_emphasis() {
    let mut ms = blank(1);
    ms.col_mut(Col::SharpFull).fill(sharp_for(0.7));
    ms.col_mut(Col::SharpCentre).fill(sharp_for(0.7));
    ms.col_mut(Col::StabRaw).fill(0.5);
    let mut s = sharp_only();
    s.stability_emphasis = 1.0;
    assert!(approx(scores(ms.clone(), &s).score[0], 0.7 * (1.0 - 0.5 * 0.5)));
    s.switches.metrics = false;
    assert!(approx(scores(ms, &s).score[0], 1.0));
}

#[test]
fn golden_camera_filter() {
    let mut ms = blank(3);
    ms.col_mut(Col::SharpFull).fill(sharp_for(0.8));
    ms.col_mut(Col::SharpCentre).fill(sharp_for(0.8));
    ms.col_mut(Col::CamConf).fill(1.0);
    // Sample 0: static. 1: pan right 0.5 fw/s. 2: pan left 0.5 fw/s.
    let (ux, dir, mag) = ([0.0f32, -0.5, 0.5], [0.0f32, 0.0, 180.0], [0.0f32, 0.5, 0.5]);
    for k in 0..3 {
        ms.col_mut(Col::CamUx)[k] = ux[k];
        ms.col_mut(Col::CamUy)[k] = 0.0;
        ms.col_mut(Col::CamDir)[k] = dir[k];
        ms.col_mut(Col::CamMag)[k] = mag[k];
        ms.col_mut(Col::CamZr)[k] = 0.0;
        ms.col_mut(Col::CamWr)[k] = 0.0;
    }
    let mut s = sharp_only();
    s.camera.enabled = true;
    s.camera.static_only = true;
    let fs = scores(ms.clone(), &s);
    assert!(approx(fs.score[0], 0.8));
    assert!(fs.score[1] < 0.01);
    s.camera.static_only = false;
    s.camera.radius = 0.7;
    s.camera.direction = 0.0;
    let fs = scores(ms.clone(), &s);
    assert!(fs.score[0] < 0.01, "static rejected by a direction filter");
    assert!(approx(fs.score[1], 0.8));
    assert!(fs.score[2] < 0.01);
    s.camera.mirror = true;
    let fs = scores(ms, &s);
    assert!(approx(fs.score[2], 0.8));
}

#[test]
fn explain_matches_fast_path() {
    let c = cal();
    let mut ms = blank(200);
    for k in 0..200 {
        let x = (k as f32 * 0.37).sin() * 0.5 + 0.5;
        ms.col_mut(Col::SharpFull)[k] = sharp_for(0.05 + 0.9 * x);
        ms.col_mut(Col::SharpCentre)[k] = sharp_for(0.05 + 0.9 * (1.0 - x));
        ms.col_mut(Col::DetailRaw)[k] = x;
        ms.col_mut(Col::FocusStabRaw)[k] = 1.0 - x;
        ms.col_mut(Col::FlickerRaw)[k] = x * 0.3;
        ms.col_mut(Col::P50)[k] = 0.3 + 0.4 * x;
        for (col, v) in [(Col::ClipWhite, 0.01 * x), (Col::ClipBlack, 0.0), (Col::P01, 0.05), (Col::P99, 0.9)] {
            ms.col_mut(col)[k] = v;
        }
        ms.col_mut(Col::P05)[k] = 0.1;
        ms.col_mut(Col::P95)[k] = 0.8;
        ms.col_mut(Col::NoiseSigma)[k] = 0.01 * x;
        ms.col_mut(Col::CamMag)[k] = x;
        ms.col_mut(Col::CamConf)[k] = 0.7;
        ms.col_mut(Col::StabRaw)[k] = 1.0 - x;
        ms.col_mut(Col::StabConf)[k] = 0.9;
        ms.col_mut(Col::SubjRaw)[k] = x * 0.2;
        ms.flags[k] = if k % 7 == 0 { FlagKind::Shaky.bit() } else { 0 };
    }
    for name in ["All-Rounder", "Talking Head", "Whip Pans", "Locked-Off"] {
        let s = Settings::from_preset(&builtin(name).unwrap()).unwrap();
        let nc = normalise(&input(1, ms.clone()), s.centre_weighting, &c);
        let fs = frame_scores(&nc, &s);
        for k in 0..200 {
            let d = explain_sample(&nc, &s, k);
            assert!((d.score - fs.score[k]).abs() < 1e-5, "{name} k{k}: {} vs {}", d.score, fs.score[k]);
            assert_eq!(d.force_out, fs.force_out[k]);
        }
    }
}

// ---------- S5–S7 fixtures ----------

fn fs_from(score: &[f32]) -> FrameScores {
    let n = score.len();
    FrameScores {
        overall: score.to_vec(),
        score: score.to_vec(),
        force_in: vec![false; n],
        force_out: vec![false; n],
        flags: vec![0; n],
        cut_before: vec![false; n],
    }
}

fn times(n: usize) -> Vec<i64> {
    (0..n as i64).map(|k| k * STEP).collect()
}

fn base_settings() -> Settings {
    Settings {
        score_lo: 0.5,
        score_hi: 1.0,
        len_min_s: 0.0,
        len_max_s: None,
        max_per_clip: None,
        gap_s: 0.5,
        handles_samples: 0,
        ..Settings::default()
    }
}

fn run_segments(fs: &FrameScores, s: &Settings, overrides: &[Override]) -> Vec<Segment> {
    let t = times(fs.score.len());
    let timing = Timing { time_us: &t, rate_hz: RATE, duration_us: t.last().unwrap() + STEP / 2 };
    let runs = candidate_runs(fs, s, &timing);
    let segs = segments_from_runs(&runs, fs, s, &timing);
    finalise(segs, fs, s, &timing, overrides, None)
}

#[test]
fn gap_merging_and_cut_splitting() {
    // 0.9 ×4, 0.1 ×2 (gap 0.5 s = 2 samples), 0.9 ×4
    let sc = [0.9, 0.9, 0.9, 0.9, 0.1, 0.1, 0.9, 0.9, 0.9, 0.9];
    let mut fs = fs_from(&sc);
    let s = base_settings();
    let segs = run_segments(&fs, &s, &[]);
    assert_eq!(segs.len(), 1, "{segs:?}");
    assert!((segs[0].mean - (8.0 * 0.9 + 0.2) / 10.0).abs() < 1e-5);
    // A force-excluded sample inside the gap prevents merging.
    fs.force_out[4] = true;
    assert_eq!(run_segments(&fs, &s, &[]).len(), 2);
    fs.force_out[4] = false;
    // A hard cut inside the gap prevents merging.
    fs.cut_before[5] = true;
    assert_eq!(run_segments(&fs, &s, &[]).len(), 2);
    // A hard cut inside a run splits it.
    let mut fs = fs_from(&[0.9; 8]);
    fs.cut_before[4] = true;
    let segs = run_segments(&fs, &s, &[]);
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].last_sample, 3);
    assert_eq!(segs[1].first_sample, 4);
}

#[test]
fn length_rules_split_and_trim() {
    let mut sc = vec![0.8f32; 40]; // 10 s run
    sc[30] = 0.99;
    sc[31] = 0.99;
    let fs = fs_from(&sc);
    let mut s = base_settings();
    s.len_min_s = 11.0;
    assert!(run_segments(&fs, &s, &[]).is_empty());
    s.len_min_s = 1.0;
    s.len_max_s = Some(3.0);
    s.long_mode = preset::LongSegmentMode::TrimBest;
    let segs = run_segments(&fs, &s, &[]);
    assert_eq!(segs.len(), 1);
    assert!(segs[0].len_us() <= 3_000_000);
    assert!((segs[0].first_sample..=segs[0].last_sample).contains(&30));
    s.long_mode = preset::LongSegmentMode::Split;
    let segs = run_segments(&fs, &s, &[]);
    assert!(segs.len() >= 3, "{}", segs.len());
    for sg in &segs {
        assert!(sg.len_us() <= 3_000_000 && sg.len_us() >= 1_000_000);
    }
    for w in segs.windows(2) {
        assert!(w[0].end_us <= w[1].start_us);
    }
}

#[test]
fn handles_respect_bounds_cuts_and_neighbours() {
    let sc = [0.1, 0.9, 0.9, 0.1, 0.9, 0.9, 0.1, 0.1, 0.1, 0.9];
    let mut fs = fs_from(&sc);
    fs.cut_before[8] = true;
    let mut s = base_settings();
    s.gap_s = 0.0;
    s.handles_samples = 3;
    let segs = run_segments(&fs, &s, &[]);
    assert_eq!(segs.len(), 3);
    assert_eq!(segs[0].in_us, 0); // clamped at clip start
    assert!(segs[0].out_us <= segs[1].in_us); // neighbours never overlap
    assert!(segs[1].out_us <= 8 * STEP - STEP / 2); // stops at the cut
    let last = segs.last().unwrap();
    assert!(last.in_us >= 8 * STEP - STEP / 2);
    for sg in &segs {
        assert!(sg.in_us <= sg.start_us && sg.out_us >= sg.end_us);
    }
}

#[test]
fn per_clip_cap_with_ties() {
    let sc = [0.9, 0.1, 0.7, 0.7, 0.1, 0.7, 0.7, 0.1, 0.8];
    let fs = fs_from(&sc);
    let mut s = base_settings();
    s.gap_s = 0.0;
    s.max_per_clip = Some(2);
    let segs = run_segments(&fs, &s, &[]);
    let means: Vec<f32> = segs.iter().map(|x| x.mean).collect();
    assert_eq!(segs.len(), 2);
    assert!(means.contains(&0.9) && means.contains(&0.8));
    s.max_per_clip = Some(3);
    let segs = run_segments(&fs, &s, &[]);
    // Tie between the two 0.7 runs (same length) goes to the earlier one.
    assert!(segs.iter().any(|x| x.first_sample == 2));
    assert!(!segs.iter().any(|x| x.first_sample == 5));
}

#[test]
fn force_include_exemptions() {
    let sc = [0.2, 0.2, 0.2, 0.2, 0.1, 0.9, 0.9, 0.9, 0.9];
    let mut fs = fs_from(&sc);
    for k in 0..4 {
        fs.force_in[k] = true;
    }
    let mut s = base_settings();
    s.gap_s = 0.0;
    s.max_per_clip = Some(1);
    s.score_hi = 0.95;
    let segs = run_segments(&fs, &s, &[]);
    // Forced run kept despite low mean and the cap; still subject to length rules.
    assert!(segs.iter().any(|x| x.kind == SegmentKind::Forced));
    assert_eq!(segs.len(), 2);
    s.len_min_s = 5.0;
    assert!(run_segments(&fs, &s, &[]).is_empty());
}

#[test]
fn overrides_survive_rescoring() {
    let sc: Vec<f32> = (0..40).map(|k| if (10..20).contains(&k) { 0.9 } else { 0.3 }).collect();
    let fs = fs_from(&sc);
    let s = base_settings();
    let ovs = [
        Override { kind: OverrideKind::ForceIn, start_us: 30 * STEP, end_us: 35 * STEP },
        Override { kind: OverrideKind::ForceOut, start_us: 10 * STEP, end_us: 20 * STEP },
    ];
    let segs = run_segments(&fs, &s, &ovs);
    assert_eq!(segs.len(), 1);
    assert_eq!(segs[0].kind, SegmentKind::Manual);
    // Lower the threshold: more auto selects appear, but the overrides still apply.
    let mut s2 = s.clone();
    s2.score_lo = 0.2;
    let segs = run_segments(&fs, &s2, &ovs);
    assert!(segs.iter().any(|x| x.kind == SegmentKind::Manual && x.start_us <= 30 * STEP));
    assert!(!segs.iter().any(|x| x.start_us < 15 * STEP && x.end_us > 15 * STEP && x.kind == SegmentKind::Auto
        && (x.end_us.min(20 * STEP) - x.start_us.max(10 * STEP)) as f64 >= 0.5 * x.len_us() as f64));
}

// ---------- Engine: caching, cancellation, determinism ----------

fn project(nclips: usize, n: usize) -> Vec<ClipInput> {
    (0..nclips)
        .map(|c| {
            let mut ms = blank(n);
            for k in 0..n {
                let x = ((k as f32 * 0.05 + c as f32).sin() * 0.5 + 0.5).clamp(0.02, 0.98);
                ms.col_mut(Col::SharpFull)[k] = sharp_for(x);
                ms.col_mut(Col::SharpCentre)[k] = sharp_for(x);
                ms.col_mut(Col::DetailRaw)[k] = x;
                ms.col_mut(Col::FlickerRaw)[k] = 0.1;
                ms.col_mut(Col::StabRaw)[k] = x;
                ms.col_mut(Col::StabConf)[k] = 0.8;
                if k % 97 == 0 {
                    ms.cut_before[k] = k > 0;
                }
                if k % 53 == 0 {
                    ms.flags[k] = FlagKind::Shaky.bit();
                }
            }
            input(c as i64 + 1, ms)
        })
        .collect()
}

#[test]
fn incremental_stages() {
    let clips = project(4, 500);
    let mut e = Engine::new(cal());
    let s = Settings::default();
    let none = HashMap::new();
    let cancel = AtomicBool::new(false);
    let a = e.score(&clips, &s, &none, &cancel).unwrap();
    assert_eq!(e.counters.snapshot(), [4, 4, 4, 4, 4]);
    // Same settings: nothing recomputed.
    e.score(&clips, &s, &none, &cancel).unwrap();
    assert_eq!(e.counters.snapshot(), [4, 4, 4, 4, 4]);
    // Low threshold: S5+ only.
    let mut s2 = s.clone();
    s2.score_lo = 0.4;
    let b = e.score(&clips, &s2, &none, &cancel).unwrap();
    assert_eq!(e.counters.snapshot(), [4, 4, 8, 8, 8]);
    assert!(b.summary.segment_count >= 1 && a.summary.footage_us == b.summary.footage_us);
    // High threshold: S6+ only.
    let mut s3 = s2.clone();
    s3.score_hi = 0.9;
    e.score(&clips, &s3, &none, &cancel).unwrap();
    assert_eq!(e.counters.snapshot(), [4, 4, 8, 12, 12]);
    // Cap: S7 only.
    let mut s4 = s3.clone();
    s4.max_per_clip = Some(1);
    let d = e.score(&clips, &s4, &none, &cancel).unwrap();
    assert_eq!(e.counters.snapshot(), [4, 4, 8, 12, 16]);
    assert!(d.clips.iter().all(|c| c.segments.len() <= 1));
    // Centre weighting: from normalisation.
    let mut s5 = s4.clone();
    s5.centre_weighting = 0.9;
    e.score(&clips, &s5, &none, &cancel).unwrap();
    assert_eq!(e.counters.snapshot(), [8, 8, 12, 16, 20]);
}

#[test]
fn cancellation_returns_none_and_leaves_cache_consistent() {
    let clips = project(3, 300);
    let mut e = Engine::new(cal());
    let s = Settings::default();
    let none = HashMap::new();
    let cancel = AtomicBool::new(true);
    assert!(e.score(&clips, &s, &none, &cancel).is_none());
    let ok = AtomicBool::new(false);
    let a = e.score(&clips, &s, &none, &ok).unwrap();
    let mut fresh = Engine::new(cal());
    let b = fresh.score(&clips, &s, &none, &ok).unwrap();
    assert_eq!(a.sorted(SortOrder::Timeline), b.sorted(SortOrder::Timeline));
}

#[test]
fn thread_count_independent() {
    let clips = project(6, 800);
    let s = Settings::from_preset(&builtin("Talking Head").unwrap()).unwrap();
    let none = HashMap::new();
    let run = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
        pool.install(|| {
            let mut e = Engine::new(cal());
            e.score(&clips, &s, &none, &AtomicBool::new(false)).unwrap().sorted(SortOrder::ScoreDesc)
        })
    };
    assert_eq!(run(1), run(4));
}

#[test]
fn sort_orders_and_summary() {
    let clips = project(3, 400);
    let mut e = Engine::new(cal());
    let sel = e.score(&clips, &base_settings(), &HashMap::new(), &AtomicBool::new(false)).unwrap();
    let by_score = sel.sorted(SortOrder::ScoreDesc);
    for w in by_score.windows(2) {
        assert!(w[0].segment.mean >= w[1].segment.mean);
    }
    let by_len = sel.sorted(SortOrder::DurationAsc);
    for w in by_len.windows(2) {
        assert!(w[0].segment.len_us() <= w[1].segment.len_us());
    }
    let total: i64 = by_len.iter().map(|x| x.segment.len_us()).sum();
    assert_eq!(total, sel.summary.selects_us);
    assert!(sel.summary.flag_runs.iter().any(|(f, n)| *f == FlagKind::Shaky && *n > 0));
    assert_eq!(MetricGroup::ALL.len(), 5);
}

// ---------- SPEC §11 invariants (property tests) ----------

fn arb_fs() -> impl Strategy<Value = FrameScores> {
    (5usize..80).prop_flat_map(|n| {
        (
            prop::collection::vec(0.0f32..1.0, n),
            prop::collection::vec(prop::bool::weighted(0.05), n),
            prop::collection::vec(prop::bool::weighted(0.05), n),
            prop::collection::vec(prop::bool::weighted(0.05), n),
        )
            .prop_map(|(score, fin, fout, cut)| {
                let n = score.len();
                let fout: Vec<bool> = fout.iter().zip(&fin).map(|(o, i)| *o && !*i).collect();
                let mut cut = cut;
                cut[0] = false;
                FrameScores { overall: score.clone(), score, force_in: fin, force_out: fout, flags: vec![0; n], cut_before: cut }
            })
    })
}

fn covered(segs: &[Segment], k: usize) -> bool {
    segs.iter().any(|s| s.first_sample <= k && k <= s.last_sample)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn inv1_lowering_low_threshold_keeps_candidates(fs in arb_fs(), lo in 0.1f32..0.9, d in 0.0f32..0.3) {
        let t = times(fs.score.len());
        let timing = Timing { time_us: &t, rate_hz: RATE, duration_us: *t.last().unwrap() + STEP / 2 };
        let mut s = base_settings();
        s.score_lo = lo;
        let a = candidate_runs(&fs, &s, &timing);
        s.score_lo = (lo - d).max(0.0);
        let b = candidate_runs(&fs, &s, &timing);
        for &(x, y) in &a {
            for k in x..=y {
                prop_assert!(b.iter().any(|&(p, q)| p <= k && k <= q));
            }
        }
    }

    #[test]
    fn inv2_raising_high_threshold_keeps_segments(fs in arb_fs(), hi in 0.3f32..0.9, d in 0.0f32..0.2) {
        let mut s = base_settings();
        s.score_lo = 0.2;
        s.score_hi = hi;
        let a = run_segments(&fs, &s, &[]);
        s.score_hi = (hi + d).min(1.0);
        let b = run_segments(&fs, &s, &[]);
        for sg in &a {
            prop_assert!(b.iter().any(|x| x.start_us == sg.start_us && x.end_us == sg.end_us));
        }
    }

    #[test]
    fn inv3_force_include_survives_range_and_cap(fs in arb_fs()) {
        let mut s = base_settings();
        s.score_lo = 0.99;
        s.score_hi = 0.995;
        s.max_per_clip = Some(0);
        s.len_min_s = 0.0;
        let segs = run_segments(&fs, &s, &[]);
        for k in 0..fs.score.len() {
            if fs.force_in[k] {
                prop_assert!(covered(&segs, k), "forced sample {} not selected", k);
            }
        }
    }

    #[test]
    fn inv4_force_out_never_selected_without_override(fs in arb_fs()) {
        let segs = run_segments(&fs, &base_settings(), &[]);
        for k in 0..fs.score.len() {
            if fs.force_out[k] {
                prop_assert!(!covered(&segs, k));
            }
        }
        // ...but a manual force-in override wins.
        let n = fs.score.len() as i64;
        let ov = [Override { kind: OverrideKind::ForceIn, start_us: 0, end_us: n * STEP }];
        let segs = run_segments(&fs, &base_settings(), &ov);
        prop_assert!(segs.iter().any(|s| s.kind == SegmentKind::Manual));
    }

    #[test]
    fn inv5_deterministic(fs in arb_fs()) {
        let s = base_settings();
        prop_assert_eq!(run_segments(&fs, &s, &[]), run_segments(&fs, &s, &[]));
    }

    #[test]
    fn inv6_handles_never_overlap_or_cross(fs in arb_fs(), h in 0u32..6) {
        let mut s = base_settings();
        s.handles_samples = h;
        let segs = run_segments(&fs, &s, &[]);
        let dur = (fs.score.len() as i64 - 1) * STEP + STEP / 2;
        for w in segs.windows(2) {
            prop_assert!(w[0].out_us <= w[1].in_us);
        }
        for sg in &segs {
            prop_assert!(sg.in_us >= 0 && sg.out_us <= dur);
            prop_assert!(sg.in_us <= sg.start_us && sg.out_us >= sg.end_us);
            for k in 1..fs.score.len() {
                if fs.cut_before[k] {
                    let ct = k as i64 * STEP - STEP / 2;
                    // A handle never crosses a cut unless the core span already touches it.
                    if sg.end_us <= ct { prop_assert!(sg.out_us <= ct); }
                    if sg.start_us >= ct { prop_assert!(sg.in_us >= ct); }
                }
            }
        }
    }
}

/// TUN-12 / ALGORITHMS §8.1: with snapping on, edges move to the nearest sentence boundary
/// within 1.5 s, but never across a force-excluded sample.
#[test]
fn sentence_snapping() {
    let mut score = vec![0.1f32; 4];
    score.extend([0.9; 12]);
    score.extend([0.1; 8]);
    let mut fs = fs_from(&score);
    let mut s = base_settings();
    s.snap_to_sentences = true;
    let t = times(fs.score.len());
    let timing = Timing { time_us: &t, rate_hz: RATE, duration_us: t.last().unwrap() + STEP / 2 };
    let segs = segments_from_runs(&candidate_runs(&fs, &s, &timing), &fs, &s, &timing);
    let snaps = (vec![600_000, 2_500_000], vec![4_200_000, 9_000_000]);
    let out = finalise(segs.clone(), &fs, &s, &timing, &[], Some(&snaps));
    assert_eq!((out[0].start_us, out[0].end_us), (600_000, 4_200_000));
    // Off: unchanged.
    s.snap_to_sentences = false;
    let plain = finalise(segs.clone(), &fs, &s, &timing, &[], Some(&snaps));
    assert_ne!(plain[0].start_us, 600_000);
    // A force-excluded sample between the old start and the boundary blocks that edge only.
    s.snap_to_sentences = true;
    fs.force_out[3] = true;
    let blocked = finalise(segs, &fs, &s, &timing, &[], Some(&snaps));
    assert_eq!((blocked[0].start_us, blocked[0].end_us), (plain[0].start_us, 4_200_000));
}
