//! Single-frame spatial kernels: noise, focus, exposure, banding, patterns (ALGORITHMS §1.1–1.3, §1.5–1.6).

use crate::image::{pairwise_sum, Image, Rect};
use crate::util::{percentile_sorted, smoothstep, weighted_percentile};

/// Tile grid for an active area (ALGORITHMS §1.0 step 4).
pub fn tile_grid(w: usize, h: usize) -> (usize, usize) {
    let rows = ((8.0 * h as f64 / w.max(1) as f64).round() as i64).clamp(4, 8) as usize;
    (8, rows)
}

/// Results of the spatial pass over the active image.
#[derive(Clone, Debug, Default)]
pub struct Spatial {
    pub noise_sigma: f32,
    pub sharp_full: f32,
    pub sharp_centre: f32,
    pub tenengrad: f32,
    pub detail_raw: f32,
    /// log10(1e-7 + LV'_t) per tile, row-major.
    pub tiles_log: Vec<f32>,
    /// Structure tensor sums Σ Gx², Σ GxGy, Σ Gy².
    pub jxx: f64,
    pub jxy: f64,
    pub jyy: f64,
    pub exposure: Exposure,
}

#[derive(Clone, Debug)]
pub struct Exposure {
    pub mean: f32,
    pub std: f32,
    pub p01: f32,
    pub p05: f32,
    pub p50: f32,
    pub p95: f32,
    pub p99: f32,
    pub clip_black: f32,
    pub clip_white: f32,
    pub near_black: f32,
    pub near_white: f32,
    pub row_band: f32,
    pub hist64: [f32; 64],
}

impl Default for Exposure {
    fn default() -> Self {
        Exposure {
            mean: 0.0,
            std: 0.0,
            p01: 0.0,
            p05: 0.0,
            p50: 0.0,
            p95: 0.0,
            p99: 0.0,
            clip_black: 0.0,
            clip_white: 0.0,
            near_black: 0.0,
            near_white: 0.0,
            row_band: 0.0,
            hist64: [0.0; 64],
        }
    }
}

const MAG_BINS: usize = 4096;
const MAG_MAX: f32 = 6.0; // max Sobel magnitude for [0,1] data is 4·√2 ≈ 5.66

/// Runs every spatial kernel on the active image `a`.
pub fn spatial(a: &Image) -> Spatial {
    let (w, h) = (a.w, a.h);
    let (tc, tr) = tile_grid(w, h);
    let mut out = Spatial { exposure: exposure(a), ..Default::default() };
    if w < 3 || h < 3 {
        out.tiles_log = vec![-7.0; tc * tr];
        out.sharp_full = -7.0;
        out.sharp_centre = -7.0;
        out.tenengrad = -7.0;
        return out;
    }

    // Pass 1: Sobel magnitude (stored), its histogram, Laplacian per tile, Tenengrad per tile,
    // structure tensor, and |L ⊛ N| (stored) for the noise estimate.
    let iw = w - 2;
    let ih = h - 2;
    let mut mag = vec![0f32; iw * ih];
    let mut nconv = vec![0f32; iw * ih];
    let mut mag_hist = vec![0u32; MAG_BINS];
    let ntiles = tc * tr;
    let mut lap_sum = vec![0f64; ntiles];
    let mut lap_sq = vec![0f64; ntiles];
    let mut ten_sum = vec![0f64; ntiles];
    let mut cnt = vec![0u64; ntiles];
    let (mut jxx, mut jxy, mut jyy) = (0f64, 0f64, 0f64);

    for y in 1..h - 1 {
        let r0 = a.row(y - 1);
        let r1 = a.row(y);
        let r2 = a.row(y + 1);
        let ty = (y * tr / h).min(tr - 1);
        let (mut rjxx, mut rjxy, mut rjyy) = (0f64, 0f64, 0f64);
        for x in 1..w - 1 {
            let (a00, a01, a02) = (r0[x - 1], r0[x], r0[x + 1]);
            let (a10, a11, a12) = (r1[x - 1], r1[x], r1[x + 1]);
            let (a20, a21, a22) = (r2[x - 1], r2[x], r2[x + 1]);
            let gx = (a02 + 2.0 * a12 + a22) - (a00 + 2.0 * a10 + a20);
            let gy = (a20 + 2.0 * a21 + a22) - (a00 + 2.0 * a01 + a02);
            let lap = a01 + a10 + a12 + a21 - 4.0 * a11;
            let n = (a00 + a02 + a20 + a22) - 2.0 * (a01 + a10 + a12 + a21) + 4.0 * a11;
            let g2 = gx * gx + gy * gy;
            let m = g2.sqrt();
            let i = (y - 1) * iw + (x - 1);
            mag[i] = m;
            nconv[i] = n.abs();
            mag_hist[((m / MAG_MAX * MAG_BINS as f32) as usize).min(MAG_BINS - 1)] += 1;
            let t = ty * tc + (x * tc / w).min(tc - 1);
            lap_sum[t] += lap as f64;
            lap_sq[t] += (lap as f64) * (lap as f64);
            ten_sum[t] += g2 as f64;
            cnt[t] += 1;
            rjxx += (gx * gx) as f64;
            rjxy += (gx * gy) as f64;
            rjyy += (gy * gy) as f64;
        }
        jxx += rjxx;
        jxy += rjxy;
        jyy += rjyy;
    }
    out.jxx = jxx;
    out.jxy = jxy;
    out.jyy = jyy;

    // Noise: Immerkær over pixels whose Sobel magnitude ≤ P90 (histogram threshold, deterministic).
    let total = (iw * ih) as u64;
    let p90_mag = hist_quantile_upper(&mag_hist, total, 0.90) * MAG_MAX / MAG_BINS as f32;
    let mut nsum = 0f64;
    let mut ncount = 0u64;
    for (m, n) in mag.iter().zip(&nconv) {
        if *m <= p90_mag {
            nsum += *n as f64;
            ncount += 1;
        }
    }
    let sigma = if ncount > 0 {
        ((std::f64::consts::PI / 2.0).sqrt() * nsum / (6.0 * ncount as f64)) as f32
    } else {
        0.0
    };
    out.noise_sigma = sigma;
    let s2 = (sigma as f64) * (sigma as f64);

    // Per-tile Laplacian variance and Tenengrad, noise-corrected.
    let mut lvp = vec![0f32; ntiles];
    let mut tgp = vec![0f32; ntiles];
    for t in 0..ntiles {
        if cnt[t] == 0 {
            continue;
        }
        let n = cnt[t] as f64;
        let mean = lap_sum[t] / n;
        let var = (lap_sq[t] / n - mean * mean).max(0.0);
        lvp[t] = (var - 20.0 * s2).max(0.0) as f32;
        tgp[t] = (ten_sum[t] / n - 24.0 * s2).max(0.0) as f32;
    }
    out.tiles_log = lvp.iter().map(|v| (1e-7 + *v as f64).log10() as f32).collect();

    let mut sorted = lvp.clone();
    sorted.sort_by(f32::total_cmp);
    let p90 = percentile_sorted(&sorted, 0.90);
    out.sharp_full = (1e-7 + p90 as f64).log10() as f32;
    let mut tsorted = tgp.clone();
    tsorted.sort_by(f32::total_cmp);
    out.tenengrad = (1e-7 + percentile_sorted(&tsorted, 0.90) as f64).log10() as f32;

    // Centre-weighted variant: Gaussian weights by normalised distance from centre (corner = 1).
    let mut pairs = Vec::with_capacity(ntiles);
    for ty in 0..tr {
        for tx in 0..tc {
            let cx = (tx as f64 + 0.5) / tc as f64 - 0.5;
            let cy = (ty as f64 + 0.5) / tr as f64 - 0.5;
            let d = (cx * cx + cy * cy).sqrt() / 0.5f64.hypot(0.5);
            let wgt = (-d * d / (2.0 * 0.30 * 0.30)).exp();
            pairs.push((lvp[ty * tc + tx], wgt as f32));
        }
    }
    out.sharp_centre = (1e-7 + weighted_percentile(&mut pairs, 0.90) as f64).log10() as f32;

    // Detail: edge density above τ = max(0.02, 3σ) and tile coverage.
    let tau = (3.0 * sigma).max(0.02);
    let edges = mag.iter().filter(|m| **m > tau).count();
    let edge_density = edges as f32 / mag.len().max(1) as f32;
    let cover_thr = 0.25 * p90;
    let coverage = if p90 > 0.0 {
        lvp.iter().filter(|v| **v >= cover_thr).count() as f32 / ntiles as f32
    } else {
        0.0
    };
    out.detail_raw = 0.5 * smoothstep(0.01, 0.20, edge_density) + 0.5 * coverage;
    out
}

/// Smallest value v (in bins) such that the cumulative count up to and including v ≥ q·total.
fn hist_quantile_upper(hist: &[u32], total: u64, q: f64) -> f32 {
    let target = (q * total as f64).ceil() as u64;
    let mut cum = 0u64;
    for (i, c) in hist.iter().enumerate() {
        cum += *c as u64;
        if cum >= target {
            return (i + 1) as f32;
        }
    }
    hist.len() as f32
}

const HBINS: usize = 1024;

/// Exposure measures over the active image (ALGORITHMS §1.3).
pub fn exposure(a: &Image) -> Exposure {
    let n = a.data.len();
    let mut e = Exposure::default();
    if n == 0 {
        return e;
    }
    let mut hist = vec![0u32; HBINS];
    let (mut cb, mut cw, mut nb, mut nw) = (0usize, 0usize, 0usize, 0usize);
    for &v in &a.data {
        hist[((v * HBINS as f32) as usize).min(HBINS - 1)] += 1;
        if v <= 0.004 {
            cb += 1;
        }
        if v >= 0.996 {
            cw += 1;
        }
        if v <= 0.02 {
            nb += 1;
        }
        if v >= 0.95 {
            nw += 1;
        }
    }
    let mean = pairwise_sum(&a.data) / n as f64;
    let mut sq = 0f64;
    for row in a.data.chunks(4096) {
        let mut s = 0f64;
        for &v in row {
            let d = v as f64 - mean;
            s += d * d;
        }
        sq += s;
    }
    e.mean = mean as f32;
    e.std = (sq / n as f64).sqrt() as f32;
    let pct = |q: f64| hist_percentile(&hist, n as u64, q);
    e.p01 = pct(0.01);
    e.p05 = pct(0.05);
    e.p50 = pct(0.50);
    e.p95 = pct(0.95);
    e.p99 = pct(0.99);
    let nf = n as f32;
    e.clip_black = cb as f32 / nf;
    e.clip_white = cw as f32 / nf;
    e.near_black = nb as f32 / nf;
    e.near_white = nw as f32 / nf;
    for (i, c) in hist.iter().enumerate() {
        e.hist64[i * 64 / HBINS] += *c as f32 / nf;
    }
    e.row_band = row_band(a);
    e
}

/// Percentile from a [0,1] histogram with linear interpolation inside the crossing bin.
fn hist_percentile(hist: &[u32], total: u64, q: f64) -> f32 {
    let target = q * total as f64;
    let mut cum = 0f64;
    let nb = hist.len() as f64;
    for (i, c) in hist.iter().enumerate() {
        let next = cum + *c as f64;
        if next >= target && *c > 0 {
            let frac = ((target - cum) / *c as f64).clamp(0.0, 1.0);
            return ((i as f64 + frac) / nb) as f32;
        }
        cum = next;
    }
    1.0
}

/// Rolling-shutter banding score (ALGORITHMS §1.3.1), via a direct DFT at integer
/// frequencies in cycles per frame height.
pub fn row_band(a: &Image) -> f32 {
    let h = a.h;
    if h < 16 {
        return 0.0;
    }
    let r: Vec<f64> = (0..h).map(|y| pairwise_sum(a.row(y)) / a.w as f64).collect();
    let win = (h / 8).max(1);
    let half = win / 2;
    let mut prefix = vec![0f64; h + 1];
    for y in 0..h {
        prefix[y + 1] = prefix[y] + r[y];
    }
    let e: Vec<f64> = (0..h)
        .map(|y| {
            let lo = y.saturating_sub(half);
            let hi = (y + half + 1).min(h);
            r[y] - (prefix[hi] - prefix[lo]) / (hi - lo) as f64
        })
        .collect();
    let em = e.iter().sum::<f64>() / h as f64;
    let std_e = (e.iter().map(|v| (v - em) * (v - em)).sum::<f64>() / h as f64).sqrt();
    let gate = smoothstep(0.002, 0.01, std_e as f32);
    if gate == 0.0 {
        return 0.0;
    }
    let nyq = h / 2;
    let mut total = 0f64;
    let mut peak = 0f64;
    for f in 1..=nyq {
        let w = 2.0 * std::f64::consts::PI * f as f64 / h as f64;
        let (mut re, mut im) = (0f64, 0f64);
        for (y, v) in e.iter().enumerate() {
            let ang = w * y as f64;
            re += v * ang.cos();
            im -= v * ang.sin();
        }
        let m = (re * re + im * im).sqrt();
        total += m;
        if (2..=40).contains(&f) {
            peak = peak.max(m);
        }
    }
    ((peak / (total + 1e-6)) as f32 * gate).clamp(0.0, 1.0)
}

/// White-balance cast from chroma thumbs and a luma thumb of the same size (ALGORITHMS §1.3.2).
pub fn wb_cast(luma: &Image, cb: &Image, cr: &Image) -> f32 {
    let (mut sb, mut sr, mut n) = (0f64, 0f64, 0u32);
    for i in 0..luma.data.len().min(cb.data.len()).min(cr.data.len()) {
        let l = luma.data[i];
        if (0.2..=0.8).contains(&l) {
            sb += cb.data[i] as f64;
            sr += cr.data[i] as f64;
            n += 1;
        }
    }
    if n == 0 {
        return 0.0;
    }
    let (mb, mr) = (sb / n as f64 - 0.5, sr / n as f64 - 0.5);
    (mb * mb + mr * mr).sqrt() as f32
}

fn mean_std(v: &[f32]) -> (f32, f32) {
    if v.is_empty() {
        return (0.0, 0.0);
    }
    let m = pairwise_sum(v) / v.len() as f64;
    let var = v.iter().map(|x| (*x as f64 - m).powi(2)).sum::<f64>() / v.len() as f64;
    (m as f32, var.sqrt() as f32)
}

/// Solid-colour test (ALGORITHMS §1.6). Returns sub-type: 0 none, 1 black, 2 white, 3 colour.
pub fn solid_colour(luma_std: f32, luma_mean: f32, chroma: Option<(&Image, &Image)>) -> u8 {
    if luma_std >= 0.010 {
        return 0;
    }
    if let Some((cb, cr)) = chroma {
        let (_, sb) = mean_std(&cb.data);
        let (_, sr) = mean_std(&cr.data);
        if sb.max(sr) >= 0.010 {
            return 0;
        }
    }
    if luma_mean < 0.05 {
        1
    } else if luma_mean > 0.95 {
        2
    } else {
        3
    }
}

/// Colour-bars detector on the 64×36 luma thumb and optional chroma thumbs (ALGORITHMS §1.6).
pub fn colour_bars(thumb: &Image, chroma: Option<(&Image, &Image)>) -> bool {
    // SMPTE RP 219 / EBU 75% bar luma (Rec.709): white, yellow, cyan, green, magenta, red, blue.
    const TEMPLATE: [f32; 7] = [0.75, 0.70, 0.59, 0.54, 0.21, 0.16, 0.05];
    let rows = thumb.h * 2 / 3;
    if rows == 0 || thumb.w < 14 {
        return false;
    }
    let mut means = [0f32; 7];
    for (b, mean_out) in means.iter_mut().enumerate() {
        let x0 = b * thumb.w / 7;
        let x1 = ((b + 1) * thumb.w / 7).max(x0 + 1);
        // Ignore one column at each band edge to tolerate resampling blur.
        let (x0, x1) = if x1 - x0 > 3 { (x0 + 1, x1 - 1) } else { (x0, x1) };
        let mut v = Vec::with_capacity((x1 - x0) * rows);
        for y in 0..rows {
            v.extend_from_slice(&thumb.row(y)[x0..x1]);
        }
        let (m, s) = mean_std(&v);
        if s >= 0.03 {
            return false;
        }
        *mean_out = m;
    }
    if pearson(&means, &TEMPLATE) < 0.9 {
        return false;
    }
    match chroma {
        Some((cb, cr)) => {
            // Bars 2..7 are saturated: mean chroma deviation from neutral must be large.
            let crow = cb.h * 2 / 3;
            let mut dev = 0f32;
            for b in 1..7 {
                let x0 = b * cb.w / 7;
                let x1 = ((b + 1) * cb.w / 7).max(x0 + 1);
                let mut s = 0f32;
                let mut n = 0f32;
                for y in 0..crow.max(1) {
                    for x in x0..x1 {
                        let i = y * cb.w + x;
                        s += (cb.data[i] - 0.5).hypot(cr.data[i] - 0.5);
                        n += 1.0;
                    }
                }
                dev += s / n.max(1.0);
            }
            dev / 6.0 > 0.05
        }
        None => true,
    }
}

fn pearson(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len() as f32;
    let ma = a.iter().sum::<f32>() / n;
    let mb = b.iter().sum::<f32>() / n;
    let (mut num, mut da, mut db) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        num += (x - ma) * (y - mb);
        da += (x - ma) * (x - ma);
        db += (y - mb) * (y - mb);
    }
    if da <= 1e-12 || db <= 1e-12 {
        return 0.0;
    }
    num / (da.sqrt() * db.sqrt())
}

/// Mean absolute difference between two equally sized thumbnails.
pub fn mad(a: &Image, b: &Image) -> f32 {
    if a.data.len() != b.data.len() || a.data.is_empty() {
        return 1.0;
    }
    let mut s = 0f64;
    for (x, y) in a.data.iter().zip(&b.data) {
        s += (x - y).abs() as f64;
    }
    (s / a.data.len() as f64) as f32
}

/// χ² histogram distance ½ Σ (h−g)²/(h+g).
pub fn chi2(h: &[f32; 64], g: &[f32; 64]) -> f32 {
    let mut s = 0f32;
    for i in 0..64 {
        let d = h[i] - g[i];
        s += d * d / (h[i] + g[i] + 1e-9);
    }
    0.5 * s
}

/// 64-bit DCT perceptual hash of a thumbnail.
pub fn phash(thumb: &Image) -> u64 {
    let img = thumb.resize_area(Rect::full(thumb.w, thumb.h), 32, 32);
    let n = 32usize;
    let mut cos = vec![0f64; 8 * n];
    for u in 0..8 {
        for x in 0..n {
            cos[u * n + x] =
                ((2 * x + 1) as f64 * u as f64 * std::f64::consts::PI / (2.0 * n as f64)).cos();
        }
    }
    let mut coeffs = [0f64; 64];
    for v in 0..8 {
        for u in 0..8 {
            let mut s = 0f64;
            for y in 0..n {
                let cy = cos[v * n + y];
                let row = img.row(y);
                let mut rs = 0f64;
                for x in 0..n {
                    rs += row[x] as f64 * cos[u * n + x];
                }
                s += rs * cy;
            }
            coeffs[v * 8 + u] = s;
        }
    }
    let mut ac: Vec<f64> = coeffs[1..].to_vec();
    ac.sort_by(f64::total_cmp);
    let median = 0.5 * (ac[30] + ac[31]);
    let mut bits = 0u64;
    for (i, c) in coeffs.iter().enumerate() {
        if *c > median {
            bits |= 1 << i;
        }
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::SplitMix;

    fn texture(w: usize, h: usize, seed: u64) -> Image {
        // Band-limited procedural texture: sum of a few sinusoids plus fine noise.
        let mut rng = SplitMix(seed);
        let comps: Vec<(f32, f32, f32, f32)> = (0..24)
            .map(|_| {
                (rng.f32() * 0.6 + 0.02, rng.f32() * 0.6 + 0.02, rng.f32() * 6.28, rng.f32() * 0.05)
            })
            .collect();
        Image::from_fn(w, h, |x, y| {
            let mut v = 0.5;
            for (fx, fy, ph, a) in &comps {
                v += a * (fx * x as f32 + fy * y as f32 + ph).sin();
            }
            v.clamp(0.0, 1.0)
        })
    }

    fn blur(img: &Image, sigma: f32) -> Image {
        if sigma <= 0.0 {
            return img.clone();
        }
        let r = (3.0 * sigma).ceil() as i64;
        let k: Vec<f32> = (-r..=r).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
        let ks: f32 = k.iter().sum();
        let pass = |src: &Image, horiz: bool| {
            Image::from_fn(src.w, src.h, |x, y| {
                let mut s = 0.0;
                for (j, kv) in k.iter().enumerate() {
                    let o = j as i64 - r;
                    let (sx, sy) = if horiz {
                        ((x as i64 + o).clamp(0, src.w as i64 - 1) as usize, y)
                    } else {
                        (x, (y as i64 + o).clamp(0, src.h as i64 - 1) as usize)
                    };
                    s += src.at(sx, sy) * kv;
                }
                s / ks
            })
        };
        pass(&pass(img, true), false)
    }

    fn add_noise(img: &Image, sigma: f32, seed: u64) -> Image {
        let mut rng = SplitMix(seed);
        Image { data: img.data.iter().map(|v| v + sigma * rng.gauss()).collect(), ..img.clone() }
    }

    #[test]
    fn noise_estimate_tracks_injected_noise() {
        let base = blur(&texture(480, 270, 3), 1.0);
        for &s in &[0.005f32, 0.01, 0.02, 0.05] {
            let img = add_noise(&base, s, 11);
            let est = spatial(&img).noise_sigma;
            assert!((est - s).abs() / s < 0.15, "σ {s}: estimated {est}");
        }
    }

    #[test]
    fn sharpness_decreases_with_blur() {
        let base = texture(480, 270, 5);
        let mut last = f32::INFINITY;
        for &s in &[0.0f32, 0.7, 1.2, 1.8, 2.6, 3.6, 5.0] {
            let v = spatial(&blur(&base, s)).sharp_full;
            assert!(v < last, "blur {s}: {v} !< {last}");
            last = v;
        }
    }

    #[test]
    fn noisy_flat_field_has_no_laplacian_energy() {
        let flat = Image::from_fn(320, 180, |_, _| 0.5);
        let img = add_noise(&flat, 0.02, 9);
        let s = spatial(&img);
        // Noise-corrected Laplacian energy is ≈ 0 (≤ a few percent of the raw noise energy 20σ²).
        assert!(10f32.powf(s.sharp_full) < 20.0 * 0.02 * 0.02 * 0.1, "{}", s.sharp_full);
    }

    #[test]
    fn centre_weighting_rewards_sharp_centre() {
        let sharp = texture(480, 270, 7);
        let soft = blur(&sharp, 6.0);
        let img = Image::from_fn(480, 270, |x, y| {
            let (dx, dy) = (x as f32 / 480.0 - 0.5, y as f32 / 270.0 - 0.5);
            if dx.abs() < 0.15 && dy.abs() < 0.15 {
                sharp.at(x, y)
            } else {
                soft.at(x, y)
            }
        });
        let s = spatial(&img);
        assert!(s.sharp_centre >= s.sharp_full, "{} < {}", s.sharp_centre, s.sharp_full);
    }

    #[test]
    fn exposure_percentiles_on_ramp() {
        let img = Image::from_fn(1000, 10, |x, _| x as f32 / 999.0);
        let e = exposure(&img);
        assert!((e.p50 - 0.5).abs() < 0.002, "{}", e.p50);
        assert!((e.p05 - 0.05).abs() < 0.002);
        assert!((e.p95 - 0.95).abs() < 0.002);
        assert!((e.mean - 0.5).abs() < 1e-4);
        assert!((e.clip_black - 0.005).abs() < 0.0015);
        let s: f32 = e.hist64.iter().sum();
        assert!((s - 1.0).abs() < 1e-4);
    }

    #[test]
    fn clip_fractions_exact() {
        let img = Image::from_fn(100, 100, |x, y| if y * 100 + x < 300 { 1.0 } else { 0.5 });
        let e = exposure(&img);
        assert!((e.clip_white - 0.03).abs() < 1e-6);
    }

    #[test]
    fn banding_detected_only_for_bands() {
        let band = Image::from_fn(320, 180, |_, y| 0.5 + 0.05 * (y as f32 * 6.0 * 6.283 / 180.0).sin());
        assert!(row_band(&band) > 0.3, "{}", row_band(&band));
        let grad = Image::from_fn(320, 180, |_, y| y as f32 / 180.0);
        assert!(row_band(&grad) < 0.05, "{}", row_band(&grad));
    }

    #[test]
    fn phash_stability() {
        let a = texture(64, 36, 1);
        let b = add_noise(&a, 0.01, 2);
        let c = texture(64, 36, 99);
        assert!((phash(&a) ^ phash(&b)).count_ones() < 6);
        assert!((phash(&a) ^ phash(&c)).count_ones() > 20);
    }

    #[test]
    fn bars_detector() {
        const L: [f32; 7] = [0.75, 0.70, 0.59, 0.54, 0.21, 0.16, 0.05];
        let bars = Image::from_fn(64, 36, |x, _| L[(x * 7 / 64).min(6)]);
        assert!(colour_bars(&bars, None));
        let tex = texture(64, 36, 4);
        assert!(!colour_bars(&tex, None));
    }
}
