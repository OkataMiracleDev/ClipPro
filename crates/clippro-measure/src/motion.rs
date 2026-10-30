//! Motion estimation: pyramids, block matching and a robust similarity model (ALGORITHMS §1.4).

use crate::image::{Image, Rect};
use crate::util::{median, Xoshiro256};

pub const MOTION_LONG_EDGE: usize = 320;
const BLOCK: usize = 16;

/// Motion pyramid of the active area: level 0 is ≤ 320 px on the long edge.
#[derive(Clone, Debug, Default)]
pub struct Pyramid {
    pub levels: Vec<Image>,
    /// Integral images (sum, sum of squares) per level, for O(1) patch statistics.
    pub ii: Vec<Integral>,
}

/// Summed-area tables of an image and its square, (w+1)×(h+1), in f64.
#[derive(Clone, Debug, Default)]
pub struct Integral {
    w: usize,
    s: Vec<f64>,
    s2: Vec<f64>,
}

impl Integral {
    pub fn new(img: &Image) -> Integral {
        let (w, h) = (img.w, img.h);
        let stride = w + 1;
        let mut s = vec![0f64; stride * (h + 1)];
        let mut s2 = vec![0f64; stride * (h + 1)];
        for y in 0..h {
            let (mut rs, mut rs2) = (0f64, 0f64);
            let row = img.row(y);
            for x in 0..w {
                let v = row[x] as f64;
                rs += v;
                rs2 += v * v;
                s[(y + 1) * stride + x + 1] = s[y * stride + x + 1] + rs;
                s2[(y + 1) * stride + x + 1] = s2[y * stride + x + 1] + rs2;
            }
        }
        Integral { w, s, s2 }
    }

    /// (sum, sum of squares) over the n×n patch at (x0, y0).
    #[inline]
    fn rect(&self, x0: usize, y0: usize, n: usize) -> (f64, f64) {
        let st = self.w + 1;
        let (a, b, c, d) = (y0 * st + x0, y0 * st + x0 + n, (y0 + n) * st + x0, (y0 + n) * st + x0 + n);
        (self.s[d] - self.s[b] - self.s[c] + self.s[a], self.s2[d] - self.s2[b] - self.s2[c] + self.s2[a])
    }
}

impl Pyramid {
    pub fn build(active: &Image) -> Pyramid {
        let long = active.w.max(active.h).max(1);
        let s = (MOTION_LONG_EDGE as f64 / long as f64).min(1.0);
        let w0 = ((active.w as f64 * s).round() as usize).max(8);
        let h0 = ((active.h as f64 * s).round() as usize).max(8);
        let l0 = active.resize_area(Rect::full(active.w, active.h), w0, h0);
        let l1 = l0.half();
        let l2 = l1.half();
        let ii = vec![Integral::new(&l0), Integral::new(&l1), Integral::new(&l2)];
        Pyramid { levels: vec![l0, l1, l2], ii }
    }

    pub fn l0(&self) -> &Image {
        &self.levels[0]
    }
}

/// Output of one frame-pair motion estimate.
#[derive(Clone, Debug, Default)]
pub struct MotionEstimate {
    /// Translation at the image centre, level-0 px.
    pub tx: f32,
    pub ty: f32,
    pub scale: f32,
    /// Rotation in radians (image coordinates, y down).
    pub rot: f32,
    pub inlier_ratio: f32,
    pub residual_rms: f32,
    pub textured_frac: f32,
    pub conf: f32,
    /// Fraction of textured blocks deviating > 1.5 px from the model.
    pub moving_frac: f32,
    /// Median residual (level-0 px) over moving blocks; 0 if none.
    pub moving_median_px: f32,
    /// Per-block displacement (level-0 px); NaN for untextured blocks.
    pub vectors: Vec<[f32; 2]>,
    pub grid: (usize, usize),
    /// Level-0 width used for normalisation.
    pub w0: usize,
}

impl MotionEstimate {
    pub fn identity(w0: usize, grid: (usize, usize)) -> Self {
        MotionEstimate {
            scale: 1.0,
            vectors: vec![[f32::NAN; 2]; grid.0 * grid.1],
            grid,
            w0,
            ..Default::default()
        }
    }
}

fn grid_for(w: usize, h: usize) -> (usize, usize) {
    if w >= h {
        (16, 9)
    } else {
        (9, 16)
    }
}

/// Zero-mean, contrast-normalised patch at (x0, y0) of size n×n (caller guarantees bounds).
fn patch(img: &Image, x0: usize, y0: usize, n: usize, out: &mut [f32]) -> (f32, f32) {
    let mut s = 0f32;
    for y in 0..n {
        let row = &img.row(y0 + y)[x0..x0 + n];
        out[y * n..(y + 1) * n].copy_from_slice(row);
        s += row.iter().sum::<f32>();
    }
    let m = s / (n * n) as f32;
    let mut v = 0f32;
    for p in out.iter() {
        v += (p - m) * (p - m);
    }
    let sd = (v / (n * n) as f32).sqrt();
    let inv = 1.0 / (sd + 0.01);
    for p in out.iter_mut() {
        *p = (*p - m) * inv;
    }
    (m, sd)
}

/// SAD between a normalised reference patch and the candidate patch at (x0, y0), normalised
/// on the fly (mean and std from the integral images, so no copy per candidate).
fn sad_at(img: &Image, ii: &Integral, x0: i64, y0: i64, n: usize, reference: &[f32]) -> f32 {
    if x0 < 0 || y0 < 0 || x0 as usize + n > img.w || y0 as usize + n > img.h {
        return f32::INFINITY;
    }
    let (x0, y0) = (x0 as usize, y0 as usize);
    let nn = (n * n) as f64;
    let (sum, sum2) = ii.rect(x0, y0, n);
    let m = sum / nn;
    let sd = ((sum2 / nn - m * m).max(0.0)).sqrt();
    let (m, inv) = (m as f32, (1.0 / (sd + 0.01)) as f32);
    let mut s = 0f32;
    for y in 0..n {
        let row = &img.row(y0 + y)[x0..x0 + n];
        let r = &reference[y * n..(y + 1) * n];
        for (a, b) in r.iter().zip(row) {
            s += (a - (b - m) * inv).abs();
        }
    }
    s
}

/// Exhaustive global translation search, ±`r` px around `(cx, cy)`. Returns the `k` best
/// (cost, dx, dy) candidates, cheapest first.
fn global_shift(a: &Image, b: &Image, r: i64, cx: i64, cy: i64, k: usize) -> Vec<(f32, i64, i64)> {
    let ma = a.mean() as f32;
    let mb = b.mean() as f32;
    let (w, h) = (a.w as i64, a.h as i64);
    let mut all: Vec<(f32, i64, i64)> = Vec::new();
    for dy in cy - r..=cy + r {
        for dx in cx - r..=cx + r {
            if dx.abs() >= w / 2 || dy.abs() >= h / 2 {
                continue;
            }
            let x0 = 0.max(-dx);
            let x1 = w.min(w - dx);
            let y0 = 0.max(-dy);
            let y1 = h.min(h - dy);
            if (x1 - x0) * (y1 - y0) < w * h / 2 {
                continue;
            }
            let mut s = 0f32;
            for y in y0..y1 {
                let ra = a.row(y as usize);
                let rb = b.row((y + dy) as usize);
                for x in x0..x1 {
                    s += ((ra[x as usize] - ma) - (rb[(x + dx) as usize] - mb)).abs();
                }
            }
            let cost = s / ((x1 - x0) * (y1 - y0)) as f32 + 1e-6 * ((dx - cx).pow(2) + (dy - cy).pow(2)) as f32;
            all.push((cost, dx, dy));
        }
    }
    all.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.cmp(&q.1)).then(p.2.cmp(&q.2)));
    all.truncate(k.max(1));
    if all.is_empty() {
        all.push((f32::INFINITY, cx, cy));
    }
    all
}

fn parabola(sm: f32, s0: f32, sp: f32) -> f32 {
    let d = sm - 2.0 * s0 + sp;
    if !d.is_finite() || d <= 1e-9 {
        return 0.0;
    }
    (0.5 * (sm - sp) / d).clamp(-0.5, 0.5)
}

/// Similarity model q = R p + t with R = [[a, −b], [b, a]].
#[derive(Clone, Copy, Debug)]
struct Model {
    a: f64,
    b: f64,
    tx: f64,
    ty: f64,
}

impl Model {
    fn apply(&self, p: [f64; 2]) -> [f64; 2] {
        [self.a * p[0] - self.b * p[1] + self.tx, self.b * p[0] + self.a * p[1] + self.ty]
    }
    fn from_pair(p1: [f64; 2], q1: [f64; 2], p2: [f64; 2], q2: [f64; 2]) -> Option<Model> {
        let dp = [p2[0] - p1[0], p2[1] - p1[1]];
        let dq = [q2[0] - q1[0], q2[1] - q1[1]];
        let den = dp[0] * dp[0] + dp[1] * dp[1];
        if den < 64.0 {
            return None;
        }
        // (a + ib) = dq / dp
        let a = (dq[0] * dp[0] + dq[1] * dp[1]) / den;
        let b = (dq[1] * dp[0] - dq[0] * dp[1]) / den;
        let tx = q1[0] - (a * p1[0] - b * p1[1]);
        let ty = q1[1] - (b * p1[0] + a * p1[1]);
        Some(Model { a, b, tx, ty })
    }
}

fn solve4(m: &mut [[f64; 5]; 4]) -> Option<[f64; 4]> {
    for c in 0..4 {
        let mut piv = c;
        for r in c + 1..4 {
            if m[r][c].abs() > m[piv][c].abs() {
                piv = r;
            }
        }
        if m[piv][c].abs() < 1e-12 {
            return None;
        }
        m.swap(c, piv);
        for r in 0..4 {
            if r != c {
                let f = m[r][c] / m[c][c];
                for k in c..5 {
                    m[r][k] -= f * m[c][k];
                }
            }
        }
    }
    Some([m[0][4] / m[0][0], m[1][4] / m[1][1], m[2][4] / m[2][2], m[3][4] / m[3][3]])
}

fn irls(ps: &[[f64; 2]], qs: &[[f64; 2]], init: Model, delta: f64) -> Model {
    let mut model = init;
    for _ in 0..5 {
        let mut m = [[0f64; 5]; 4];
        for (p, q) in ps.iter().zip(qs) {
            let r = model.apply(*p);
            let e = ((r[0] - q[0]).powi(2) + (r[1] - q[1]).powi(2)).sqrt();
            let w = if e <= delta { 1.0 } else { delta / e };
            // Rows: [px, -py, 1, 0] -> qx ; [py, px, 0, 1] -> qy
            let rows = [([p[0], -p[1], 1.0, 0.0], q[0]), ([p[1], p[0], 0.0, 1.0], q[1])];
            for (row, rhs) in rows {
                for i in 0..4 {
                    for j in 0..4 {
                        m[i][j] += w * row[i] * row[j];
                    }
                    m[i][4] += w * row[i] * rhs;
                }
            }
        }
        match solve4(&mut m) {
            Some(s) => model = Model { a: s[0], b: s[1], tx: s[2], ty: s[3] },
            None => break,
        }
    }
    model
}

/// Estimates motion from `prev` to `cur` (content at p in prev appears at p + v in cur).
pub fn estimate(prev: &Pyramid, cur: &Pyramid, noise_sigma: f32, seed: u64) -> MotionEstimate {
    estimate_with_prior(prev, cur, noise_sigma, seed, None)
}

/// As `estimate`, with an optional translation prior (level-0 px) that centres the coarse
/// search, e.g. the pair velocity scaled to the sample interval.
pub fn estimate_with_prior(
    prev: &Pyramid,
    cur: &Pyramid,
    noise_sigma: f32,
    seed: u64,
    prior: Option<[f32; 2]>,
) -> MotionEstimate {
    let a0 = prev.l0();
    let b0 = cur.l0();
    let grid = grid_for(a0.w, a0.h);
    let mut est = MotionEstimate::identity(a0.w, grid);
    if a0.w != b0.w || a0.h != b0.h || a0.w < 2 * BLOCK || a0.h < 2 * BLOCK {
        return est;
    }
    // Coarse global prior (covers ±16 px at level 2 = ±64 px at level 0, around the prior).
    let (pcx, pcy) = prior.map(|p| ((p[0] / 4.0).round() as i64, (p[1] / 4.0).round() as i64)).unwrap_or((0, 0));
    // Coarse-to-fine: ±8 px on a half-size copy of level 2 (= ±16 at level 2), then ±2 px at
    // level 2. Same range as an exhaustive ±16 search at a fraction of the cost.
    // The coarse level can alias on fine texture, so the 4 best coarse candidates and the prior
    // itself are each refined, and the cheapest refined result wins.
    let (a3, b3) = (prev.levels[2].half(), cur.levels[2].half());
    let mut seeds: Vec<(i64, i64)> = vec![(pcx, pcy)];
    if a3.w >= 16 && a3.h >= 8 {
        let c3 = global_shift(&a3, &b3, 8, (pcx as f64 / 2.0).round() as i64, (pcy as f64 / 2.0).round() as i64, 4);
        seeds.extend(c3.iter().map(|c| (2 * c.1, 2 * c.2)));
    }
    let (mut gx, mut gy, mut gbest) = (pcx, pcy, f32::INFINITY);
    for (sx, sy) in seeds {
        let c = global_shift(&prev.levels[2], &cur.levels[2], 2, sx, sy, 1)[0];
        if c.0 < gbest {
            gbest = c.0;
            gx = c.1;
            gy = c.2;
        }
    }

    let (gw, gh) = grid;
    let tex_thr = (2.0 * noise_sigma).max(0.015);
    let cx = a0.w as f64 / 2.0;
    let cy = a0.h as f64 / 2.0;
    let mut ps = Vec::new();
    let mut qs = Vec::new();
    let mut idx = Vec::new();
    let mut textured = 0usize;
    let mut refp = vec![0f32; BLOCK * BLOCK];
    let mut ref1 = vec![0f32; 64];
    let (a1, b1) = (&prev.levels[1], &cur.levels[1]);
    let (a2, b2) = (&prev.levels[2], &cur.levels[2]);
    for j in 0..gh {
        for i in 0..gw {
            let bx = if gw > 1 { i * (a0.w - BLOCK) / (gw - 1) } else { (a0.w - BLOCK) / 2 };
            let by = if gh > 1 { j * (a0.h - BLOCK) / (gh - 1) } else { (a0.h - BLOCK) / 2 };
            let (_, sd) = patch(a0, bx, by, BLOCK, &mut refp);
            if sd < tex_thr {
                continue;
            }
            textured += 1;
            // Level 2 per-block search around the global prior (8×8 block, ±6 px = ±24 px at
            // level 0), so subjects moving against the camera are still matched.
            let (bx2, by2) = ((bx / 4).saturating_sub(2).min(a2.w.saturating_sub(8)), (by / 4).saturating_sub(2).min(a2.h.saturating_sub(8)));
            patch(a2, bx2, by2, 8, &mut ref1);
            let mut best2 = (f32::INFINITY, gx, gy);
            for dy in -6..=6i64 {
                for dx in -6..=6i64 {
                    let (vx, vy) = (gx + dx, gy + dy);
                    let s = sad_at(b2, &cur.ii[2], bx2 as i64 + vx, by2 as i64 + vy, 8, &ref1)
                        + 1e-4 * (dx * dx + dy * dy) as f32;
                    if s < best2.0 {
                        best2 = (s, vx, vy);
                    }
                }
            }
            // Level 1 refinement (8×8 block, ±3 px).
            let (bx1, by1) = ((bx / 2).min(a1.w.saturating_sub(8)), (by / 2).min(a1.h.saturating_sub(8)));
            patch(a1, bx1, by1, 8, &mut ref1);
            let mut best1 = (f32::INFINITY, 2 * best2.1, 2 * best2.2);
            for dy in -3..=3i64 {
                for dx in -3..=3i64 {
                    let (vx, vy) = (2 * best2.1 + dx, 2 * best2.2 + dy);
                    let s = sad_at(b1, &cur.ii[1], bx1 as i64 + vx, by1 as i64 + vy, 8, &ref1)
                        + 1e-4 * (dx * dx + dy * dy) as f32;
                    if s < best1.0 {
                        best1 = (s, vx, vy);
                    }
                }
            }
            // Level 0 refinement (16×16 block, ±2 px) with sub-pixel parabola fit.
            let (px, py) = (2 * best1.1, 2 * best1.2);
            let mut costs = [[f32::INFINITY; 7]; 7];
            let mut best0 = (f32::INFINITY, px, py);
            for dy in -3..=3i64 {
                for dx in -3..=3i64 {
                    let (vx, vy) = (px + dx, py + dy);
                    let s = sad_at(b0, &cur.ii[0], bx as i64 + vx, by as i64 + vy, BLOCK, &refp);
                    costs[(dy + 3) as usize][(dx + 3) as usize] = s;
                    if dx.abs() <= 2 && dy.abs() <= 2 && s < best0.0 {
                        best0 = (s, vx, vy);
                    }
                }
            }
            if !best0.0.is_finite() {
                continue;
            }
            let (ox, oy) = ((best0.1 - px + 3) as usize, (best0.2 - py + 3) as usize);
            let sx = parabola(costs[oy][ox - 1], costs[oy][ox], costs[oy][ox + 1]);
            let sy = parabola(costs[oy - 1][ox], costs[oy][ox], costs[oy + 1][ox]);
            let v = [best0.1 as f32 + sx, best0.2 as f32 + sy];
            let k = j * gw + i;
            est.vectors[k] = v;
            let p = [bx as f64 + BLOCK as f64 / 2.0 - cx, by as f64 + BLOCK as f64 / 2.0 - cy];
            ps.push(p);
            qs.push([p[0] + v[0] as f64, p[1] + v[1] as f64]);
            idx.push(k);
        }
    }
    let total = gw * gh;
    est.textured_frac = textured as f32 / total as f32;
    if ps.len() < 6 {
        return est;
    }

    // RANSAC (200 iterations, inlier threshold 1 px) then IRLS-Huber refinement on inliers.
    let mut rng = Xoshiro256::seed(seed);
    let mut best_inliers: Vec<usize> = Vec::new();
    let n = ps.len();
    for _ in 0..200 {
        let i1 = rng.below(n);
        let mut i2 = rng.below(n);
        if i2 == i1 {
            i2 = (i1 + 1) % n;
        }
        let Some(m) = Model::from_pair(ps[i1], qs[i1], ps[i2], qs[i2]) else { continue };
        let inl: Vec<usize> = (0..n)
            .filter(|&k| {
                let r = m.apply(ps[k]);
                (r[0] - qs[k][0]).hypot(r[1] - qs[k][1]) < 1.0
            })
            .collect();
        if inl.len() > best_inliers.len() {
            best_inliers = inl;
        }
    }
    if best_inliers.len() < 3 {
        // No consensus: fall back to the median translation with low confidence.
        let vx: Vec<f32> = qs.iter().zip(&ps).map(|(q, p)| (q[0] - p[0]) as f32).collect();
        let vy: Vec<f32> = qs.iter().zip(&ps).map(|(q, p)| (q[1] - p[1]) as f32).collect();
        est.tx = median(&vx);
        est.ty = median(&vy);
        est.inlier_ratio = best_inliers.len() as f32 / textured.max(1) as f32;
        est.conf = 0.0;
        return est;
    }
    let ip: Vec<[f64; 2]> = best_inliers.iter().map(|&k| ps[k]).collect();
    let iq: Vec<[f64; 2]> = best_inliers.iter().map(|&k| qs[k]).collect();
    let init = Model { a: 1.0, b: 0.0, tx: 0.0, ty: 0.0 };
    let init = irls(&ip, &iq, init, 1.0e6); // least squares start
    let model = irls(&ip, &iq, init, 1.0);

    let mut sq = 0f64;
    let mut inl = 0usize;
    let mut moving = Vec::new();
    for k in 0..n {
        let r = model.apply(ps[k]);
        let e = (r[0] - qs[k][0]).hypot(r[1] - qs[k][1]);
        if e < 1.0 {
            sq += e * e;
            inl += 1;
        }
        if e > 1.5 {
            moving.push(e as f32);
        }
    }
    let _ = idx;
    est.tx = model.tx as f32;
    est.ty = model.ty as f32;
    est.scale = (model.a.hypot(model.b)) as f32;
    est.rot = model.b.atan2(model.a) as f32;
    est.inlier_ratio = inl as f32 / textured.max(1) as f32;
    est.residual_rms = if inl > 0 { (sq / inl as f64).sqrt() as f32 } else { 0.0 };
    est.moving_frac = moving.len() as f32 / textured.max(1) as f32;
    est.moving_median_px = if moving.is_empty() { 0.0 } else { median(&moving) };
    est.conf = est.inlier_ratio * crate::util::smoothstep(0.10, 0.40, est.textured_frac);
    est
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::SplitMix;

    fn texture(w: usize, h: usize, seed: u64) -> Image {
        let mut rng = SplitMix(seed);
        let comps: Vec<(f32, f32, f32, f32)> = (0..30)
            .map(|_| (rng.f32() * 0.5 + 0.01, rng.f32() * 0.5 + 0.01, rng.f32() * std::f32::consts::TAU, rng.f32() * 0.04))
            .collect();
        Image::from_fn(w, h, |x, y| {
            let mut v = 0.5;
            for (fx, fy, ph, a) in &comps {
                v += a * (fx * x as f32 + fy * y as f32 + ph).sin();
            }
            v
        })
    }

    /// Bilinear sample of a large texture with a similarity transform about the frame centre.
    fn view(src: &Image, w: usize, h: usize, ox: f32, oy: f32, scale: f32, rot: f32) -> Image {
        let (c, s) = (rot.cos(), rot.sin());
        Image::from_fn(w, h, |x, y| {
            let (px, py) = (x as f32 - w as f32 / 2.0, y as f32 - h as f32 / 2.0);
            // inverse map: content moves by +transform, so sample at R^-1 (p) / scale
            let (ix, iy) = ((c * px + s * py) / scale, (-s * px + c * py) / scale);
            let sx = (ix + src.w as f32 / 2.0 - ox).clamp(0.0, src.w as f32 - 2.0);
            let sy = (iy + src.h as f32 / 2.0 - oy).clamp(0.0, src.h as f32 - 2.0);
            let (x0, y0) = (sx.floor() as usize, sy.floor() as usize);
            let (fx, fy) = (sx - x0 as f32, sy - y0 as f32);
            let a = src.at(x0, y0) * (1.0 - fx) + src.at(x0 + 1, y0) * fx;
            let b = src.at(x0, y0 + 1) * (1.0 - fx) + src.at(x0 + 1, y0 + 1) * fx;
            a * (1.0 - fy) + b * fy
        })
    }

    #[test]
    fn recovers_translation() {
        let big = texture(800, 600, 1);
        let a = Pyramid::build(&view(&big, 320, 180, 0.0, 0.0, 1.0, 0.0));
        for &(dx, dy) in &[(3.0f32, 0.0f32), (-7.5, 2.0), (12.0, -9.0), (40.0, 5.0)] {
            let b = Pyramid::build(&view(&big, 320, 180, dx, dy, 1.0, 0.0));
            let e = estimate(&a, &b, 0.0, 7);
            assert!((e.tx - dx).abs() < 0.35 && (e.ty - dy).abs() < 0.35, "{dx},{dy}: {} {}", e.tx, e.ty);
            assert!(e.conf > 0.5, "conf {}", e.conf);
        }
    }

    #[test]
    fn recovers_zoom_and_rotation() {
        let big = texture(800, 600, 2);
        let a = Pyramid::build(&view(&big, 320, 180, 0.0, 0.0, 1.0, 0.0));
        let b = Pyramid::build(&view(&big, 320, 180, 0.0, 0.0, 1.03, 0.0));
        let e = estimate(&a, &b, 0.0, 1);
        assert!((e.scale - 1.03).abs() < 0.006, "scale {}", e.scale);
        let c = Pyramid::build(&view(&big, 320, 180, 0.0, 0.0, 1.0, 0.02));
        let e = estimate(&a, &c, 0.0, 1);
        assert!((e.rot - 0.02).abs() < 0.005, "rot {}", e.rot);
    }

    #[test]
    fn flat_field_has_zero_confidence() {
        let flat = Image::from_fn(320, 180, |_, _| 0.4);
        let p = Pyramid::build(&flat);
        let e = estimate(&p, &p, 0.0, 3);
        assert_eq!(e.conf, 0.0);
    }

    #[test]
    fn deterministic() {
        let big = texture(800, 600, 4);
        let a = Pyramid::build(&view(&big, 320, 180, 0.0, 0.0, 1.0, 0.0));
        let b = Pyramid::build(&view(&big, 320, 180, 5.3, -2.1, 1.01, 0.004));
        let e1 = estimate(&a, &b, 0.0, 99);
        for _ in 0..5 {
            let e2 = estimate(&a, &b, 0.0, 99);
            assert_eq!(e1.tx.to_bits(), e2.tx.to_bits());
            assert_eq!(e1.rot.to_bits(), e2.rot.to_bits());
        }
    }
}
