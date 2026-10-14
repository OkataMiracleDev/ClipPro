//! Stored measurement columns, quantisation and the per-clip `MeasurementSet` (SPEC §12.3).

use crate::calibration::Calibration;
use serde::{Deserialize, Serialize};

macro_rules! columns {
    ($($name:ident => $key:literal),* $(,)?) => {
        /// A stored per-sample measure. Keys match `[ranges]` in calibration.toml.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
        #[repr(u8)]
        pub enum Col { $($name),* }
        impl Col {
            pub const ALL: &'static [Col] = &[$(Col::$name),*];
            pub fn key(self) -> &'static str { match self { $(Col::$name => $key),* } }
            pub fn from_key(k: &str) -> Option<Col> { match k { $($key => Some(Col::$name),)* _ => None } }
        }
    };
}

columns! {
    NoiseSigma => "noise_sigma",
    SharpFull => "sharp_full",
    SharpCentre => "sharp_centre",
    Tenengrad => "tenengrad",
    DetailRaw => "detail_raw",
    Mean => "mean",
    Std => "std",
    P01 => "p01",
    P05 => "p05",
    P50 => "p50",
    P95 => "p95",
    P99 => "p99",
    ClipBlack => "clip_black",
    ClipWhite => "clip_white",
    NearBlack => "near_black",
    NearWhite => "near_white",
    RowBand => "row_band",
    PairDmean => "pair_dmean",
    WbCast => "wb_cast",
    CamUx => "cam_u_x",
    CamUy => "cam_u_y",
    CamZr => "cam_zr",
    CamWr => "cam_wr",
    CamMag => "cam_mag",
    CamConf => "cam_conf",
    CamDir => "cam_dir",
    AvgUx => "avg_u_x",
    AvgUy => "avg_u_y",
    AvgZr => "avg_zr",
    AvgWr => "avg_wr",
    AvgConf => "avg_conf",
    SubjRaw => "subj_raw",
    ShakeIa => "shake_ia",
    BlurDirRatio => "blur_dir_ratio",
    PxPerFrame => "px_per_frame",
    ResidualRms => "residual_rms",
    MadPair => "mad_pair",
    MadPrev => "mad_prev",
    Chi2Prev => "chi2_prev",
    TiltDeg => "tilt_deg",
    // Temporal pass outputs (ALGORITHMS §2)
    FocusStabRaw => "focus_stab_raw",
    FlickerRaw => "flicker_raw",
    StabRaw => "stab_raw",
    StabConf => "stab_conf",
}

pub const NCOLS: usize = Col::ALL.len();

/// u16 quantisation over `[lo, hi]`; NaN is stored as 0xFFFF.
pub fn quantise(v: f32, [lo, hi]: [f32; 2]) -> u16 {
    if v.is_nan() {
        return u16::MAX;
    }
    let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
    (t * 65534.0).round() as u16
}

pub fn dequantise(q: u16, [lo, hi]: [f32; 2]) -> f32 {
    if q == u16::MAX {
        return f32::NAN;
    }
    lo + (q as f32 / 65534.0) * (hi - lo)
}

/// Per-clip measurements in structure-of-arrays form (in memory, post-temporal pass).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MeasurementSet {
    /// Sample presentation times (µs, relative to the clip's first pts).
    pub time_us: Vec<i64>,
    /// `cols[c as usize][sample]`
    pub cols: Vec<Vec<f32>>,
    /// Per-sample flag bitset (`FlagKind::bit`).
    pub flags: Vec<u32>,
    /// Hard-cut marker per sample (cut lies between sample k-1 and k).
    pub cut_before: Vec<bool>,
    /// Tile grid (cols, rows) and per-sample log10 tile sharpness (row-major).
    pub tile_dims: (u8, u8),
    pub tiles: Vec<f32>,
    /// Motion vector grid for overlays: per sample `grid_w*grid_h` (dx, dy) in level-0 px.
    pub mv_dims: (u8, u8),
    pub mvs: Vec<[i8; 2]>,
    /// 64-bit pHash per sample.
    pub phash: Vec<u64>,
    /// 64-bin luma histogram per sample, flattened (64 per sample, scaled to u8, sum ≈ 255).
    pub hist64: Vec<u8>,
    /// Sample rate used (Hz), source fps.
    pub rate_hz: f64,
    pub fps: f64,
    pub pair_sampling: bool,
    pub active: crate::image::Rect,
    pub working: (u32, u32),
    /// Compute groups that were analysed.
    #[serde(default)]
    pub groups: clippro_types::GroupMask,
}

impl MeasurementSet {
    pub fn new(n: usize) -> Self {
        MeasurementSet { cols: vec![Vec::with_capacity(n); NCOLS], ..Default::default() }
    }

    pub fn len(&self) -> usize {
        self.time_us.len()
    }

    pub fn is_empty(&self) -> bool {
        self.time_us.is_empty()
    }

    #[inline]
    pub fn col(&self, c: Col) -> &[f32] {
        &self.cols[c as usize]
    }

    #[inline]
    pub fn col_mut(&mut self, c: Col) -> &mut Vec<f32> {
        &mut self.cols[c as usize]
    }

    pub fn get(&self, c: Col, k: usize) -> f32 {
        self.cols[c as usize][k]
    }

    pub fn has_flag(&self, k: usize, f: clippro_types::FlagKind) -> bool {
        self.flags[k] & f.bit() != 0
    }

    /// Rounds every column through the quantiser so in-memory values equal stored values.
    pub fn quantise_in_place(&mut self, cal: &Calibration) {
        for &c in Col::ALL {
            let r = cal.range(c);
            for v in self.cols[c as usize].iter_mut() {
                *v = dequantise(quantise(*v, r), r);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantisation_error_bound() {
        let r = [-7.0f32, 0.0];
        for i in 0..1000 {
            let v = -7.0 + 7.0 * i as f32 / 999.0;
            let back = dequantise(quantise(v, r), r);
            assert!((back - v).abs() <= 7.0 / 65534.0 + 1e-6);
        }
        assert!(dequantise(quantise(f32::NAN, r), r).is_nan());
        assert_eq!(dequantise(quantise(100.0, r), r), 0.0);
    }

    #[test]
    fn keys_roundtrip() {
        for &c in Col::ALL {
            assert_eq!(Col::from_key(c.key()), Some(c));
        }
    }
}
