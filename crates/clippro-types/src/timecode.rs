//! SMPTE timecode with drop-frame support.

use crate::rational::FrameRate;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A timecode, stored as a frame count since 00:00:00:00 at an integer timecode base.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Timecode {
    /// Frames since midnight (always the *real* frame count, i.e. drop-frame labels are skipped).
    pub frames: i64,
    /// Integer timecode base (24, 25, 30, 50, 60, …).
    pub base: u32,
    /// Drop-frame labelling (only meaningful for base 30/60 on NTSC rates).
    pub drop: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TimecodeError {
    #[error("malformed timecode '{0}'")]
    Malformed(String),
    #[error("field out of range in '{0}'")]
    OutOfRange(String),
    #[error("'{0}' is not a valid drop-frame label")]
    DroppedLabel(String),
}

impl Timecode {
    pub fn from_frames(frames: i64, rate: FrameRate, drop: bool) -> Self {
        let drop = drop && rate.supports_drop_frame();
        Timecode { frames, base: rate.timecode_base(), drop }
    }

    pub fn with_base(frames: i64, base: u32, drop: bool) -> Self {
        Timecode { frames, base: base.max(1), drop: drop && base % 30 == 0 }
    }

    fn drop_per_minute(&self) -> i64 {
        if self.drop {
            (self.base / 15) as i64
        } else {
            0
        }
    }

    /// Splits into (hh, mm, ss, ff) labels.
    pub fn components(&self) -> (i64, i64, i64, i64) {
        let fps = self.base as i64;
        let neg = self.frames < 0;
        let mut f = self.frames.abs();
        let d = self.drop_per_minute();
        if d > 0 {
            let per_10min = fps * 600 - d * 9;
            let per_min = fps * 60 - d;
            let ten = f / per_10min;
            let rem = f % per_10min;
            f += d * 9 * ten;
            if rem > d {
                f += d * ((rem - d) / per_min);
            }
        }
        let ff = f % fps;
        let total_s = f / fps;
        let ss = total_s % 60;
        let mm = (total_s / 60) % 60;
        let hh = total_s / 3600;
        if neg {
            (-hh, mm, ss, ff)
        } else {
            (hh, mm, ss, ff)
        }
    }

    /// Builds from labels; rejects labels that drop-frame skips (e.g. 00:01:00;00).
    pub fn from_components(
        hh: i64,
        mm: i64,
        ss: i64,
        ff: i64,
        base: u32,
        drop: bool,
    ) -> Result<Self, TimecodeError> {
        let tc = Timecode::with_base(0, base, drop);
        let fps = tc.base as i64;
        let label = format!("{hh:02}:{mm:02}:{ss:02}{}{ff:02}", if tc.drop { ';' } else { ':' });
        if !(0..60).contains(&mm) || !(0..60).contains(&ss) || !(0..fps).contains(&ff) || hh < 0 {
            return Err(TimecodeError::OutOfRange(label));
        }
        let d = tc.drop_per_minute();
        if d > 0 && ss == 0 && ff < d && mm % 10 != 0 {
            return Err(TimecodeError::DroppedLabel(label));
        }
        let total_min = 60 * hh + mm;
        let frames = fps * 3600 * hh + fps * 60 * mm + fps * ss + ff - d * (total_min - total_min / 10);
        Ok(Timecode { frames, ..tc })
    }

    /// Parses `HH:MM:SS:FF`, `HH:MM:SS;FF` (drop-frame) or `HH;MM;SS;FF`.
    pub fn parse(s: &str, base: u32) -> Result<Self, TimecodeError> {
        let t = s.trim();
        let drop = t.contains(';');
        let parts: Vec<&str> = t.split([':', ';', '.']).collect();
        if parts.len() != 4 {
            return Err(TimecodeError::Malformed(s.to_string()));
        }
        let mut v = [0i64; 4];
        for (i, p) in parts.iter().enumerate() {
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                return Err(TimecodeError::Malformed(s.to_string()));
            }
            v[i] = p.parse().map_err(|_| TimecodeError::Malformed(s.to_string()))?;
        }
        Timecode::from_components(v[0], v[1], v[2], v[3], base, drop)
    }

    pub fn add_frames(self, n: i64) -> Self {
        Timecode { frames: self.frames + n, ..self }
    }
}

impl fmt::Display for Timecode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (hh, mm, ss, ff) = self.components();
        let sep = if self.drop { ';' } else { ':' };
        if hh < 0 || self.frames < 0 {
            write!(f, "-{:02}:{mm:02}:{ss:02}{sep}{ff:02}", hh.abs())
        } else {
            write!(f, "{hh:02}:{mm:02}:{ss:02}{sep}{ff:02}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndf_roundtrip() {
        for base in [24u32, 25, 30, 50, 60] {
            for frames in [0i64, 1, 59, 1440, 86_399, 123_456, 2_000_000] {
                let tc = Timecode::with_base(frames, base, false);
                let parsed = Timecode::parse(&tc.to_string(), base).unwrap();
                assert_eq!(parsed.frames, frames, "base {base} frames {frames} -> {tc}");
            }
        }
    }

    #[test]
    fn df_known_values() {
        // 00:09:59;29 is followed by 00:10:00;00 (no drop at the 10-minute mark).
        let a = Timecode::parse("00:09:59;29", 30).unwrap();
        assert_eq!(a.add_frames(1).to_string(), "00:10:00;00");
        // 00:00:59;29 is followed by 00:01:00;02.
        let b = Timecode::parse("00:00:59;29", 30).unwrap();
        assert_eq!(b.add_frames(1).to_string(), "00:01:00;02");
        // One hour of 29.97 DF is 107892 frames.
        assert_eq!(Timecode::parse("01:00:00;00", 30).unwrap().frames, 107_892);
        // 59.94 DF drops 4 labels.
        let c = Timecode::parse("00:00:59;59", 60).unwrap();
        assert_eq!(c.add_frames(1).to_string(), "00:01:00;04");
        assert!(Timecode::parse("00:01:00;01", 30).is_err());
    }

    #[test]
    fn df_roundtrip_every_frame_for_11_minutes() {
        for f in 0..(30 * 60 * 11) {
            let tc = Timecode::with_base(f, 30, true);
            assert_eq!(Timecode::parse(&tc.to_string(), 30).unwrap().frames, f);
        }
    }

    #[test]
    fn malformed() {
        assert!(Timecode::parse("1:2:3", 25).is_err());
        assert!(Timecode::parse("00:00:00:25", 25).is_err());
        assert!(Timecode::parse("aa:00:00:00", 25).is_err());
    }
}
