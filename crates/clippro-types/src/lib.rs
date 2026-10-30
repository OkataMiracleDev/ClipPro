//! Shared data types for ClipPro: rational time, timecode, typed-value parsing and domain enums.
//! No IO lives here.

pub mod domain;
pub mod paths;
pub mod rational;
pub mod timecode;
pub mod typed;

pub use domain::*;
pub use rational::{FrameRate, Rational};
pub use timecode::{Timecode, TimecodeError};

/// Microseconds on a clip's presentation timeline.
pub type TimeUs = i64;

pub const US_PER_SEC: i64 = 1_000_000;

pub fn secs_to_us(s: f64) -> TimeUs {
    (s * 1_000_000.0).round() as TimeUs
}

pub fn us_to_secs(us: TimeUs) -> f64 {
    us as f64 / 1_000_000.0
}
