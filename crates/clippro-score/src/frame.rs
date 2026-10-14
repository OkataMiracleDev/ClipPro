//! Stages S1–S4: per-sample scores (ALGORITHMS §6).

use crate::normalize::NormCols;
use crate::settings::{CameraFilter, Settings};
use crate::submetric::SubMetric;
use clippro_measure::util::{lerp, smoothstep};
use clippro_types::{FlagKind, MetricGroup, Tier};
use serde::{Deserialize, Serialize};

/// Per-sample outputs of S1–S4 for one clip.
#[derive(Clone, Debug, Default)]
pub struct FrameScores {
    /// Overall O (after stability emphasis, before modifiers); 0 when no group is available.
    pub overall: Vec<f32>,
    /// Final score F (after tiers and camera filter).
    pub score: Vec<f32>,
    pub force_in: Vec<bool>,
    pub force_out: Vec<bool>,
    pub flags: Vec<u32>,
    pub cut_before: Vec<bool>,
}

/// Breakdown of one sample, for the inspector (SPEC VIE-5).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SampleBreakdown {
    /// (sub-metric, n, s, ω); `s`/`n` are NaN when unavailable.
    pub subs: Vec<(SubMetric, f32, f32, f32)>,
    /// Group score per `MetricGroup` (NaN = unavailable).
    pub groups: Vec<(MetricGroup, f32, f32)>,
    pub overall: f32,
    pub tier_multiplier: f32,
    pub camera_factor: f32,
    pub score: f32,
    pub flags: Vec<FlagKind>,
    pub force_in: bool,
    pub force_out: bool,
}

#[inline]
fn sub_score(sm: SubMetric, x: f32, st: &crate::settings::SubSetting) -> f32 {
    if sm.is_target() {
        let d = (x - st.target) / st.tol.max(1e-6);
        (-0.5 * d * d).exp()
    } else if st.pref > 0.0 {
        x
    } else {
        1.0 - x
    }
}

fn angle_dist(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

/// Camera-movement filter factor `cm` (ALGORITHMS §6.5).
pub fn camera_factor(c: &CameraFilter, nc: &NormCols, k: usize) -> f32 {
    let conf = nc.cam_conf[k];
    let conf = if conf.is_nan() { 0.0 } else { conf };
    let mag = nc.cam_mag[k];
    let c_raw = if mag.is_nan() {
        0.5
    } else if c.static_only {
        (-mag / 0.03).exp()
    } else {
        let mut prod = 1.0;
        if c.radius >= 0.05 {
            let half = lerp(90.0, 5.0, c.radius.clamp(0.0, 1.0));
            let mut d = angle_dist(nc.cam_dir[k], c.direction);
            if c.mirror {
                d = d.min(angle_dist(nc.cam_dir[k], c.direction + 180.0));
            }
            let dm = 1.0 - smoothstep(0.5 * half, half, d);
            let mm = smoothstep(0.02, 0.10, nc.cam_speed[k]);
            prod *= dm * mm;
        }
        if c.zoom.abs() >= 0.05 {
            let zmin = lerp(0.02, 0.30, c.zoom.abs());
            let z = if c.mirror { nc.cam_zr[k].abs() } else { c.zoom.signum() * nc.cam_zr[k] };
            prod *= smoothstep(0.0, zmin, z);
        }
        if c.rotate.abs() >= 0.05 {
            let wmin = lerp(2.0, 30.0, c.rotate.abs()).to_radians();
            let w = if c.mirror { nc.cam_wr[k].abs() } else { c.rotate.signum() * nc.cam_wr[k] };
            prod *= smoothstep(0.0, wmin, w);
        }
        prod
    };
    conf * c_raw + (1.0 - conf) * 0.5
}

/// Scores one sample. Returns (O, F, force_in, force_out, tier multiplier, camera factor).
#[inline]
fn score_sample(
    nc: &NormCols,
    s: &Settings,
    k: usize,
    mut detail: Option<&mut SampleBreakdown>,
) -> (f32, f32, bool, bool, f32, f32) {
    // S1 + S2.
    let mut num_o = 0f32;
    let mut den_o = 0f32;
    for g in MetricGroup::ALL {
        let wg = s.weight(g);
        let (mut num, mut den) = (0f32, 0f32);
        for sm in SubMetric::of_group(g) {
            let st = s.sub(sm);
            let w = st.weight(sm);
            let col = &nc.n[sm as usize];
            let x = if col.is_empty() { f32::NAN } else { col[k] };
            let mut sc = f32::NAN;
            if w > 0.0 && !x.is_nan() {
                sc = sub_score(sm, x, st);
                if sm.confidence_weighted() {
                    let c = if sm == SubMetric::MotionStability { nc.stab_conf[k] } else { nc.cam_conf[k] };
                    let c = if c.is_nan() { 0.0 } else { c.clamp(0.0, 1.0) };
                    sc = c * sc + (1.0 - c) * 0.5;
                }
                num += w * sc;
                den += w;
            }
            if let Some(d) = detail.as_deref_mut() {
                d.subs.push((sm, x, sc, w));
            }
        }
        let gs = if den > 0.0 { num / den } else { f32::NAN };
        if let Some(d) = detail.as_deref_mut() {
            d.groups.push((g, gs, wg));
        }
        if wg > 0.0 && !gs.is_nan() {
            num_o += wg * gs;
            den_o += wg;
        }
    }
    // S3.
    let mut o = if den_o > 0.0 { num_o / den_o } else { 0.0 };
    let stab_col = &nc.n[SubMetric::MotionStability as usize];
    if !stab_col.is_empty() && !stab_col[k].is_nan() {
        o *= 1.0 - 0.5 * s.stability_emphasis * (1.0 - stab_col[k]);
    }
    // S4.
    let flags = nc.flags[k];
    let (mut m, mut fin, mut fout) = (1.0f32, false, false);
    if s.switches.flags {
        for f in FlagKind::ALL {
            if flags & f.bit() != 0 {
                match s.tier(f) {
                    Tier::ForceExclude => fout = true,
                    Tier::ForceInclude => fin = true,
                    t => m *= t.multiplier(),
                }
            }
        }
    }
    let cm = if s.switches.camera_filter && s.camera.enabled { camera_factor(&s.camera, nc, k) } else { 1.0 };
    let base = if s.switches.metrics { o } else { 1.0 };
    let f = (base * m * cm).clamp(0.0, 1.0);
    if let Some(d) = detail {
        d.overall = o;
        d.tier_multiplier = m;
        d.camera_factor = cm;
        d.score = f;
        d.flags = FlagKind::ALL.into_iter().filter(|fl| flags & fl.bit() != 0).collect();
        d.force_in = fin;
        d.force_out = fout && !fin;
    }
    (o, f, fin, fout && !fin, m, cm)
}

struct ActiveSub<'a> {
    sm: SubMetric,
    st: crate::settings::SubSetting,
    w: f32,
    col: &'a [f32],
    conf: Option<&'a [f32]>,
}

/// Fast path of `score_sample` for whole clips: active sub-metrics and flag masks are
/// resolved once. Must stay equivalent to `score_sample` (tested).
pub fn frame_scores(nc: &NormCols, s: &Settings) -> FrameScores {
    let n = nc.len();
    let mut groups: Vec<(f32, Vec<ActiveSub>)> = Vec::new();
    for g in MetricGroup::ALL {
        let wg = s.weight(g);
        if wg <= 0.0 {
            continue;
        }
        let subs: Vec<ActiveSub> = SubMetric::of_group(g)
            .filter_map(|sm| {
                let st = *s.sub(sm);
                let w = st.weight(sm);
                let col = &nc.n[sm as usize];
                if w <= 0.0 || col.is_empty() {
                    return None;
                }
                let conf = sm.confidence_weighted().then(|| {
                    if sm == SubMetric::MotionStability {
                        nc.stab_conf.as_slice()
                    } else {
                        nc.cam_conf.as_slice()
                    }
                });
                Some(ActiveSub { sm, st, w, col, conf })
            })
            .collect();
        if !subs.is_empty() {
            groups.push((wg, subs));
        }
    }
    let (mut excl_mask, mut incl_mask) = (0u32, 0u32);
    let mut mult: Vec<(u32, f32)> = Vec::new();
    if s.switches.flags {
        for f in FlagKind::ALL {
            match s.tier(f) {
                Tier::ForceExclude => excl_mask |= f.bit(),
                Tier::ForceInclude => incl_mask |= f.bit(),
                Tier::Neutral => {}
                t => mult.push((f.bit(), t.multiplier())),
            }
        }
    }
    let stab_col = &nc.n[SubMetric::MotionStability as usize];
    let use_cam = s.switches.camera_filter && s.camera.enabled;
    let mut out = FrameScores {
        overall: Vec::with_capacity(n),
        score: Vec::with_capacity(n),
        force_in: Vec::with_capacity(n),
        force_out: Vec::with_capacity(n),
        flags: nc.flags.clone(),
        cut_before: nc.cut_before.clone(),
    };
    for k in 0..n {
        let (mut num_o, mut den_o) = (0f32, 0f32);
        for (wg, subs) in &groups {
            let (mut num, mut den) = (0f32, 0f32);
            for a in subs {
                let x = a.col[k];
                if x.is_nan() {
                    continue;
                }
                let mut sc = sub_score(a.sm, x, &a.st);
                if let Some(c) = a.conf {
                    let c = if c[k].is_nan() { 0.0 } else { c[k].clamp(0.0, 1.0) };
                    sc = c * sc + (1.0 - c) * 0.5;
                }
                num += a.w * sc;
                den += a.w;
            }
            if den > 0.0 {
                num_o += wg * (num / den);
                den_o += wg;
            }
        }
        let mut o = if den_o > 0.0 { num_o / den_o } else { 0.0 };
        if !stab_col.is_empty() && !stab_col[k].is_nan() {
            o *= 1.0 - 0.5 * s.stability_emphasis * (1.0 - stab_col[k]);
        }
        let fl = nc.flags[k];
        let mut m = 1.0f32;
        for (bit, f) in &mult {
            if fl & bit != 0 {
                m *= f;
            }
        }
        let fin = fl & incl_mask != 0;
        let fout = fl & excl_mask != 0 && !fin;
        let cm = if use_cam { camera_factor(&s.camera, nc, k) } else { 1.0 };
        let base = if s.switches.metrics { o } else { 1.0 };
        out.overall.push(o);
        out.score.push((base * m * cm).clamp(0.0, 1.0));
        out.force_in.push(fin);
        out.force_out.push(fout);
    }
    out
}

pub fn explain_sample(nc: &NormCols, s: &Settings, k: usize) -> SampleBreakdown {
    let mut d = SampleBreakdown::default();
    if k < nc.len() {
        score_sample(nc, s, k, Some(&mut d));
    }
    d
}
