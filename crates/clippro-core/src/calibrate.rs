//! Calibration sweep (ALGORITHMS §10 step 1, PLAN P2.9): controlled synthetic ramps run through
//! the real measurement, temporal and normalisation code; the documented anchors are checked and
//! constants proposed. Proposals are reported, never applied.

use clippro_measure::calibration::Calibration;
use clippro_measure::columns::{Col, MeasurementSet};
use clippro_measure::image::{working_dims, Image, Rect};
use clippro_measure::sample::{ClipContext, SampleInput, SampleMeasurer};
use clippro_measure::temporal::temporal_pass;
use clippro_score::normalize::{normalise, ClipInput};
use clippro_score::SubMetric;
use clippro_testgen::scene::{Frame, Pose, Texture};
use clippro_types::ClipId;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::sync::Arc;

const FPS: f64 = 25.0;
const RATE: f64 = 4.0;
/// Frame width in texture pixels (1080p renders one texture pixel per frame pixel).
const FW: f32 = 1920.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub name: String,
    pub measured: f32,
    /// e.g. "≥ 0.85", "0.35–0.60".
    pub want: String,
    pub pass: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SweepPoint {
    pub series: String,
    pub x: f32,
    pub raw: f32,
    pub n: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub key: String,
    pub current: f64,
    pub proposed: f64,
    pub why: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SweepReport {
    pub calibration_version: u32,
    pub anchors: Vec<Anchor>,
    pub points: Vec<SweepPoint>,
    pub proposals: Vec<Proposal>,
}

impl SweepReport {
    pub fn passed(&self) -> bool {
        self.anchors.iter().all(|a| a.pass)
    }
}

/// Median of the middle half of a series (skips the temporal windows' edges), ignoring NaN.
fn mid_median(v: &[f32]) -> f32 {
    let n = v.len();
    let mut m: Vec<f32> = v[n / 4..(3 * n / 4).max(n / 4 + 1).min(n)].iter().copied().filter(|x| x.is_finite()).collect();
    if m.is_empty() {
        return f32::NAN;
    }
    m.sort_by(f32::total_cmp);
    m[m.len() / 2]
}

fn hash_noise(i: usize, seed: u64) -> f32 {
    let mut h = (i as u64 ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 31;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 29;
    (h >> 40) as f32 / (1u64 << 24) as f32 - 0.5
}

/// Luma plane of a frame, with sensor-like noise (uniform, σ ≈ `sigma`).
fn luma(f: &Frame, sigma: f32, seed: u64) -> Image {
    let k = sigma * 12f32.sqrt();
    Image { w: f.w, h: f.h, data: f.y.iter().enumerate().map(|(i, v)| (v + k * hash_noise(i, seed)).clamp(0.0, 1.0)).collect() }
}

/// Separable Gaussian blur of a plane, in parallel over rows.
fn blur(img: &Image, sigma: f32) -> Image {
    if sigma <= 0.05 {
        return img.clone();
    }
    let r = (3.0 * sigma).ceil() as i64;
    let k: Vec<f32> = (-r..=r).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let ks: f32 = k.iter().sum();
    let (w, h) = (img.w, img.h);
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let pass = |src: &[f32], horiz: bool| -> Vec<f32> {
        let mut out = vec![0f32; w * h];
        let rows_per = h.div_ceil(threads);
        std::thread::scope(|s| {
            for (ci, chunk) in out.chunks_mut(rows_per * w).enumerate() {
                let k = &k;
                s.spawn(move || {
                    for (ry, row) in chunk.chunks_mut(w).enumerate() {
                        let y = ci * rows_per + ry;
                        for (x, o) in row.iter_mut().enumerate() {
                            let mut acc = 0.0;
                            for (j, kv) in k.iter().enumerate() {
                                let d = j as i64 - r;
                                let (sx, sy) = if horiz { ((x as i64 + d).clamp(0, w as i64 - 1) as usize, y) } else { (x, (y as i64 + d).clamp(0, h as i64 - 1) as usize) };
                                acc += src[sy * w + sx] * kv;
                            }
                            *o = acc / ks;
                        }
                    }
                });
            }
        });
        out
    };
    Image { w, h, data: pass(&pass(&img.data, true), false) }
}

fn to_working(img: &Image) -> Image {
    let (ww, wh) = working_dims(img.w, img.h, 960);
    if (ww, wh) == (img.w, img.h) {
        return img.clone();
    }
    img.resize_area(Rect::full(img.w, img.h), ww, wh)
}

fn measure(frames: &[(Image, Option<Image>)], motion: bool, cal: &Calibration) -> MeasurementSet {
    let (w, h) = (frames[0].0.w, frames[0].0.h);
    let ctx = ClipContext { fps: FPS, active: Rect::full(w, h), working: (w, h), seed_base: 7, pair_sampling: motion, motion };
    let mut m = SampleMeasurer::new(ctx.clone());
    let raws: Vec<_> = frames
        .iter()
        .enumerate()
        .map(|(i, (f, p))| m.measure(SampleInput { index: i, time_us: (i as f64 / RATE * 1e6) as i64, frame: f, pair: p.as_ref(), chroma: None }))
        .collect();
    temporal_pass(&raws, &ctx, RATE, cal)
}

fn normalised(ms: MeasurementSet, sm: SubMetric, cal: &Calibration) -> Vec<f32> {
    let n = ms.len();
    let input = ClipInput { id: ClipId(1), version: 1, ms: Arc::new(ms), duration_us: (n as f64 / RATE * 1e6) as i64, profile: Default::default(), extra: Default::default(), extra_flags: None, snaps: None };
    normalise(&input, 0.0, cal).n.get(sm as usize).cloned().unwrap_or_default()
}

fn logit(p: f64) -> f64 {
    (p / (1.0 - p)).ln()
}

/// Runs the sweep with `cal` (normally the committed constants).
pub fn sweep(cal: &Calibration) -> SweepReport {
    let mut anchors = Vec::new();
    let mut points = Vec::new();
    let mut proposals = Vec::new();
    let mut anchor = |name: &str, v: f32, want: &str, pass: bool| anchors.push(Anchor { name: name.into(), measured: v, want: want.into(), pass });

    // Sharpness: blur σ (1080p pixels) at 1080p and 4K sources.
    let sigmas = [0.0f32, 0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0];
    let mut raw_15 = Vec::new();
    for (label, w, h, size, k) in [("1080p", 1920, 1080, 2048usize, 1.0f32), ("4K", 3840, 2160, 4096, 2.0)] {
        let tex = Texture::new(size, 11);
        let c = size as f32 / 2.0;
        let base = luma(&Frame::render(&tex, w, h, Pose { cx: c, cy: c, zoom: 1.0, rot: 0.0 }, 1.0), 0.0, 0);
        for &s in &sigmas {
            let b = blur(&base, s * k);
            let frames: Vec<(Image, Option<Image>)> = (0..6u64)
                .map(|i| {
                    let noisy = Image { w: b.w, h: b.h, data: b.data.iter().enumerate().map(|(j, v)| (v + 0.004 * 12f32.sqrt() * hash_noise(j, i + 1)).clamp(0.0, 1.0)).collect() };
                    (to_working(&noisy), None)
                })
                .collect();
            let ms = measure(&frames, false, cal);
            let raw = mid_median(ms.col(Col::SharpFull));
            let n = mid_median(&normalised(ms, SubMetric::Sharpness, cal));
            points.push(SweepPoint { series: format!("Sharpness {label}"), x: s, raw, n });
            if s == 1.5 {
                raw_15.push(raw);
            }
            match s {
                0.0 => anchor(&format!("Sharpness {label}, σ 0"), n, "≥ 0.85", n >= 0.85),
                1.5 => anchor(&format!("Sharpness {label}, σ 1.5"), n, "0.35–0.60", (0.35..=0.60).contains(&n)),
                4.0 => anchor(&format!("Sharpness {label}, σ 4"), n, "≤ 0.15", n <= 0.15),
                _ => {}
            }
        }
    }
    // Keep the width; centre the logistic so σ 1.5 (1080p and 4K on average) lands mid-band.
    let mean15 = raw_15.iter().map(|v| *v as f64).sum::<f64>() / raw_15.len().max(1) as f64;
    if mean15.is_finite() && !raw_15.is_empty() {
        let s0 = mean15 - logit(0.475) * cal.focus.sharp_k as f64;
        proposals.push(Proposal { key: "focus.sharp_s0".into(), current: cal.focus.sharp_s0 as f64, proposed: s0, why: "σ 1.5 → 0.475 (mean of 1080p and 4K), width unchanged".into() });
    }

    // Motion: camera paths over the texture, at working resolution, with pair sampling.
    let tex = Texture::new(2048, 11);
    let path = |t: f32, kind: &str| -> (f32, f32) {
        match kind {
            "pan" => (0.25 * t, 0.0),
            "gimbal" => (0.08 * (std::f32::consts::TAU * 0.2 * t).sin(), 0.03 * (std::f32::consts::TAU * 0.13 * t).sin()),
            "jitter" => {
                // Velocity RMS 0.1 fw/s split over x and y, three components each.
                let comps = [3.1f32, 4.7, 6.3];
                let v = 0.1 / (2.0f32 * comps.len() as f32).sqrt();
                let mut x = 0.0;
                let mut y = 0.0;
                for (i, f) in comps.iter().enumerate() {
                    let a = v * 2f32.sqrt() / (std::f32::consts::TAU * f);
                    x += a * (std::f32::consts::TAU * f * t + i as f32).sin();
                    y += a * (std::f32::consts::TAU * f * 1.07 * t + 2.0 * i as f32).sin();
                }
                (x, y)
            }
            _ => (0.0, 0.0),
        }
    };
    let mut stab = Vec::new();
    for kind in ["tripod", "gimbal", "jitter", "pan"] {
        let frame_at = |t: f32, seed: u64| {
            let (x, y) = path(t, kind);
            let f = Frame::render(&tex, 960, 540, Pose { cx: 1024.0 + x * FW, cy: 1024.0 + y * FW, zoom: 1.0, rot: 0.0 }, 2.0);
            luma(&f, 0.003, seed)
        };
        let frames: Vec<(Image, Option<Image>)> = (0..32u64)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                (frame_at(t, 2 * i + 1), Some(frame_at(t + 1.0 / FPS as f32, 2 * i + 2)))
            })
            .collect();
        let ms = measure(&frames, true, cal);
        let cam_raw = mid_median(ms.col(Col::CamMag));
        let s = mid_median(&normalised(ms.clone(), SubMetric::MotionStability, cal));
        let cam = mid_median(&normalised(ms, SubMetric::CameraMotion, cal));
        points.push(SweepPoint { series: "Motion stability".into(), x: ["tripod", "gimbal", "jitter", "pan"].iter().position(|k| *k == kind).unwrap_or(0) as f32, raw: s, n: s });
        stab.push((kind, s));
        match kind {
            "tripod" => anchor("Motion stability, tripod", s, "≥ 0.90", s >= 0.9),
            "gimbal" => anchor("Motion stability, gimbal walk (0.2 Hz)", s, "≥ 0.80", s >= 0.8),
            "jitter" => anchor("Motion stability, handheld jitter 0.1 fw/s", s, "≤ 0.35", s <= 0.35),
            _ => {
                anchor("Camera motion, 0.25 fw/s pan", cam, "0.63 ± 0.08", (cam - 0.63).abs() <= 0.08);
                points.push(SweepPoint { series: "Camera motion (pan 0.25 fw/s)".into(), x: 0.25, raw: cam_raw, n: cam });
                if cam_raw.is_finite() && cam_raw > 0.0 {
                    proposals.push(Proposal { key: "motion.cam_scale".into(), current: cal.motion.cam_scale as f64, proposed: cam_raw as f64 / -(1.0f64 - 0.63).ln(), why: "0.25 fw/s pan → 0.63".into() });
                }
            }
        }
    }
    // Stability is exp(-J / stab_scale): recover each J and propose a scale between the bounds.
    let sc = cal.motion.stab_scale as f64;
    let j = |k: &str| stab.iter().find(|s| s.0 == k).map(|s| -sc * (s.1.clamp(1e-6, 1.0) as f64).ln()).unwrap_or(f64::NAN);
    let lo = (j("tripod") / -(0.9f64).ln()).max(j("gimbal") / -(0.8f64).ln());
    let hi = j("jitter") / -(0.35f64).ln();
    if lo.is_finite() && hi.is_finite() && lo > 0.0 {
        let proposed = if lo < hi { (lo * hi).sqrt() } else { hi };
        proposals.push(Proposal {
            key: "motion.stab_scale".into(),
            current: sc,
            proposed,
            why: if lo < hi { format!("any value in {lo:.4}–{hi:.4} meets all three stability anchors") } else { "no single scale meets all three anchors; the jitter bound wins".into() },
        });
    }
    SweepReport { calibration_version: cal.version, anchors, points, proposals }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A self-contained HTML report of the sweep.
pub fn report_html(r: &SweepReport, generated: &str) -> String {
    let mut h = String::new();
    let _ = write!(
        h,
        "<!doctype html><meta charset=utf-8><title>ClipPro calibration sweep</title><style>body{{font:14px/1.5 system-ui,sans-serif;max-width:860px;margin:32px auto;padding:0 16px;color:#1d1d1f;background:#fff}}h1{{font-size:22px}}h2{{font-size:16px;margin-top:28px}}table{{border-collapse:collapse;width:100%;font-variant-numeric:tabular-nums}}th,td{{text-align:left;padding:5px 10px 5px 0;border-bottom:1px solid #e5e5ea}}.pass{{color:#1a7f37}}.fail{{color:#c62828;font-weight:600}}pre{{background:#f5f5f7;padding:12px;border-radius:6px}}</style><h1>Calibration sweep</h1><p>Calibration version {} · generated {} · {} of {} anchors pass. Proposed constants are suggestions; nothing is applied.</p>",
        r.calibration_version,
        esc(generated),
        r.anchors.iter().filter(|a| a.pass).count(),
        r.anchors.len()
    );
    h.push_str("<h2>Anchors (ALGORITHMS §10)</h2><table><tr><th>Anchor</th><th>Measured</th><th>Wanted</th><th></th></tr>");
    for a in &r.anchors {
        let _ = write!(h, "<tr><td>{}</td><td>{:.3}</td><td>{}</td><td class={}>{}</td></tr>", esc(&a.name), a.measured, esc(&a.want), if a.pass { "pass" } else { "fail" }, if a.pass { "pass" } else { "FAIL" });
    }
    h.push_str("</table><h2>Sweep</h2><table><tr><th>Series</th><th>x</th><th>Raw</th><th>Normalised</th></tr>");
    for p in &r.points {
        let _ = write!(h, "<tr><td>{}</td><td>{}</td><td>{:.4}</td><td>{:.3}</td></tr>", esc(&p.series), p.x, p.raw, p.n);
    }
    h.push_str("</table><h2>Proposed constants</h2><table><tr><th>Key</th><th>Current</th><th>Proposed</th><th>Basis</th></tr>");
    for p in &r.proposals {
        let _ = write!(h, "<tr><td><code>{}</code></td><td>{:.4}</td><td>{:.4}</td><td>{}</td></tr>", esc(&p.key), p.current, p.proposed, esc(&p.why));
    }
    h.push_str("</table><p>To adopt a value, edit <code>calibration/calibration.toml</code> and bump its <code>version</code>.</p>");
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(mid_median(&[9.0, 1.0, 2.0, 3.0, 9.0, 9.0, 9.0, 9.0]), 9.0);
        assert!(mid_median(&[f32::NAN; 4]).is_nan());
        let img = Image::from_fn(64, 32, |x, _| if x < 32 { 0.0 } else { 1.0 });
        let b = blur(&img, 2.0);
        assert!(b.at(31, 10) > 0.3 && b.at(31, 10) < 0.5 && b.at(32, 10) > 0.5);
        assert!((b.data.iter().sum::<f32>() - img.data.iter().sum::<f32>()).abs() < 1.0);
    }
}
