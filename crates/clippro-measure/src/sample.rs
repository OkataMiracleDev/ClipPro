//! Per-sample measurement (ALGORITHMS §1): combines the spatial and motion kernels with the
//! previous sample's state into one `SampleRaw` row.

use crate::columns::{Col, NCOLS};
use crate::image::{Image, Rect};
use crate::motion::{estimate, MotionEstimate, Pyramid};
use crate::spatial::{chi2, colour_bars, mad, phash, solid_colour, spatial, wb_cast};
use crate::util::hash2;
use serde::{Deserialize, Serialize};

/// Clip-level constants fixed before measuring.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClipContext {
    pub fps: f64,
    pub active: Rect,
    pub working: (usize, usize),
    /// Seed base for RANSAC (derived from the media fingerprint).
    pub seed_base: u64,
    pub pair_sampling: bool,
    /// Run motion estimation (the Motion compute group; by far the heaviest).
    pub motion: bool,
}

/// One sample's inputs.
pub struct SampleInput<'a> {
    pub index: usize,
    pub time_us: i64,
    /// Full working image (≤ 960 px).
    pub frame: &'a Image,
    /// The next frame (pair sampling), working image.
    pub pair: Option<&'a Image>,
    /// Cb/Cr thumbnails (32×18) of the active area, if the source has chroma.
    pub chroma: Option<(Image, Image)>,
}

/// Raw measurements of one sample (temporal columns are NaN until the temporal pass).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SampleRaw {
    pub index: usize,
    pub time_us: i64,
    pub v: Vec<f32>,
    pub tiles: Vec<f32>,
    pub mvs: Vec<[i8; 2]>,
    pub mv_grid: (u8, u8),
    pub phash: u64,
    pub hist64: Vec<f32>,
    /// 0 none, 1 black, 2 white, 3 colour.
    pub solid: u8,
    pub bars: bool,
}

impl SampleRaw {
    pub fn get(&self, c: Col) -> f32 {
        self.v[c as usize]
    }
}

struct Prev {
    pyr: Pyramid,
    thumb: Image,
    hist: [f32; 64],
    time_us: i64,
}

/// Stateful per-clip (or per-section) measurer. Feed samples in order.
pub struct SampleMeasurer {
    pub ctx: ClipContext,
    prev: Option<Prev>,
}

pub const THUMB_W: usize = 64;
pub const THUMB_H: usize = 36;

fn normalised(e: &MotionEstimate, dt: f64) -> [f32; 4] {
    let w0 = e.w0.max(1) as f64;
    let dt = dt.max(1e-6);
    let ux = (e.tx as f64 / w0 / dt) as f32;
    let uy = (e.ty as f64 / w0 / dt) as f32;
    let zr = ((e.scale.max(1e-6) as f64).ln() / dt) as f32;
    // Image coordinates are y-down; positive `rot` is clockwise on screen, so negate for CCW.
    let wr = (-(e.rot as f64) / dt) as f32;
    [ux, uy, zr, wr]
}

impl SampleMeasurer {
    pub fn new(ctx: ClipContext) -> Self {
        SampleMeasurer { ctx, prev: None }
    }

    /// Forgets the previous sample (e.g. at a section start without overlap).
    pub fn reset(&mut self) {
        self.prev = None;
    }

    pub fn measure(&mut self, input: SampleInput) -> SampleRaw {
        let mut v = vec![f32::NAN; NCOLS];
        let ctx = &self.ctx;
        let a = crop_active(input.frame, ctx.active);
        let sp = spatial(&a);
        let e = &sp.exposure;
        let thumb = a.resize_area(Rect::full(a.w, a.h), THUMB_W, THUMB_H);
        let pyr = Pyramid::build(&a);
        let mut set = |c: Col, x: f32| v[c as usize] = x;

        set(Col::NoiseSigma, sp.noise_sigma);
        set(Col::SharpFull, sp.sharp_full);
        set(Col::SharpCentre, sp.sharp_centre);
        set(Col::Tenengrad, sp.tenengrad);
        set(Col::DetailRaw, sp.detail_raw);
        set(Col::Mean, e.mean);
        set(Col::Std, e.std);
        set(Col::P01, e.p01);
        set(Col::P05, e.p05);
        set(Col::P50, e.p50);
        set(Col::P95, e.p95);
        set(Col::P99, e.p99);
        set(Col::ClipBlack, e.clip_black);
        set(Col::ClipWhite, e.clip_white);
        set(Col::NearBlack, e.near_black);
        set(Col::NearWhite, e.near_white);
        set(Col::RowBand, e.row_band);

        // Chroma-derived measures.
        let luma32 = thumb.resize_area(Rect::full(THUMB_W, THUMB_H), 32, 18);
        let chroma_ref = input.chroma.as_ref().map(|(b, r)| (b, r));
        if let Some((cb, cr)) = chroma_ref {
            set(Col::WbCast, wb_cast(&luma32, cb, cr));
        }
        let solid = solid_colour(e.std, e.mean, chroma_ref);
        let bars = colour_bars(&thumb, chroma_ref);

        // Pair motion (instantaneous) and sample motion (since the previous sample).
        let frame_dt = 1.0 / ctx.fps.max(1.0);
        let seed = hash2(ctx.seed_base, input.index as u64);
        let mut pair_est = None;
        if let Some(pair) = input.pair {
            let pa = crop_active(pair, ctx.active);
            let pthumb = pa.resize_area(Rect::full(pa.w, pa.h), THUMB_W, THUMB_H);
            let pmean = pa.mean() as f32;
            set(Col::MadPair, mad(&thumb, &pthumb));
            set(Col::PairDmean, (e.mean - pmean).abs() / e.mean.max(0.02));
            if ctx.motion {
                let ppyr = Pyramid::build(&pa);
                pair_est = Some(estimate(&pyr, &ppyr, sp.noise_sigma, seed.wrapping_mul(2)));
            }
        }
        let mut avg_est = None;
        let mut avg_dt = 0.0;
        if let Some(prev) = &self.prev {
            avg_dt = ((input.time_us - prev.time_us) as f64 / 1e6).max(frame_dt);
            if ctx.motion {
                avg_est = Some(estimate(&prev.pyr, &pyr, sp.noise_sigma, seed.wrapping_mul(2) + 1));
            }
            set(Col::MadPrev, mad(&thumb, &prev.thumb));
            set(Col::Chi2Prev, chi2(&e.hist64, &prev.hist));
        }
        let pair_n = pair_est.as_ref().map(|p| normalised(p, frame_dt));
        let avg_n = avg_est.as_ref().map(|p| normalised(p, avg_dt));
        if let (Some(n), Some(est)) = (avg_n, &avg_est) {
            set(Col::AvgUx, n[0]);
            set(Col::AvgUy, n[1]);
            set(Col::AvgZr, n[2]);
            set(Col::AvgWr, n[3]);
            set(Col::AvgConf, est.conf);
        }
        // Primary camera estimate: the pair if available, otherwise the sample interval.
        let primary = match (&pair_est, pair_n, &avg_est, avg_n) {
            (Some(e), Some(n), _, _) => Some((e, n, frame_dt)),
            (_, _, Some(e), Some(n)) => Some((e, n, avg_dt)),
            _ => None,
        };
        let mut mvs = Vec::new();
        let mut mv_grid = (0u8, 0u8);
        if let Some((est, n, dt)) = primary {
            let [ux, uy, zr, wr] = n;
            set(Col::CamUx, ux);
            set(Col::CamUy, uy);
            set(Col::CamZr, zr);
            set(Col::CamWr, wr);
            let umag = ux.hypot(uy);
            set(Col::CamMag, (umag * umag + (0.5 * zr).powi(2) + (0.5 * wr).powi(2)).sqrt());
            set(Col::CamConf, est.conf);
            let dir = (uy).atan2(-ux).to_degrees();
            set(Col::CamDir, if dir < 0.0 { dir + 360.0 } else { dir });
            let w0 = est.w0.max(1) as f64;
            set(
                Col::SubjRaw,
                est.moving_frac * (est.moving_median_px as f64 / w0 / dt.max(1e-6)) as f32,
            );
            set(Col::ResidualRms, est.residual_rms);
            // Directional blur ratio along the image-motion direction.
            if umag >= 0.01 {
                let (ex, ey) = ((ux / umag) as f64, (uy / umag) as f64);
                let par = ex * ex * sp.jxx + 2.0 * ex * ey * sp.jxy + ey * ey * sp.jyy;
                let perp = ey * ey * sp.jxx - 2.0 * ex * ey * sp.jxy + ex * ex * sp.jyy;
                set(Col::BlurDirRatio, (par / (perp + 1e-9)) as f32);
            }
            set(Col::PxPerFrame, umag * a.w as f32 * frame_dt as f32);
            mv_grid = (est.grid.0 as u8, est.grid.1 as u8);
            mvs = est
                .vectors
                .iter()
                .map(|v| {
                    if v[0].is_nan() {
                        [i8::MIN, i8::MIN]
                    } else {
                        [v[0].round().clamp(-127.0, 127.0) as i8, v[1].round().clamp(-127.0, 127.0) as i8]
                    }
                })
                .collect();
        }
        if let (Some(p), Some(q)) = (pair_n, avg_n) {
            let du = (p[0] - q[0]).hypot(p[1] - q[1]);
            set(Col::ShakeIa, du + 0.5 * ((p[3] - q[3]).abs() + (p[2] - q[2]).abs()));
        } else if pair_n.is_none() && avg_n.is_some() {
            set(Col::ShakeIa, 0.0);
        }
        set(Col::TiltDeg, horizon_tilt(pyr.l0()));

        let ph = phash(&thumb);
        let out = SampleRaw {
            index: input.index,
            time_us: input.time_us,
            v,
            tiles: sp.tiles_log.clone(),
            mvs,
            mv_grid,
            phash: ph,
            hist64: e.hist64.to_vec(),
            solid,
            bars,
        };
        self.prev = Some(Prev { pyr, thumb, hist: e.hist64, time_us: input.time_us });
        out
    }
}

fn crop_active(frame: &Image, r: Rect) -> Image {
    if r.w == 0 || r.h == 0 || (r.x == 0 && r.y == 0 && r.w == frame.w && r.h == frame.h) {
        frame.clone()
    } else {
        frame.crop(Rect { x: r.x, y: r.y, w: r.w.min(frame.w - r.x), h: r.h.min(frame.h - r.y) })
    }
}

/// Letterbox / pillarbox detection over a clip's first samples (ALGORITHMS §1.0 step 3).
pub fn detect_active_area(frames: &[&Image]) -> Rect {
    let Some(first) = frames.first() else { return Rect::default() };
    let (w, h) = (first.w, first.h);
    let full = Rect::full(w, h);
    if frames.iter().any(|f| f.w != w || f.h != h) || w < 8 || h < 8 {
        return full;
    }
    let stats = |along_rows: bool| -> Vec<bool> {
        let n = if along_rows { h } else { w };
        let mut means = vec![Vec::with_capacity(frames.len()); n];
        let mut stds = vec![Vec::with_capacity(frames.len()); n];
        for f in frames {
            for i in 0..n {
                let (mut s, mut s2, mut c) = (0f64, 0f64, 0f64);
                if along_rows {
                    for &v in f.row(i) {
                        s += v as f64;
                        s2 += (v as f64) * (v as f64);
                        c += 1.0;
                    }
                } else {
                    for y in 0..h {
                        let v = f.at(i, y) as f64;
                        s += v;
                        s2 += v * v;
                        c += 1.0;
                    }
                }
                let m = s / c;
                means[i].push(m as f32);
                stds[i].push(((s2 / c - m * m).max(0.0)).sqrt() as f32);
            }
        }
        (0..n)
            .map(|i| {
                crate::util::median(&means[i]) < 0.035 && crate::util::median(&stds[i]) < 0.012
            })
            .collect()
    };
    let rows = stats(true);
    let cols = stats(false);
    let span = |bar: &[bool]| -> (usize, usize) {
        let n = bar.len();
        let top = bar.iter().take_while(|b| **b).count();
        if top == n {
            return (0, n);
        }
        let bottom = bar.iter().rev().take_while(|b| **b).count();
        let min = (n as f64 * 0.02).ceil() as usize;
        let t = if top >= min { top } else { 0 };
        let b = if bottom >= min { bottom } else { 0 };
        (t, n - b)
    };
    let (y0, y1) = span(&rows);
    let (x0, x1) = span(&cols);
    if y1 <= y0 + 8 || x1 <= x0 + 8 {
        return full;
    }
    Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

/// Horizon tilt (degrees, CCW positive) from a Hough transform over near-horizontal edges
/// of the level-0 motion image (ALGORITHMS §1.6). NaN when no strong line exists.
pub fn horizon_tilt(img: &Image) -> f32 {
    let (w, h) = (img.w, img.h);
    if w < 16 || h < 16 {
        return f32::NAN;
    }
    const STEPS: usize = 61; // −15°..15° in 0.5° steps
    let angles: Vec<(f32, f32)> = (0..STEPS)
        .map(|i| {
            let a = (-15.0 + 0.5 * i as f32).to_radians();
            (a.sin(), a.cos())
        })
        .collect();
    let diag = ((w * w + h * h) as f32).sqrt();
    let nrho = (2.0 * diag) as usize + 2;
    let mut acc = vec![0u32; STEPS * nrho];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let gx = img.at(x + 1, y) - img.at(x - 1, y);
            let gy = img.at(x, y + 1) - img.at(x, y - 1);
            let m = gx.hypot(gy);
            // Strong edge whose gradient is close to vertical (i.e. a near-horizontal line).
            if m < 0.08 || gx.abs() > gy.abs() * 0.47 {
                continue;
            }
            for (i, (s, c)) in angles.iter().enumerate() {
                let rho = -(x as f32) * s + y as f32 * c + diag;
                acc[i * nrho + rho as usize] += 1;
            }
        }
    }
    let (mut best, mut bi) = (0u32, 0usize);
    for (k, v) in acc.iter().enumerate() {
        if *v > best {
            best = *v;
            bi = k / nrho;
        }
    }
    if (best as f32) < 0.15 * w as f32 {
        return f32::NAN;
    }
    // Line direction (cos θ, sin θ) in y-down coordinates: positive θ descends to the right,
    // which is a clockwise tilt on screen.
    -(-15.0 + 0.5 * bi as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letterbox_detected() {
        let f = Image::from_fn(960, 540, |x, y| {
            if !(70..470).contains(&y) {
                0.0
            } else {
                0.3 + 0.4 * ((x * 7 + y * 3) % 17) as f32 / 17.0
            }
        });
        let r = detect_active_area(&[&f, &f, &f]);
        assert!((r.y as i64 - 70).abs() <= 2 && (r.h as i64 - 400).abs() <= 2, "{r:?}");
        assert_eq!(r.x, 0);
        assert_eq!(r.w, 960);
    }

    #[test]
    fn dark_scene_is_not_letterbox() {
        let f = Image::from_fn(320, 180, |_, _| 0.01);
        assert_eq!(detect_active_area(&[&f]), Rect::full(320, 180));
    }

    #[test]
    fn horizon_tilt_detects_angle() {
        for &deg in &[0.0f32, 3.0, -5.0] {
            let t = deg.to_radians().tan();
            // Visual CCW tilt: the line rises to the right, i.e. y decreases with x.
            let img = Image::from_fn(320, 180, |x, y| {
                let yl = 90.0 - (x as f32 - 160.0) * t;
                if (y as f32) < yl {
                    0.8
                } else {
                    0.2
                }
            });
            let got = horizon_tilt(&img);
            assert!((got - deg).abs() <= 0.6, "{deg}: {got}");
        }
    }
}
