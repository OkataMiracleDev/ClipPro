//! Catalogue of user-weightable sub-metrics (ALGORITHMS §3).

use clippro_types::MetricGroup;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SubMetric {
    Sharpness,
    Detail,
    FocusStability,
    GradingFlexibility,
    Flicker,
    Luma,
    Contrast,
    Noise,
    WbCast,
    CameraMotion,
    SubjectMotion,
    MotionStability,
    Loudness,
    Clean,
    NoiseFloor,
    SpeechPresence,
    Wind,
    Hum,
    FaceFocus,
    EyesOpen,
    FaceSize,
    Aesthetic,
    ShotTypeMatch,
}

/// How a sub-metric is scored by default.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SubDefault {
    /// Bipolar preference in [−1, 1] (0 = neutral/excluded).
    Pref(f32),
    /// Off by default (bipolar when switched on).
    Off,
    /// Target-type: (enabled, target, tolerance).
    Target(bool, f32, f32),
}

impl SubMetric {
    pub const ALL: [SubMetric; 23] = [
        SubMetric::Sharpness,
        SubMetric::Detail,
        SubMetric::FocusStability,
        SubMetric::GradingFlexibility,
        SubMetric::Flicker,
        SubMetric::Luma,
        SubMetric::Contrast,
        SubMetric::Noise,
        SubMetric::WbCast,
        SubMetric::CameraMotion,
        SubMetric::SubjectMotion,
        SubMetric::MotionStability,
        SubMetric::Loudness,
        SubMetric::Clean,
        SubMetric::NoiseFloor,
        SubMetric::SpeechPresence,
        SubMetric::Wind,
        SubMetric::Hum,
        SubMetric::FaceFocus,
        SubMetric::EyesOpen,
        SubMetric::FaceSize,
        SubMetric::Aesthetic,
        SubMetric::ShotTypeMatch,
    ];

    pub fn key(self) -> &'static str {
        match self {
            SubMetric::Sharpness => "sharpness",
            SubMetric::Detail => "detail",
            SubMetric::FocusStability => "focus_stability",
            SubMetric::GradingFlexibility => "grading_flexibility",
            SubMetric::Flicker => "flicker",
            SubMetric::Luma => "luma",
            SubMetric::Contrast => "contrast",
            SubMetric::Noise => "noise",
            SubMetric::WbCast => "wb_cast",
            SubMetric::CameraMotion => "camera_motion",
            SubMetric::SubjectMotion => "subject_motion",
            SubMetric::MotionStability => "motion_stability",
            SubMetric::Loudness => "loudness",
            SubMetric::Clean => "clean",
            SubMetric::NoiseFloor => "noise_floor",
            SubMetric::SpeechPresence => "speech_presence",
            SubMetric::Wind => "wind",
            SubMetric::Hum => "hum",
            SubMetric::FaceFocus => "face_focus",
            SubMetric::EyesOpen => "eyes_open",
            SubMetric::FaceSize => "face_size",
            SubMetric::Aesthetic => "aesthetic",
            SubMetric::ShotTypeMatch => "shot_type_match",
        }
    }

    pub fn from_key(k: &str) -> Option<SubMetric> {
        SubMetric::ALL.iter().copied().find(|s| s.key() == k)
    }

    pub fn label(self) -> &'static str {
        match self {
            SubMetric::Sharpness => "Sharpness",
            SubMetric::Detail => "Detail",
            SubMetric::FocusStability => "Focus stability",
            SubMetric::GradingFlexibility => "Grading flexibility",
            SubMetric::Flicker => "Flicker",
            SubMetric::Luma => "Luma",
            SubMetric::Contrast => "Contrast",
            SubMetric::Noise => "Noise",
            SubMetric::WbCast => "White-balance cast",
            SubMetric::CameraMotion => "Camera motion",
            SubMetric::SubjectMotion => "Subject motion",
            SubMetric::MotionStability => "Motion stability",
            SubMetric::Loudness => "Loudness",
            SubMetric::Clean => "Clean (no clipping)",
            SubMetric::NoiseFloor => "Noise floor",
            SubMetric::SpeechPresence => "Speech presence",
            SubMetric::Wind => "Wind",
            SubMetric::Hum => "Hum",
            SubMetric::FaceFocus => "Face in focus",
            SubMetric::EyesOpen => "Eyes open",
            SubMetric::FaceSize => "Face size",
            SubMetric::Aesthetic => "Aesthetic",
            SubMetric::ShotTypeMatch => "Shot-type match",
        }
    }

    /// Labels for the low and high ends of the bipolar slider (what n = 0 / n = 1 means).
    pub fn ends(self) -> (&'static str, &'static str) {
        match self {
            SubMetric::Sharpness => ("Soft", "Sharp"),
            SubMetric::Detail => ("Flat", "Detailed"),
            SubMetric::FocusStability => ("Hunting", "Locked"),
            SubMetric::GradingFlexibility => ("Little latitude", "Lots of latitude"),
            SubMetric::Flicker => ("Steady light", "Flickering"),
            SubMetric::Luma => ("Dark", "Bright"),
            SubMetric::Contrast => ("Flat", "Punchy"),
            SubMetric::Noise => ("Clean", "Noisy"),
            SubMetric::WbCast => ("Neutral", "Colour cast"),
            SubMetric::CameraMotion => ("Still camera", "Moving camera"),
            SubMetric::SubjectMotion => ("Still subject", "Moving subject"),
            SubMetric::MotionStability => ("Shaky", "Steady"),
            SubMetric::Loudness => ("Quiet", "Loud"),
            SubMetric::Clean => ("Clipping", "Clean"),
            SubMetric::NoiseFloor => ("Quiet floor", "Noisy floor"),
            SubMetric::SpeechPresence => ("No speech", "Speech"),
            SubMetric::Wind => ("No wind", "Windy"),
            SubMetric::Hum => ("No hum", "Hum"),
            SubMetric::FaceFocus => ("Soft face", "Sharp face"),
            SubMetric::EyesOpen => ("Eyes closed", "Eyes open"),
            SubMetric::FaceSize => ("Small face", "Large face"),
            SubMetric::Aesthetic => ("Plain", "Pleasing"),
            SubMetric::ShotTypeMatch => ("Other shots", "Chosen shot type"),
        }
    }

    pub fn group(self) -> MetricGroup {
        use SubMetric::*;
        match self {
            Sharpness | Detail | FocusStability => MetricGroup::Focus,
            GradingFlexibility | Flicker | Luma | Contrast | Noise | WbCast => MetricGroup::Exposure,
            CameraMotion | SubjectMotion | MotionStability => MetricGroup::Motion,
            Loudness | Clean | NoiseFloor | SpeechPresence | Wind | Hum => MetricGroup::Audio,
            FaceFocus | EyesOpen | FaceSize | Aesthetic | ShotTypeMatch => MetricGroup::Content,
        }
    }

    /// Target-type sub-metrics (ALGORITHMS §3).
    pub fn is_target(self) -> bool {
        matches!(self, SubMetric::Luma | SubMetric::Contrast | SubMetric::Loudness)
    }

    /// Default state (ALGORITHMS §3 table, "Default state" column).
    pub fn default_state(self) -> SubDefault {
        use SubMetric::*;
        match self {
            Sharpness => SubDefault::Pref(0.7),
            Detail => SubDefault::Pref(0.3),
            FocusStability => SubDefault::Pref(0.5),
            GradingFlexibility => SubDefault::Pref(0.5),
            Flicker => SubDefault::Pref(-0.5),
            Luma => SubDefault::Target(false, 0.45, 0.15),
            Contrast => SubDefault::Target(false, 0.55, 0.20),
            Noise => SubDefault::Pref(-0.2),
            WbCast => SubDefault::Off,
            CameraMotion | SubjectMotion | SpeechPresence => SubDefault::Pref(0.0),
            MotionStability => SubDefault::Pref(0.6),
            Loudness => SubDefault::Target(true, -23.0, 8.0),
            Clean => SubDefault::Pref(0.6),
            NoiseFloor => SubDefault::Pref(-0.3),
            Wind => SubDefault::Pref(-0.5),
            Hum => SubDefault::Pref(-0.3),
            FaceFocus | EyesOpen | FaceSize | Aesthetic | ShotTypeMatch => SubDefault::Off,
        }
    }

    /// Sub-metrics whose score is blended with motion confidence (ALGORITHMS §6.1).
    pub fn confidence_weighted(self) -> bool {
        matches!(self, SubMetric::CameraMotion | SubMetric::SubjectMotion | SubMetric::MotionStability)
    }

    pub fn of_group(g: MetricGroup) -> impl Iterator<Item = SubMetric> {
        SubMetric::ALL.into_iter().filter(move |s| s.group() == g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_roundtrip_and_groups() {
        for s in SubMetric::ALL {
            assert_eq!(SubMetric::from_key(s.key()), Some(s));
        }
        assert_eq!(SubMetric::of_group(MetricGroup::Focus).count(), 3);
        assert_eq!(SubMetric::of_group(MetricGroup::Audio).count(), 6);
    }
}
