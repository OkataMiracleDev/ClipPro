//! Temporal pass over a clip's samples (ALGORITHMS §2): cuts first, then stability, flicker,
//! shake and the stored flags. Produces the final `MeasurementSet`.

use crate::calibration::Calibration;
use crate::columns::{Col, MeasurementSet, NCOLS};
use crate::image::Rect;
use crate::sample::{ClipContext, SampleRaw};
use crate::util::{mad, median, percentile_sorted, smoothstep};
use clippro_types::FlagKind;

/// Samples spanning `s` seconds at rate `r` (ALGORITHMS §2.1).
pub fn wsamples(s: f64, r: f64) -> usize {
    ((s * r).round() as usize).max(1)
}

/// Window bounds `[lo, hi]` around `k`, truncated at clip ends and hard cuts.
struct Windows {
    seg_start: Vec<usize>,
    seg_end: Vec<usize>,
}

impl Windows {
    fn new(cut_before: &[bool]) -> Self {
        let n = cut_before.len();
        let mut seg_start = vec![0; n];
        let mut seg_end = vec![0; n];
        let mut s = 0;
        for k in 0..n {
            if k > 0 && cut_before[k] {
                s = k;
            }
            seg_start[k] = s;
        }
        let mut e = n.saturating_sub(1);
        for k in (0..n).rev() {
            seg_end[k] = e;
            if cut_before[k] && k > 0 {
                e = k - 1;
            }
        }
        Windows { seg_start, seg_end }
    }

    fn at(&self, k: usize, w: usize) -> (usize, usize) {
        (k.saturating_sub(w).max(self.seg_start[k]), (k + w).min(self.seg_end[k]))
    }
}

/// Runs the temporal pass. `rate_hz` is the effective sample rate (fps for "All").
pub fn temporal_pass(
    samples: &[SampleRaw],
    ctx: &ClipContext,
    rate_hz: f64,
    cal: &Calibration,
) -> MeasurementSet {
    let n = samples.len();
    let mut ms = MeasurementSet::new(n);
    ms.rate_hz = rate_hz;
    ms.fps = ctx.fps;
    ms.pair_sampling = ctx.pair_sampling;
    ms.active = ctx.active;
    ms.working = (ctx.working.0 as u32, ctx.working.1 as u32);
    if n == 0 {
        return ms;
    }
    for s in samples {
        ms.time_us.push(s.time_us);
        for c in 0..NCOLS {
            ms.cols[c].push(s.v[c]);
        }
    }
    let tiles = samples[0].tiles.len();
    let (tc, tr) = crate::spatial::tile_grid(ctx.active.w.max(1), ctx.active.h.max(1));
    ms.tile_dims = if tc * tr == tiles { (tc as u8, tr as u8) } else { (tiles as u8, 1) };
    let mvg = samples.iter().map(|s| s.mv_grid).find(|g| g.0 > 0).unwrap_or((16, 9));
    ms.mv_dims = mvg;
    let mv_len = mvg.0 as usize * mvg.1 as usize;
    for s in samples {
        ms.tiles.extend(s.tiles.iter().copied().chain(std::iter::repeat(f32::NAN)).take(tiles));
        if s.mvs.len() == mv_len {
            ms.mvs.extend_from_slice(&s.mvs);
        } else {
            ms.mvs.extend(std::iter::repeat_n([i8::MIN, i8::MIN], mv_len));
        }
        ms.phash.push(s.phash);
        ms.hist64.extend(s.hist64.iter().map(|h| (h * 255.0).round().clamp(0.0, 255.0) as u8));
        if s.hist64.len() < 64 {
            ms.hist64.extend(std::iter::repeat_n(0, 64 - s.hist64.len()));
        }
    }
    let r = rate_hz;
    let col = |ms: &MeasurementSet, c: Col| ms.col(c).to_vec();

    // §2.6 Hard cuts (computed first; windows stop at cuts).
    let chi = col(&ms, Col::Chi2Prev);
    let madp = col(&ms, Col::MadPrev);
    let madpair = col(&ms, Col::MadPair);
    let avgconf = col(&ms, Col::AvgConf);
    let c_score: Vec<f32> = (0..n)
        .map(|k| {
            if chi[k].is_nan() {
                0.0
            } else {
                0.5 * smoothstep(0.05, 0.6, chi[k]) + 0.5 * smoothstep(0.02, 0.2, madp[k])
            }
        })
        .collect();
    let sc = &cal.similarity;
    let mut cut_before = vec![false; n];
    for k in 1..n {
        let lo = k.saturating_sub(5);
        let hi = (k + 5).min(n - 1);
        let neigh: Vec<f32> = (lo..=hi).filter(|&i| i != k).map(|i| c_score[i]).collect();
        let med = median(&neigh);
        let med = if med.is_nan() { 0.0 } else { med };
        let conf = if avgconf[k].is_nan() { 0.0 } else { avgconf[k] };
        let pair_ok = madpair[k].is_nan() || madpair[k] < 0.5 * madp[k];
        if c_score[k] > sc.cut_min && c_score[k] > sc.cut_ratio * med && conf < sc.cut_conf_max && pair_ok
        {
            cut_before[k] = true;
        }
    }
    let win = Windows::new(&cut_before);

    // §2.2 Focus stability and hunting.
    let fcal = &cal.focus;
    let s_full = col(&ms, Col::SharpFull);
    let w075 = wsamples(0.75, r);
    let w1 = wsamples(1.0, r);
    let mut focus_stab = vec![f32::NAN; n];
    let mut hunting = vec![false; n];
    let sign_changes = |lo: usize, hi: usize, thr: f32| -> usize {
        let mut last = 0f32;
        let mut changes = 0;
        for i in lo + 1..=hi {
            let d = s_full[i] - s_full[i - 1];
            if d.abs() > thr {
                if last != 0.0 && d.signum() != last.signum() {
                    changes += 1;
                }
                last = d;
            }
        }
        changes
    };
    for k in 0..n {
        let (lo, hi) = win.at(k, w075);
        let m = mad(&s_full[lo..=hi]);
        let osc = sign_changes(lo, hi, 0.10) as f32 / (hi - lo + 1) as f32;
        focus_stab[k] =
            (1.0 - m / fcal.stab_mad_scale - fcal.stab_osc_weight * osc).clamp(0.0, 1.0);
        let (lo, hi) = win.at(k, w1);
        let ch = sign_changes(lo, hi, fcal.hunt_step);
        let window = &s_full[lo..=hi];
        let range = window.iter().cloned().fold(f32::MIN, f32::max)
            - window.iter().cloned().fold(f32::MAX, f32::min);
        hunting[k] = ch as u32 >= fcal.hunt_min_changes && range > fcal.hunt_range;
    }

    // §2.3 Flicker.
    let ecal = &cal.exposure;
    let mean = col(&ms, Col::Mean);
    let pair_dmean = col(&ms, Col::PairDmean);
    let row_band = col(&ms, Col::RowBand);
    let w15 = wsamples(1.5, r);
    let w05 = wsamples(0.5, r);
    let mut detr = vec![0f32; n];
    let mut med_mu = vec![0f32; n];
    for k in 0..n {
        let (lo, hi) = win.at(k, w15);
        med_mu[k] = median(&mean[lo..=hi]);
        detr[k] = mean[k] - med_mu[k];
    }
    let mut flicker = vec![0f32; n];
    for k in 0..n {
        let (lo, hi) = win.at(k, w05);
        let rms = (detr[lo..=hi].iter().map(|v| v * v).sum::<f32>() / (hi - lo + 1) as f32).sqrt();
        let f = rms / med_mu[k].max(0.02);
        // Frame-to-frame brightness change beyond what the sample-level trend predicts
        // (a smooth exposure ramp is not flicker).
        let pd = if pair_dmean[k].is_nan() {
            0.0
        } else {
            let (lo, hi) = win.at(k, 1);
            let trend = if hi > lo {
                let dt = (ms.time_us[hi] - ms.time_us[lo]) as f32 / 1e6;
                ((mean[hi] - mean[lo]) / dt.max(1e-3)).abs() / ctx.fps.max(1.0) as f32 / mean[k].max(0.02)
            } else {
                0.0
            };
            smoothstep(ecal.flicker_pair_lo, ecal.flicker_pair_hi, (pair_dmean[k] - trend).max(0.0))
        };
        // Under fast camera motion, sample-level brightness changes mostly come from new content.
        let cm = ms.get(Col::CamMag, k);
        let motion_damp = if cm.is_nan() { 1.0 } else { 1.0 - smoothstep(0.05, 0.25, cm) };
        flicker[k] = (smoothstep(ecal.flicker_f_lo, ecal.flicker_f_hi, f) * motion_damp)
            .max(pd)
            .max(smoothstep(ecal.flicker_band_lo, ecal.flicker_band_hi, row_band[k]));
    }

    // §2.4 Motion stability (shake).
    let mcal = &cal.motion;
    let ux = col(&ms, Col::CamUx);
    let uy = col(&ms, Col::CamUy);
    let shake_ia = col(&ms, Col::ShakeIa);
    let conf = col(&ms, Col::CamConf);
    // High-frequency velocity jitter: deviation of each sample's velocity from the mean of its
    // neighbours (zero for smooth moves, ramps and single speed steps).
    let lap: Vec<f32> = (0..n)
        .map(|k| {
            if k == 0 || k + 1 >= n || cut_before[k] || cut_before[k + 1] {
                return f32::NAN;
            }
            let (a, b, c) = (k - 1, k, k + 1);
            if ux[a].is_nan() || ux[b].is_nan() || ux[c].is_nan() {
                return f32::NAN;
            }
            (ux[b] - 0.5 * (ux[a] + ux[c])).hypot(uy[b] - 0.5 * (uy[a] + uy[c]))
        })
        .collect();
    let mut stab = vec![f32::NAN; n];
    let mut stab_conf = vec![0f32; n];
    let alias = if ctx.pair_sampling { 1.0 } else { (r / 8.0).min(1.0) as f32 };
    for k in 0..n {
        let (lo, hi) = win.at(k, w05);
        if ux[k].is_nan() && lap[lo..=hi].iter().all(|v| v.is_nan()) {
            continue;
        }
        let j1 = median(&lap[lo..=hi]);
        let j1 = if j1.is_nan() { 0.0 } else { j1 };
        let j2 = median(&shake_ia[lo..=hi]);
        let j = j1.max(if j2.is_nan() { 0.0 } else { j2 });
        stab[k] = (-j / mcal.stab_scale).exp();
        let c = median(&conf[lo..=hi]);
        stab_conf[k] = if c.is_nan() { 0.0 } else { c * alias };
    }

    // Flags.
    let mut flags = vec![0u32; n];
    let mut set_flag = |k: usize, f: FlagKind| flags[k] |= f.bit();
    for k in 0..n {
        if cut_before[k] {
            set_flag(k, FlagKind::HardCut);
        }
        if hunting[k] {
            set_flag(k, FlagKind::FocusHunting);
        }
    }
    // Shaky: sustained low stability with confidence.
    let shaky_raw: Vec<bool> = (0..n)
        .map(|k| !stab[k].is_nan() && stab[k] < mcal.shaky_stab && stab_conf[k] >= mcal.shaky_conf)
        .collect();
    for (a, b) in runs(&shaky_raw) {
        if b - a + 1 >= w05 {
            for k in a..=b {
                set_flag(k, FlagKind::Shaky);
            }
        }
    }
    // Motion blur (§2.5).
    let ppf = col(&ms, Col::PxPerFrame);
    let bdr = col(&ms, Col::BlurDirRatio);
    let w3 = wsamples(3.0, r);
    for k in 0..n {
        let (lo, hi) = win.at(k, w3);
        let mut v: Vec<f32> = s_full[lo..=hi].iter().copied().filter(|x| !x.is_nan()).collect();
        v.sort_by(f32::total_cmp);
        let sref = percentile_sorted(&v, 0.90);
        if ppf[k] >= mcal.blur_px_per_frame
            && s_full[k] < sref - mcal.blur_sharp_drop
            && bdr[k] < mcal.blur_dir_ratio
            && conf[k] >= mcal.blur_conf
        {
            set_flag(k, FlagKind::MotionBlur);
        }
    }
    // Duplicate (§2.7), clipping (§2.8), patterns (§2.10).
    let cb = col(&ms, Col::ClipBlack);
    let cw = col(&ms, Col::ClipWhite);
    let tilt = col(&ms, Col::TiltDeg);
    for k in 0..n {
        let dup = if ctx.pair_sampling && !madpair[k].is_nan() {
            madpair[k] < sc.dup_pair_mad && !madp[k].is_nan() && madp[k] < sc.dup_prev_mad
        } else {
            !madp[k].is_nan() && madp[k] < sc.dup_pair_mad
        };
        if dup {
            set_flag(k, FlagKind::Duplicate);
        }
        if cb[k] >= ecal.clip_flag_fraction || cw[k] >= ecal.clip_flag_fraction {
            set_flag(k, FlagKind::Clipping);
        }
        if samples[k].solid != 0 {
            set_flag(k, FlagKind::SolidColour);
        }
        if samples[k].bars {
            set_flag(k, FlagKind::ColourBars);
        }
        let (lo, hi) = win.at(k, w1);
        let valid: Vec<f32> = tilt[lo..=hi].iter().copied().filter(|t| !t.is_nan()).collect();
        if valid.len() as f32 >= 0.7 * (hi - lo + 1) as f32 && median(&valid).abs() > 2.0 {
            set_flag(k, FlagKind::HorizonTilt);
        }
    }

    *ms.col_mut(Col::FocusStabRaw) = focus_stab;
    *ms.col_mut(Col::FlickerRaw) = flicker;
    *ms.col_mut(Col::StabRaw) = stab;
    *ms.col_mut(Col::StabConf) = stab_conf;
    ms.flags = flags;
    ms.cut_before = cut_before;
    ms.quantise_in_place(cal);
    let _ = Rect::default();
    ms
}

/// Maximal runs of `true` as inclusive (start, end) pairs.
pub fn runs(v: &[bool]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, &b) in v.iter().enumerate() {
        match (b, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, i - 1));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, v.len() - 1));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_stop_at_cuts() {
        let mut cuts = vec![false; 10];
        cuts[5] = true;
        let w = Windows::new(&cuts);
        assert_eq!(w.at(4, 3), (1, 4));
        assert_eq!(w.at(5, 3), (5, 8));
        assert_eq!(w.at(0, 3), (0, 3));
        assert_eq!(w.at(9, 3), (6, 9));
    }

    #[test]
    fn run_finder() {
        assert_eq!(runs(&[true, true, false, true]), vec![(0, 1), (3, 3)]);
        assert!(runs(&[false, false]).is_empty());
    }
}
