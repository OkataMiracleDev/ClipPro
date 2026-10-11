//! Calibration constants (`calibration/calibration.toml`), embedded at compile time.

use serde::Deserialize;
use std::collections::BTreeMap;

pub const EMBEDDED: &str = include_str!("../../../calibration/calibration.toml");

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Calibration {
    pub version: u32,
    pub focus: FocusCal,
    pub exposure: ExposureCal,
    pub motion: MotionCal,
    pub similarity: SimilarityCal,
    pub audio: AudioCal,
    pub takes: TakesCal,
    pub faces: FacesCal,
    pub ranges: BTreeMap<String, [f32; 2]>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FocusCal {
    pub sharp_s0: f32,
    pub sharp_k: f32,
    pub stab_mad_scale: f32,
    pub stab_osc_weight: f32,
    pub hunt_step: f32,
    pub hunt_range: f32,
    pub hunt_min_changes: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExposureCal {
    pub clip_flag_fraction: f32,
    pub flicker_f_lo: f32,
    pub flicker_f_hi: f32,
    pub flicker_pair_lo: f32,
    pub flicker_pair_hi: f32,
    pub flicker_band_lo: f32,
    pub flicker_band_hi: f32,
    pub low_light_p50: f32,
    pub low_light_p95: f32,
    pub low_light_p50_log: f32,
    pub low_light_p95_log: f32,
    pub low_light_noise_ratio: f32,
    pub gf_spread_lo: f32,
    pub gf_spread_hi: f32,
    pub gf_spread_lo_log: f32,
    pub gf_spread_hi_log: f32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MotionCal {
    pub cam_scale: f32,
    pub subj_scale: f32,
    pub stab_scale: f32,
    pub shaky_stab: f32,
    pub shaky_conf: f32,
    pub blur_px_per_frame: f32,
    pub blur_sharp_drop: f32,
    pub blur_dir_ratio: f32,
    pub blur_conf: f32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SimilarityCal {
    pub dup_pair_mad: f32,
    pub dup_prev_mad: f32,
    pub cut_min: f32,
    pub cut_ratio: f32,
    pub cut_conf_max: f32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AudioCal {
    pub noise_lo_db: f32,
    pub noise_hi_db: f32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TakesCal {
    pub group_threshold: f32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FacesCal {
    pub cluster_distance: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum CalibrationError {
    #[error("calibration parse error: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("calibration is missing a range for column '{0}'")]
    MissingRange(String),
}

impl Calibration {
    pub fn parse(text: &str) -> Result<Self, CalibrationError> {
        let c: Calibration = toml::from_str(text)?;
        for col in crate::columns::Col::ALL {
            if !c.ranges.contains_key(col.key()) {
                return Err(CalibrationError::MissingRange(col.key().to_string()));
            }
        }
        Ok(c)
    }

    /// The compiled-in calibration. The embedded file is validated by tests.
    pub fn embedded() -> &'static Calibration {
        use std::sync::OnceLock;
        static CAL: OnceLock<Calibration> = OnceLock::new();
        #[allow(clippy::expect_used)]
        CAL.get_or_init(|| Calibration::parse(EMBEDDED).expect("embedded calibration is valid"))
    }

    pub fn range(&self, col: crate::columns::Col) -> [f32; 2] {
        self.ranges.get(col.key()).copied().unwrap_or([0.0, 1.0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_parses() {
        let c = Calibration::parse(EMBEDDED).unwrap();
        assert_eq!(c.version, 1);
        assert!(c.focus.sharp_k > 0.0);
    }

    #[test]
    fn rejects_missing_key() {
        let broken = EMBEDDED.replace("sharp_k = 0.35", "");
        assert!(Calibration::parse(&broken).is_err());
        let broken = EMBEDDED.replace("noise_sigma = [0.0, 0.25]", "");
        assert!(matches!(Calibration::parse(&broken), Err(CalibrationError::MissingRange(_))));
    }
}
