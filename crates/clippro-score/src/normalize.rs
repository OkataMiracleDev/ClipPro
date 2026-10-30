//! Maps stored measures to sub-metric values `n` (ALGORITHMS §3) and evaluates score-time
//! flags (Low light, §2.9). Results depend only on measurements, calibration, the clip's
//! colour profile and centre weighting, so they are cached per clip.

use crate::submetric::SubMetric;
use clippro_measure::calibration::Calibration;
use clippro_measure::columns::{Col, MeasurementSet};
use clippro_measure::util::{logistic, smoothstep};
use clippro_types::{ColourProfile, FlagKind, MetricGroup};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Everything the engine needs about one clip.
#[derive(Clone, Debug)]
pub struct ClipInput {
    pub id: clippro_types::ClipId,
    /// Changes whenever the measurements change (e.g. the analysis run id).
    pub version: u64,
    pub ms: Arc<MeasurementSet>,
    pub duration_us: i64,
    pub profile: ColourProfile,
    /// Extra normalised columns from audio / AI passes, aligned to the samples.
    pub extra: BTreeMap<SubMetric, Arc<Vec<f32>>>,
    /// Extra per-sample flags (audio, AI), OR-ed with stored flags.
    pub extra_flags: Option<Arc<Vec<u32>>>,
    /// Sentence boundaries from the transcript (ALGORITHMS §8.1): where a select may start and
    /// where it may end, in µs. Used when `Settings::snap_to_sentences` is on.
    pub snaps: Option<Arc<(Vec<i64>, Vec<i64>)>>,
}

/// Normalised columns for one clip.
#[derive(Clone, Debug, Default)]
pub struct NormCols {
    /// Indexed by `SubMetric as usize`; empty = unavailable for this clip.
    pub n: Vec<Vec<f32>>,
    pub cam_conf: Vec<f32>,
    pub stab_conf: Vec<f32>,
    pub flags: Vec<u32>,
    pub cut_before: Vec<bool>,
    // Camera-filter inputs.
    pub cam_mag: Vec<f32>,
    pub cam_dir: Vec<f32>,
    pub cam_speed: Vec<f32>,
    pub cam_zr: Vec<f32>,
    pub cam_wr: Vec<f32>,
}

impl NormCols {
    pub fn len(&self) -> usize {
        self.flags.len()
    }

    pub fn is_empty(&self) -> bool {
        self.flags.is_empty()
    }

    pub fn has(&self, sm: SubMetric) -> bool {
        !self.n[sm as usize].is_empty()
    }
}

fn low_light_thresholds(cal: &Calibration, profile: ColourProfile) -> (f32, f32) {
    let e = &cal.exposure;
    if profile.is_log() {
        (e.low_light_p50_log, e.low_light_p95_log)
    } else {
        (e.low_light_p50, e.low_light_p95)
    }
}

/// Grading flexibility (ALGORITHMS §3) from stored percentiles.
pub fn grading_flexibility(ms: &MeasurementSet, k: usize, cal: &Calibration, log: bool) -> f32 {
    let e = &cal.exposure;
    let h_white = 1.0 - smoothstep(0.002, 0.05, ms.get(Col::ClipWhite, k));
    let h_black = 1.0 - smoothstep(0.005, 0.10, ms.get(Col::ClipBlack, k));
    let (a, b) = if log { (e.gf_spread_lo_log, e.gf_spread_hi_log) } else { (e.gf_spread_lo, e.gf_spread_hi) };
    let spread = smoothstep(a, b, ms.get(Col::P99, k) - ms.get(Col::P01, k));
    let mid = 1.0 - smoothstep(0.25, 0.45, (ms.get(Col::P50, k) - 0.45).abs());
    h_white * h_black.powf(0.7) * (0.5 + 0.5 * spread) * (0.6 + 0.4 * mid)
}

pub fn normalise(input: &ClipInput, centre_weighting: f32, cal: &Calibration) -> NormCols {
    let ms = &*input.ms;
    let n = ms.len();
    let mut out = NormCols { n: vec![Vec::new(); SubMetric::ALL.len()], ..Default::default() };
    if n == 0 {
        return out;
    }
    let f = &cal.focus;
    let m = &cal.motion;
    let log = input.profile.is_log();
    let col = |c: Col| ms.col(c);
    let map = |c: Col, func: &dyn Fn(f32) -> f32| -> Vec<f32> { col(c).iter().map(|x| func(*x)).collect() };

    // Focus.
    let full = col(Col::SharpFull);
    let centre = col(Col::SharpCentre);
    let cw = centre_weighting.clamp(0.0, 1.0);
    out.n[SubMetric::Sharpness as usize] = (0..n)
        .map(|k| {
            let s = full[k] + (centre[k] - full[k]) * cw;
            if s.is_nan() {
                f32::NAN
            } else {
                logistic((s - f.sharp_s0) / f.sharp_k)
            }
        })
        .collect();
    out.n[SubMetric::Detail as usize] = col(Col::DetailRaw).to_vec();
    out.n[SubMetric::FocusStability as usize] = col(Col::FocusStabRaw).to_vec();

    // Exposure.
    out.n[SubMetric::GradingFlexibility as usize] =
        (0..n).map(|k| grading_flexibility(ms, k, cal, log)).collect();
    out.n[SubMetric::Flicker as usize] = col(Col::FlickerRaw).to_vec();
    // Luma is target-type on display-referred p50; log profiles go through the profile
    // transform (ALGORITHMS §3).
    out.n[SubMetric::Luma as usize] =
        col(Col::P50).iter().map(|p| crate::colour::to_display(input.profile, *p)).collect();
    let (p95, p05) = (col(Col::P95), col(Col::P05));
    out.n[SubMetric::Contrast as usize] = (0..n).map(|k| ((p95[k] - p05[k]) / 0.9).clamp(0.0, 1.0)).collect();
    out.n[SubMetric::Noise as usize] = map(Col::NoiseSigma, &|x| smoothstep(0.003, 0.03, x));
    let wb = col(Col::WbCast);
    if wb.iter().any(|x| !x.is_nan()) {
        out.n[SubMetric::WbCast as usize] = wb.iter().map(|x| smoothstep(0.01, 0.08, *x)).collect();
    }

    // Motion (NaN-preserving maps).
    let keep_nan = |x: f32, v: f32| if x.is_nan() { f32::NAN } else { v };
    out.n[SubMetric::CameraMotion as usize] =
        map(Col::CamMag, &|x| keep_nan(x, 1.0 - (-x / m.cam_scale).exp()));
    out.n[SubMetric::SubjectMotion as usize] =
        map(Col::SubjRaw, &|x| keep_nan(x, 1.0 - (-x / m.subj_scale).exp()));
    out.n[SubMetric::MotionStability as usize] = col(Col::StabRaw).to_vec();
    out.cam_conf = col(Col::CamConf).to_vec();
    out.stab_conf = col(Col::StabConf).to_vec();
    out.cam_mag = col(Col::CamMag).to_vec();
    out.cam_dir = col(Col::CamDir).to_vec();
    let (ux, uy) = (col(Col::CamUx), col(Col::CamUy));
    out.cam_speed = (0..n).map(|k| ux[k].hypot(uy[k])).collect();
    out.cam_zr = col(Col::CamZr).to_vec();
    out.cam_wr = col(Col::CamWr).to_vec();
    // A clip analysed without the Motion group has all-NaN motion columns: mark unavailable.
    for sm in [SubMetric::CameraMotion, SubMetric::SubjectMotion, SubMetric::MotionStability] {
        if out.n[sm as usize].iter().all(|x| x.is_nan()) {
            out.n[sm as usize].clear();
        }
    }
    for sm in [SubMetric::Sharpness, SubMetric::Detail, SubMetric::FocusStability] {
        if out.n[sm as usize].iter().all(|x| x.is_nan()) {
            out.n[sm as usize].clear();
        }
    }

    // Audio / content columns supplied by other passes.
    for (sm, v) in &input.extra {
        if v.len() == n {
            out.n[*sm as usize] = v.to_vec();
        }
    }

    // Groups that weren't analysed are unavailable.
    for (g, subs) in [
        (MetricGroup::Focus, &[SubMetric::Sharpness, SubMetric::Detail, SubMetric::FocusStability][..]),
        (
            MetricGroup::Exposure,
            &[SubMetric::GradingFlexibility, SubMetric::Flicker, SubMetric::Luma, SubMetric::Contrast, SubMetric::Noise, SubMetric::WbCast][..],
        ),
        (MetricGroup::Motion, &[SubMetric::CameraMotion, SubMetric::SubjectMotion, SubMetric::MotionStability][..]),
    ] {
        if !ms.groups.contains(g) {
            for sm in subs {
                out.n[*sm as usize].clear();
            }
        }
    }

    // Flags: stored | extra | score-time Low light.
    let (t50, t95) = low_light_thresholds(cal, input.profile);
    let ratio = cal.exposure.low_light_noise_ratio;
    let (p50, sigma) = (col(Col::P50), col(Col::NoiseSigma));
    out.flags = (0..n)
        .map(|k| {
            let mut fl = ms.flags.get(k).copied().unwrap_or(0) & !FlagKind::LowLight.bit();
            if let Some(extra) = &input.extra_flags {
                fl |= extra.get(k).copied().unwrap_or(0);
            }
            let low = p50[k] < t50
                && p95[k] < t95
                && (sigma[k] / p50[k].max(0.01) > ratio || p50[k] < t50 / 2.0);
            if low {
                fl |= FlagKind::LowLight.bit();
            }
            fl
        })
        .collect();
    out.cut_before = if ms.cut_before.len() == n { ms.cut_before.clone() } else { vec![false; n] };
    out
}
