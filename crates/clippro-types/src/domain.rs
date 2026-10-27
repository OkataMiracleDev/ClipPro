//! Domain enums shared by every crate and the UI.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id_type {
    ($($name:ident),*) => {$(
        #[cfg_attr(feature = "specta", derive(specta::Type))]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
        #[serde(transparent)]
        pub struct $name(#[cfg_attr(feature = "specta", specta(type = f64))] pub i64);
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}", self.0) }
        }
    )*};
}
id_type!(MediaId, ClipId, BinId, RunId, PresetId, PersonId, OverrideId, SyncGroupId);

/// Metric groups (SPEC §29).
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum MetricGroup {
    Focus,
    Exposure,
    Motion,
    Audio,
    Content,
}

impl MetricGroup {
    pub const ALL: [MetricGroup; 5] = [
        MetricGroup::Focus,
        MetricGroup::Exposure,
        MetricGroup::Motion,
        MetricGroup::Audio,
        MetricGroup::Content,
    ];
    /// Groups measured from video/audio signals (compute profiles).
    pub const COMPUTE: [MetricGroup; 4] =
        [MetricGroup::Focus, MetricGroup::Exposure, MetricGroup::Motion, MetricGroup::Audio];

    pub fn bit(self) -> u8 {
        1 << (self as u8)
    }

    pub fn label(self) -> &'static str {
        match self {
            MetricGroup::Focus => "Focus",
            MetricGroup::Exposure => "Exposure",
            MetricGroup::Motion => "Motion",
            MetricGroup::Audio => "Audio",
            MetricGroup::Content => "Content",
        }
    }
}

/// Bitmask of enabled compute groups.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupMask(pub u8);

impl GroupMask {
    pub const ALL_COMPUTE: GroupMask = GroupMask(0b1111);
    pub fn contains(self, g: MetricGroup) -> bool {
        self.0 & g.bit() != 0
    }
    pub fn with(self, g: MetricGroup, on: bool) -> GroupMask {
        if on {
            GroupMask(self.0 | g.bit())
        } else {
            GroupMask(self.0 & !g.bit())
        }
    }
    pub fn is_empty(self) -> bool {
        self.0 & 0b1111 == 0
    }
}

impl Default for GroupMask {
    fn default() -> Self {
        GroupMask::ALL_COMPUTE
    }
}

/// Every flag ClipPro can raise (SPEC §5.3, ALGORITHMS §2).
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FlagKind {
    Clipping = 0,
    Duplicate = 1,
    SolidColour = 2,
    HardCut = 3,
    MotionBlur = 4,
    Shaky = 5,
    LowLight = 6,
    FocusHunting = 7,
    ColourBars = 8,
    HorizonTilt = 9,
    AudioClipping = 10,
    Silence = 11,
    Wind = 12,
    Hum = 13,
    DeadChannel = 14,
    ChannelImbalance = 15,
    NoFace = 16,
    Flub = 17,
}

impl FlagKind {
    pub const ALL: [FlagKind; 18] = [
        FlagKind::Clipping,
        FlagKind::Duplicate,
        FlagKind::SolidColour,
        FlagKind::HardCut,
        FlagKind::MotionBlur,
        FlagKind::Shaky,
        FlagKind::LowLight,
        FlagKind::FocusHunting,
        FlagKind::ColourBars,
        FlagKind::HorizonTilt,
        FlagKind::AudioClipping,
        FlagKind::Silence,
        FlagKind::Wind,
        FlagKind::Hum,
        FlagKind::DeadChannel,
        FlagKind::ChannelImbalance,
        FlagKind::NoFace,
        FlagKind::Flub,
    ];

    pub fn bit(self) -> u32 {
        1u32 << (self as u32)
    }

    pub fn key(self) -> &'static str {
        match self {
            FlagKind::Clipping => "clipping",
            FlagKind::Duplicate => "duplicate",
            FlagKind::SolidColour => "solid_colour",
            FlagKind::HardCut => "hard_cut",
            FlagKind::MotionBlur => "motion_blur",
            FlagKind::Shaky => "shaky",
            FlagKind::LowLight => "low_light",
            FlagKind::FocusHunting => "focus_hunting",
            FlagKind::ColourBars => "colour_bars",
            FlagKind::HorizonTilt => "horizon_tilt",
            FlagKind::AudioClipping => "audio_clipping",
            FlagKind::Silence => "silence",
            FlagKind::Wind => "wind",
            FlagKind::Hum => "hum",
            FlagKind::DeadChannel => "dead_channel",
            FlagKind::ChannelImbalance => "channel_imbalance",
            FlagKind::NoFace => "no_face",
            FlagKind::Flub => "flub",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FlagKind::Clipping => "Clipping",
            FlagKind::Duplicate => "Duplicate",
            FlagKind::SolidColour => "Solid colour",
            FlagKind::HardCut => "Hard cut",
            FlagKind::MotionBlur => "Motion blur",
            FlagKind::Shaky => "Shaky",
            FlagKind::LowLight => "Low light",
            FlagKind::FocusHunting => "Focus hunting",
            FlagKind::ColourBars => "Colour bars",
            FlagKind::HorizonTilt => "Horizon tilt",
            FlagKind::AudioClipping => "Audio clipping",
            FlagKind::Silence => "Silence",
            FlagKind::Wind => "Wind",
            FlagKind::Hum => "Hum",
            FlagKind::DeadChannel => "Dead channel",
            FlagKind::ChannelImbalance => "Channel imbalance",
            FlagKind::NoFace => "No face",
            FlagKind::Flub => "Flub",
        }
    }

    pub fn from_key(k: &str) -> Option<FlagKind> {
        FlagKind::ALL.iter().copied().find(|f| f.key() == k)
    }

    /// Tier used when a preset doesn't mention the flag (ALGORITHMS §9 legend).
    pub fn default_tier(self) -> Tier {
        match self {
            FlagKind::Clipping
            | FlagKind::MotionBlur
            | FlagKind::Shaky
            | FlagKind::LowLight => Tier::Disfavour,
            FlagKind::Duplicate | FlagKind::SolidColour => Tier::ForceExclude,
            _ => Tier::Neutral,
        }
    }
}

/// What a flag does to scoring (ALGORITHMS §6.4).
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    ForceExclude,
    Disfavour,
    #[default]
    Neutral,
    Favour,
    ForceInclude,
}

impl Tier {
    pub fn multiplier(self) -> f32 {
        match self {
            Tier::Disfavour => 0.6,
            Tier::Favour => 1.2,
            _ => 1.0,
        }
    }
}

/// Sample-rate presets (ALGORITHMS §0.1).
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SampleRate {
    Draft,
    Fast,
    #[default]
    Standard,
    Detailed,
    Thorough,
    All,
}

impl SampleRate {
    /// Samples per second, or `None` for every frame.
    pub fn hz(self) -> Option<f64> {
        match self {
            SampleRate::Draft => Some(1.0),
            SampleRate::Fast => Some(2.0),
            SampleRate::Standard => Some(4.0),
            SampleRate::Detailed => Some(8.0),
            SampleRate::Thorough => Some(16.0),
            SampleRate::All => None,
        }
    }

    pub fn parse(s: &str) -> Option<SampleRate> {
        Some(match s.trim().to_lowercase().as_str() {
            "draft" | "1" => SampleRate::Draft,
            "fast" | "2" => SampleRate::Fast,
            "standard" | "4" => SampleRate::Standard,
            "detailed" | "8" => SampleRate::Detailed,
            "thorough" | "16" => SampleRate::Thorough,
            "all" | "every" => SampleRate::All,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            SampleRate::Draft => "Draft",
            SampleRate::Fast => "Fast",
            SampleRate::Standard => "Standard",
            SampleRate::Detailed => "Detailed",
            SampleRate::Thorough => "Thorough",
            SampleRate::All => "All frames",
        }
    }
}

/// Log/colour profiles for preview and log-aware exposure (SPEC VIE-6).
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ColourProfile {
    #[default]
    Rec709,
    SLog3,
    LogC3,
    LogC4,
    VLog,
    CLog2,
    CLog3,
    NLog,
    FLog2,
    DLog,
    AppleLog,
    Log3G10,
    BmdFilmGen5,
}

impl ColourProfile {
    pub fn is_log(self) -> bool {
        !matches!(self, ColourProfile::Rec709)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_keys_roundtrip() {
        for f in FlagKind::ALL {
            assert_eq!(FlagKind::from_key(f.key()), Some(f));
        }
        let mut bits = 0u32;
        for f in FlagKind::ALL {
            assert_eq!(bits & f.bit(), 0);
            bits |= f.bit();
        }
    }

    #[test]
    fn group_mask() {
        let m = GroupMask::ALL_COMPUTE.with(MetricGroup::Motion, false);
        assert!(!m.contains(MetricGroup::Motion));
        assert!(m.contains(MetricGroup::Focus));
        assert!(!m.is_empty());
        assert!(GroupMask(0).is_empty());
    }
}
