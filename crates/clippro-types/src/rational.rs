//! Exact rational numbers for frame rates and edit-unit time.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

/// A reduced fraction with a positive denominator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rational {
    pub num: i64,
    pub den: i64,
}

pub(crate) fn gcd(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

impl Rational {
    pub const ZERO: Rational = Rational { num: 0, den: 1 };
    pub const ONE: Rational = Rational { num: 1, den: 1 };

    /// Creates a reduced rational. Panics never; a zero denominator yields `0/1`.
    pub fn new(num: i64, den: i64) -> Self {
        if den == 0 {
            return Self::ZERO;
        }
        let g = gcd(num, den).max(1);
        let sign = if den < 0 { -1 } else { 1 };
        Rational { num: sign * num / g, den: sign * den / g }
    }

    pub fn from_int(v: i64) -> Self {
        Rational { num: v, den: 1 }
    }

    pub fn to_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    pub fn is_zero(self) -> bool {
        self.num == 0
    }

    pub fn recip(self) -> Self {
        Rational::new(self.den, self.num)
    }

    pub fn mul(self, o: Rational) -> Rational {
        let g1 = gcd(self.num, o.den).max(1);
        let g2 = gcd(o.num, self.den).max(1);
        Rational::new((self.num / g1) * (o.num / g2), (self.den / g2) * (o.den / g1))
    }

    pub fn div(self, o: Rational) -> Rational {
        self.mul(o.recip())
    }

    pub fn add(self, o: Rational) -> Rational {
        let g = gcd(self.den, o.den).max(1);
        let l = self.den / g * o.den;
        Rational::new(self.num * (l / self.den) + o.num * (l / o.den), l)
    }

    pub fn sub(self, o: Rational) -> Rational {
        self.add(Rational { num: -o.num, den: o.den })
    }

    /// Floor of the value as an integer.
    pub fn floor(self) -> i64 {
        self.num.div_euclid(self.den)
    }

    /// Nearest integer, ties away from zero.
    pub fn round(self) -> i64 {
        let two = 2 * self.num + if self.num >= 0 { self.den } else { -self.den };
        two / (2 * self.den)
    }

    /// Best rational approximation of `v` with denominator ≤ `max_den` (continued fractions).
    pub fn approximate(v: f64, max_den: i64) -> Rational {
        if !v.is_finite() {
            return Rational::ZERO;
        }
        let neg = v < 0.0;
        let mut x = v.abs();
        let (mut h0, mut h1) = (0i64, 1i64);
        let (mut k0, mut k1) = (1i64, 0i64);
        for _ in 0..64 {
            let a = x.floor();
            if a > i64::MAX as f64 / 4.0 {
                break;
            }
            let ai = a as i64;
            let h2 = ai.saturating_mul(h1).saturating_add(h0);
            let k2 = ai.saturating_mul(k1).saturating_add(k0);
            if k2 > max_den {
                break;
            }
            h0 = h1;
            h1 = h2;
            k0 = k1;
            k1 = k2;
            let frac = x - a;
            if frac < 1e-12 {
                break;
            }
            x = 1.0 / frac;
        }
        if k1 == 0 {
            return Rational::ZERO;
        }
        Rational::new(if neg { -h1 } else { h1 }, k1)
    }
}

impl PartialOrd for Rational {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Rational {
    fn cmp(&self, other: &Self) -> Ordering {
        ((self.num as i128) * (other.den as i128)).cmp(&((other.num as i128) * (self.den as i128)))
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

/// A video frame rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FrameRate(pub Rational);

impl Default for FrameRate {
    fn default() -> Self {
        FrameRate(Rational { num: 25, den: 1 })
    }
}

impl FrameRate {
    pub const fn new(num: i64, den: i64) -> Self {
        FrameRate(Rational { num, den })
    }

    /// Normalises common float rates to their exact rational form (23.976 → 24000/1001, …).
    pub fn from_f64(fps: f64) -> Self {
        const NTSC: [i64; 5] = [24, 30, 48, 60, 120];
        for n in NTSC {
            let exact = n as f64 * 1000.0 / 1001.0;
            if (fps - exact).abs() < 0.005 {
                return FrameRate(Rational::new(n * 1000, 1001));
            }
        }
        let r = fps.round();
        if (fps - r).abs() < 0.001 && r > 0.0 {
            return FrameRate(Rational::from_int(r as i64));
        }
        FrameRate(Rational::approximate(fps, 1001))
    }

    pub fn fps(self) -> f64 {
        self.0.to_f64()
    }

    /// Integer timecode base (24 for 23.976, 30 for 29.97, …).
    pub fn timecode_base(self) -> u32 {
        (self.0.to_f64().round() as u32).max(1)
    }

    /// True for x/1001 rates.
    pub fn is_ntsc(self) -> bool {
        self.0.den == 1001
    }

    /// True for rates at which drop-frame timecode is defined (29.97, 59.94).
    pub fn supports_drop_frame(self) -> bool {
        self.is_ntsc() && self.timecode_base() % 30 == 0
    }

    /// Duration of one frame in microseconds (exact when representable, rounded otherwise).
    pub fn frame_duration_us(self) -> f64 {
        1_000_000.0 * self.0.den as f64 / self.0.num as f64
    }

    /// Frame index → time in µs (floor).
    pub fn frames_to_us(self, frames: i64) -> i64 {
        ((frames as i128 * 1_000_000 * self.0.den as i128) / self.0.num as i128) as i64
    }

    /// Time in µs → frame index containing it (floor).
    pub fn us_to_frames(self, us: i64) -> i64 {
        let n = us as i128 * self.0.num as i128;
        let d = 1_000_000i128 * self.0.den as i128;
        n.div_euclid(d) as i64
    }

    /// Time in µs → nearest frame index.
    pub fn us_to_frames_round(self, us: i64) -> i64 {
        let n = us as i128 * self.0.num as i128;
        let d = 1_000_000i128 * self.0.den as i128;
        ((2 * n + d) / (2 * d)) as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduce_and_sign() {
        assert_eq!(Rational::new(4, -8), Rational { num: -1, den: 2 });
        assert_eq!(Rational::new(0, 5), Rational::ZERO);
        assert_eq!(Rational::new(3, 0), Rational::ZERO);
    }

    #[test]
    fn arithmetic() {
        let a = Rational::new(1, 3);
        let b = Rational::new(1, 6);
        assert_eq!(a.add(b), Rational::new(1, 2));
        assert_eq!(a.sub(b), Rational::new(1, 6));
        assert_eq!(a.mul(b), Rational::new(1, 18));
        assert_eq!(a.div(b), Rational::from_int(2));
        assert!(a > b);
        assert_eq!(Rational::new(-7, 2).floor(), -4);
        assert_eq!(Rational::new(7, 2).round(), 4);
        assert_eq!(Rational::new(-7, 2).round(), -4);
    }

    #[test]
    fn frame_rates() {
        assert_eq!(FrameRate::from_f64(23.976), FrameRate::new(24000, 1001));
        assert_eq!(FrameRate::from_f64(29.97), FrameRate::new(30000, 1001));
        assert_eq!(FrameRate::from_f64(59.94006), FrameRate::new(60000, 1001));
        assert_eq!(FrameRate::from_f64(25.0), FrameRate::new(25, 1));
        assert!(FrameRate::new(30000, 1001).supports_drop_frame());
        assert!(!FrameRate::new(24000, 1001).supports_drop_frame());
        let r = FrameRate::new(30000, 1001);
        assert_eq!(r.us_to_frames(r.frames_to_us(12345)), 12345);
        assert_eq!(r.timecode_base(), 30);
    }

    #[test]
    fn approximate() {
        assert_eq!(Rational::approximate(0.5, 100), Rational::new(1, 2));
        assert_eq!(Rational::approximate(29.97002997, 1001), Rational::new(30000, 1001));
    }
}
