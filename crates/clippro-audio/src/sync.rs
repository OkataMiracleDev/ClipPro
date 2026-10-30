//! Audio sync for multicam groups (ALGORITHMS §8.3): onset envelopes, FFT cross-correlation
//! with a peak-to-sidelobe confidence, a GCC-PHAT refinement, and group chaining.

use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;

/// Envelope frame rate (Hz): hop 480 at 48 kHz.
pub const ENV_HZ: f64 = 100.0;
const NFFT: usize = 1024;
const HOP: usize = 480;
const RATE: f64 = 48_000.0;
const FINE_RATE: f64 = 8_000.0;

/// Half-wave rectified spectral flux of a mono 48 kHz signal at 100 Hz, z-normalised.
pub fn onset_envelope(x: &[f32]) -> Vec<f32> {
    if x.len() < NFFT {
        return Vec::new();
    }
    let fft = FftPlanner::new().plan_fft_forward(NFFT);
    let win: Vec<f32> = (0..NFFT).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / NFFT as f32).cos()).collect();
    let frames = (x.len() - NFFT) / HOP + 1;
    let mut prev = vec![0f32; NFFT / 2];
    let mut env = Vec::with_capacity(frames);
    let mut buf = vec![Complex32::new(0.0, 0.0); NFFT];
    for f in 0..frames {
        let s = &x[f * HOP..f * HOP + NFFT];
        for i in 0..NFFT {
            buf[i] = Complex32::new(s[i] * win[i], 0.0);
        }
        fft.process(&mut buf);
        let mut flux = 0f32;
        for (k, p) in prev.iter_mut().enumerate() {
            // Log-compressed magnitude keeps quiet onsets from vanishing under loud ones.
            let m = (1.0 + 100.0 * buf[k].norm()).ln();
            flux += (m - *p).max(0.0);
            *p = m;
        }
        env.push(if f == 0 { 0.0 } else { flux });
    }
    zscore(&mut env);
    env
}

fn zscore(v: &mut [f32]) {
    if v.is_empty() {
        return;
    }
    let n = v.len() as f64;
    let mean = v.iter().map(|x| *x as f64).sum::<f64>() / n;
    let var = v.iter().map(|x| (*x as f64 - mean).powi(2)).sum::<f64>() / n;
    let sd = var.sqrt().max(1e-9);
    v.iter_mut().for_each(|x| *x = ((*x as f64 - mean) / sd) as f32);
}

/// Cross-correlation `r[lag] = Σ a[t + lag]·b[t]` for lags in `-(b.len()-1) ..= a.len()-1`,
/// returned with the lag of element 0.
fn xcorr(a: &[f32], b: &[f32]) -> (i64, Vec<f32>) {
    let n = (a.len() + b.len()).next_power_of_two();
    let mut planner = FftPlanner::new();
    let (fwd, inv) = (planner.plan_fft_forward(n), planner.plan_fft_inverse(n));
    let mut fa: Vec<Complex32> = (0..n).map(|i| Complex32::new(a.get(i).copied().unwrap_or(0.0), 0.0)).collect();
    let mut fb: Vec<Complex32> = (0..n).map(|i| Complex32::new(b.get(i).copied().unwrap_or(0.0), 0.0)).collect();
    fwd.process(&mut fa);
    fwd.process(&mut fb);
    for (x, y) in fa.iter_mut().zip(&fb) {
        *x *= y.conj();
    }
    inv.process(&mut fa);
    let lo = -(b.len() as i64 - 1);
    let hi = a.len() as i64 - 1;
    let out = (lo..=hi).map(|lag| fa[lag.rem_euclid(n as i64) as usize].re / n as f32).collect();
    (lo, out)
}

/// Coarse lag of `b` against `a` in envelope frames (b's first frame sits at a-frame `lag`) and
/// its peak-to-sidelobe ratio. `max_lag` limits the search (frames).
pub fn coarse_lag(a: &[f32], b: &[f32], max_lag: Option<i64>) -> Option<(i64, f32)> {
    if a.len() < 8 || b.len() < 8 {
        return None;
    }
    let (lo, r) = xcorr(a, b);
    // Only lags overlapping at least 5 s (or half the shorter clip) are trusted: a sliver of a
    // few onsets lines up by chance somewhere.
    let min_ov = ((5.0 * ENV_HZ) as i64).min(a.len().min(b.len()) as i64 / 2).max(4);
    let ok = |lag: i64| {
        let ov = (a.len() as i64).min(lag + b.len() as i64) - lag.max(0);
        ov >= min_ov && max_lag.is_none_or(|m| lag.abs() <= m)
    };
    // For z-scored envelopes a chance sum over N frames spreads like √N: dividing by √overlap
    // puts every lag on the same footing (dividing by N would favour slivers at the edges).
    let score: Vec<(i64, f32)> = r
        .iter()
        .enumerate()
        .map(|(i, v)| (lo + i as i64, *v))
        .filter(|(lag, _)| ok(*lag))
        .map(|(lag, v)| {
            let ov = (a.len() as i64).min(lag + b.len() as i64) - lag.max(0);
            (lag, v / (ov as f32).sqrt())
        })
        .collect();
    let (best, peak) = score.iter().copied().max_by(|x, y| x.1.total_cmp(&y.1))?;
    let excl = (0.5 * ENV_HZ) as i64;
    let side: Vec<f32> = score.iter().filter(|(l, _)| (l - best).abs() > excl).map(|s| s.1).collect();
    if side.len() < 4 {
        return Some((best, 0.0));
    }
    let m = side.iter().sum::<f32>() / side.len() as f32;
    let sd = (side.iter().map(|v| (v - m).powi(2)).sum::<f32>() / side.len() as f32).sqrt().max(1e-9);
    Some((best, (peak - m) / sd))
}

/// 48 kHz → 8 kHz with a 6-tap box filter (enough for onset timing).
fn decimate(x: &[f32]) -> Vec<f32> {
    let k = (RATE / FINE_RATE) as usize;
    x.chunks_exact(k).map(|c| c.iter().sum::<f32>() / k as f32).collect()
}

/// Refines a lag (seconds, b relative to a) with GCC-PHAT on 8 kHz audio in a window of up to
/// 10 s around b's strongest onsets, searching ±50 ms. Returns the refined lag, or the input.
pub fn fine_lag(a48: &[f32], b48: &[f32], b_env: &[f32], coarse_s: f64) -> f64 {
    let (a, b) = (decimate(a48), decimate(b48));
    let win = (10.0 * FINE_RATE) as usize;
    let search = (0.05 * FINE_RATE) as i64;
    // Centre on the busiest second of b's envelope.
    let per = ENV_HZ as usize;
    let busiest = (0..b_env.len().saturating_sub(per).max(1))
        .max_by(|&i, &j| {
            let e = |k: usize| b_env[k..(k + per).min(b_env.len())].iter().map(|v| v.max(0.0)).sum::<f32>();
            e(i).total_cmp(&e(j))
        })
        .unwrap_or(0);
    let centre = ((busiest as f64 + per as f64 / 2.0) / ENV_HZ * FINE_RATE) as i64;
    let b0 = (centre - win as i64 / 2).clamp(0, (b.len() as i64 - win as i64).max(0));
    let b1 = (b0 + win as i64).min(b.len() as i64);
    let lag0 = (coarse_s * FINE_RATE).round() as i64;
    let a0 = b0 + lag0 - search;
    let a1 = b1 + lag0 + search;
    if a0 < 0 || a1 > a.len() as i64 || b1 - b0 < (FINE_RATE as i64) {
        return coarse_s;
    }
    let (bs, as_) = (&b[b0 as usize..b1 as usize], &a[a0 as usize..a1 as usize]);
    let n = (as_.len() + bs.len()).next_power_of_two();
    let mut planner = FftPlanner::new();
    let (fwd, inv) = (planner.plan_fft_forward(n), planner.plan_fft_inverse(n));
    let mut fa: Vec<Complex32> = (0..n).map(|i| Complex32::new(as_.get(i).copied().unwrap_or(0.0), 0.0)).collect();
    let mut fb: Vec<Complex32> = (0..n).map(|i| Complex32::new(bs.get(i).copied().unwrap_or(0.0), 0.0)).collect();
    fwd.process(&mut fa);
    fwd.process(&mut fb);
    for (x, y) in fa.iter_mut().zip(&fb) {
        let g = *x * y.conj();
        *x = g / g.norm().max(1e-12);
    }
    inv.process(&mut fa);
    // r[k]: bs[t] ≈ as_[t + k]; k = search means exactly the coarse lag.
    let at = |k: i64| fa[k.rem_euclid(n as i64) as usize].re;
    let Some(k) = (0..=2 * search).max_by(|x, y| at(*x).total_cmp(&at(*y))) else { return coarse_s };
    let (l, c, r) = (at(k - 1), at(k), at(k + 1));
    let den = l - 2.0 * c + r;
    let frac = if den.abs() > 1e-12 { (0.5 * (l - r) / den).clamp(-0.5, 0.5) } else { 0.0 };
    (lag0 - search + k) as f64 / FINE_RATE + frac as f64 / FINE_RATE
}

#[derive(Clone, Debug, PartialEq)]
pub struct SyncOptions {
    /// Search limit (seconds); `None` searches every overlap.
    pub max_lag_s: Option<f64>,
    /// Minimum peak-to-sidelobe ratio to accept a lag.
    pub min_psr: f32,
    /// Loop disagreements beyond this many seconds are warned about (one frame).
    pub tolerance_s: f64,
}

impl Default for SyncOptions {
    fn default() -> Self {
        SyncOptions { max_lag_s: None, min_psr: 6.0, tolerance_s: 1.0 / 25.0 }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroupSync {
    /// The longest clip; offsets are relative to its start.
    pub reference: usize,
    /// Start of each clip on the reference's clock (seconds); `None` = could not sync.
    pub offsets: Vec<Option<f64>>,
    /// Confidence of the lag that placed each clip (the reference has none).
    pub psr: Vec<Option<f32>>,
    pub warnings: Vec<String>,
}

/// Syncs mono 48 kHz signals: each to the reference, else chained through any synced clip,
/// then a loop check over synced pairs.
pub fn sync_group(audio: &[Vec<f32>], opts: &SyncOptions) -> GroupSync {
    let n = audio.len();
    let mut out = GroupSync { offsets: vec![None; n], psr: vec![None; n], ..GroupSync::default() };
    if n == 0 {
        return out;
    }
    let env: Vec<Vec<f32>> = audio.iter().map(|a| onset_envelope(a)).collect();
    let reference = (0..n).max_by_key(|&i| audio[i].len()).unwrap_or(0);
    out.reference = reference;
    out.offsets[reference] = Some(0.0);
    let max_lag = opts.max_lag_s.map(|s| (s * ENV_HZ).round() as i64);
    let pair = |i: usize, j: usize| -> Option<(f64, f32)> {
        // Lag of j against i, seconds.
        let (lag, psr) = coarse_lag(&env[i], &env[j], max_lag)?;
        if psr < opts.min_psr {
            return None;
        }
        Some((fine_lag(&audio[i], &audio[j], &env[j], lag as f64 / ENV_HZ), psr))
    };
    let mut progress = true;
    while progress {
        progress = false;
        for j in 0..n {
            if out.offsets[j].is_some() {
                continue;
            }
            // The reference first, then other synced clips.
            let mut anchors: Vec<usize> = (0..n).filter(|&i| out.offsets[i].is_some()).collect();
            anchors.sort_by_key(|&i| i != reference);
            for i in anchors {
                if let (Some(base), Some((lag, psr))) = (out.offsets[i], pair(i, j)) {
                    out.offsets[j] = Some(base + lag);
                    out.psr[j] = Some(psr);
                    progress = true;
                    break;
                }
            }
        }
    }
    for j in 0..n {
        if out.offsets[j].is_none() {
            out.warnings.push(format!("Clip {} could not be synced (no confident audio match).", j + 1));
        }
    }
    // Loop check: pairs that didn't place each other should still agree.
    if n <= 8 {
        for i in 0..n {
            for j in i + 1..n {
                let (Some(oi), Some(oj)) = (out.offsets[i], out.offsets[j]) else { continue };
                if i == reference || j == reference {
                    continue;
                }
                if let Some((lag, _)) = pair(i, j) {
                    let d = (oi + lag - oj).abs();
                    if d > opts.tolerance_s {
                        out.warnings.push(format!("Clips {} and {} disagree by {:.0} ms.", i + 1, j + 1, d * 1000.0));
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Noise bed with irregular clicks (deterministic).
    fn signal(secs: f64, seed: u64) -> Vec<f32> {
        let n = (secs * RATE) as usize;
        let mut s = seed;
        let mut rnd = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((s >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
        };
        let mut v: Vec<f32> = (0..n).map(|_| 0.02 * rnd()).collect();
        let mut t = 0.2;
        while t < secs - 0.1 {
            let i = (t * RATE) as usize;
            for k in 0..300 {
                if i + k < n {
                    v[i + k] += 0.7 * (-(k as f32) / 50.0).exp() * if k % 2 == 0 { 1.0 } else { -1.0 };
                }
            }
            t += 0.3 + 0.8 * (rnd() * 0.5 + 0.5) as f64;
        }
        v
    }

    fn cut(x: &[f32], from_s: f64, len_s: f64) -> Vec<f32> {
        let a = (from_s * RATE) as usize;
        x[a..a + (len_s * RATE) as usize].to_vec()
    }

    #[test]
    fn finds_offsets_with_confidence() {
        let master = signal(30.0, 7);
        let a = cut(&master, 0.0, 20.0);
        let b = cut(&master, 3.4817, 12.0);
        let c = cut(&master, 11.25, 15.0);
        let g = sync_group(&[b, a, c], &SyncOptions::default());
        assert_eq!(g.reference, 1, "the longest clip");
        let off: Vec<f64> = g.offsets.iter().map(|o| o.unwrap()).collect();
        assert!((off[0] - 3.4817).abs() < 0.001, "{off:?}");
        assert!((off[2] - 11.25).abs() < 0.001, "{off:?}");
        assert!(g.psr[0].unwrap() >= 6.0 && g.psr[2].unwrap() >= 6.0, "{:?}", g.psr);
        assert!(g.warnings.is_empty(), "{:?}", g.warnings);
    }

    #[test]
    fn chains_through_a_synced_clip() {
        let master = signal(40.0, 11);
        // a and c don't overlap; b bridges them (6 s of overlap each side).
        let a = cut(&master, 0.0, 20.0);
        let b = cut(&master, 14.0, 16.0);
        let c = cut(&master, 24.0, 12.0);
        let g = sync_group(&[a, b, c], &SyncOptions::default());
        assert_eq!(g.reference, 0, "the longest clip");
        assert!((g.offsets[1].unwrap() - 14.0).abs() < 0.001, "{:?}", g.offsets);
        assert!((g.offsets[2].unwrap() - 24.0).abs() < 0.001, "{:?}", g.offsets);
    }

    #[test]
    fn unrelated_audio_is_not_synced() {
        let a = signal(15.0, 1);
        let b = signal(10.0, 2);
        let g = sync_group(&[a, b], &SyncOptions::default());
        assert_eq!(g.offsets[1], None);
        assert_eq!(g.warnings.len(), 1);
    }

    #[test]
    fn search_limit_is_respected() {
        let master = signal(30.0, 3);
        let a = cut(&master, 0.0, 25.0);
        let b = cut(&master, 9.0, 10.0);
        let g = sync_group(&[a, b], &SyncOptions { max_lag_s: Some(5.0), ..SyncOptions::default() });
        assert_eq!(g.offsets[1], None, "the true lag is outside ±5 s");
    }
}
