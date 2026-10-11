//! Small numeric helpers shared by the kernels.

/// Hermite smoothstep: 0 for x ≤ a, 1 for x ≥ b.
#[inline]
pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    if x.is_nan() {
        return 0.0;
    }
    if b <= a {
        return if x >= b { 1.0 } else { 0.0 };
    }
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
pub fn logistic(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Percentile of an ascending-sorted slice with linear interpolation (q in [0,1]).
pub fn percentile_sorted(v: &[f32], q: f64) -> f32 {
    match v.len() {
        0 => 0.0,
        1 => v[0],
        n => {
            let pos = q.clamp(0.0, 1.0) * (n - 1) as f64;
            let i = pos.floor() as usize;
            let f = (pos - i as f64) as f32;
            if i + 1 >= n {
                v[n - 1]
            } else {
                v[i] + (v[i + 1] - v[i]) * f
            }
        }
    }
}

/// Weighted percentile: values sorted ascending, interpolated on cumulative-weight midpoints.
pub fn weighted_percentile(pairs: &mut [(f32, f32)], q: f64) -> f32 {
    if pairs.is_empty() {
        return 0.0;
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f64 = pairs.iter().map(|p| p.1 as f64).sum();
    if total <= 0.0 {
        return pairs[pairs.len() / 2].0;
    }
    let mut cum = 0f64;
    let mids: Vec<f64> = pairs
        .iter()
        .map(|p| {
            let m = (cum + p.1 as f64 / 2.0) / total;
            cum += p.1 as f64;
            m
        })
        .collect();
    if q <= mids[0] {
        return pairs[0].0;
    }
    for i in 1..pairs.len() {
        if q <= mids[i] {
            let t = ((q - mids[i - 1]) / (mids[i] - mids[i - 1]).max(1e-12)) as f32;
            return pairs[i - 1].0 + (pairs[i].0 - pairs[i - 1].0) * t;
        }
    }
    pairs[pairs.len() - 1].0
}

/// Exact median (copies and selects). NaNs are ignored; empty → NaN.
pub fn median(v: &[f32]) -> f32 {
    let mut c: Vec<f32> = v.iter().copied().filter(|x| !x.is_nan()).collect();
    if c.is_empty() {
        return f32::NAN;
    }
    c.sort_by(f32::total_cmp);
    let n = c.len();
    if n % 2 == 1 {
        c[n / 2]
    } else {
        0.5 * (c[n / 2 - 1] + c[n / 2])
    }
}

/// Median absolute deviation.
pub fn mad(v: &[f32]) -> f32 {
    let m = median(v);
    if m.is_nan() {
        return 0.0;
    }
    let d: Vec<f32> = v.iter().filter(|x| !x.is_nan()).map(|x| (x - m).abs()).collect();
    median(&d)
}

/// Deterministic xoshiro256** RNG (ALGORITHMS §1.4.2).
pub struct Xoshiro256 {
    s: [u64; 4],
}

impl Xoshiro256 {
    pub fn seed(seed: u64) -> Self {
        let mut sm = SplitMix(seed);
        Xoshiro256 { s: [sm.next(), sm.next(), sm.next(), sm.next()] }
    }

    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// Uniform integer in [0, n).
    pub fn below(&mut self, n: usize) -> usize {
        ((self.next_u64() >> 11) as f64 / (1u64 << 53) as f64 * n as f64) as usize % n.max(1)
    }
}

/// SplitMix64, used for seeding and in tests.
pub struct SplitMix(pub u64);

impl SplitMix {
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn f32(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Standard normal via Box–Muller.
    pub fn gauss(&mut self) -> f32 {
        let u1 = ((self.next() >> 11) as f64 + 1.0) / ((1u64 << 53) as f64 + 1.0);
        let u2 = (self.next() >> 11) as f64 / (1u64 << 53) as f64;
        ((-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()) as f32
    }
}

/// FNV-1a style mix of two values into a seed.
pub fn hash2(a: u64, b: u64) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for v in [a, b] {
        for byte in v.to_le_bytes() {
            h ^= byte as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothstep_edges() {
        assert_eq!(smoothstep(0.0, 1.0, -1.0), 0.0);
        assert_eq!(smoothstep(0.0, 1.0, 2.0), 1.0);
        assert!((smoothstep(0.0, 1.0, 0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn percentiles() {
        let v = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile_sorted(&v, 0.5), 3.0);
        assert_eq!(percentile_sorted(&v, 0.25), 2.0);
        assert_eq!(median(&[3.0, 1.0, 2.0, f32::NAN]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), 2.5);
        let mut p = vec![(1.0, 1.0), (2.0, 1.0), (3.0, 1.0)];
        assert!((weighted_percentile(&mut p, 0.5) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn rng_deterministic() {
        let mut a = Xoshiro256::seed(42);
        let mut b = Xoshiro256::seed(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        for _ in 0..1000 {
            assert!(a.below(7) < 7);
        }
    }
}
