//! Preset TOML format (SPEC §13). Unknown keys are preserved on round-trip.

use clippro_types::{FlagKind, Tier};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type Extra = BTreeMap<String, toml::Value>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Preset {
    pub preset: PresetMeta,
    #[serde(default)]
    pub thresholds: Thresholds,
    #[serde(default)]
    pub weights: Weights,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub focus: BTreeMap<String, SubPref>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub exposure: BTreeMap<String, SubPref>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub motion: BTreeMap<String, SubPref>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub audio: BTreeMap<String, SubPref>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub content: BTreeMap<String, SubPref>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub flags: BTreeMap<String, Tier>,
    #[serde(default)]
    pub camera_filter: CameraFilterToml,
    #[serde(default)]
    pub advanced: Advanced,
    #[serde(default)]
    pub switches: Switches,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct PresetMeta {
    pub name: String,
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default)]
    pub description: String,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Thresholds {
    /// [low, high] in 0–1.
    pub score: [f32; 2],
    /// [min, max] as typed durations ("3s", "none").
    pub segment_length: [String; 2],
    /// Count or "none".
    pub max_per_clip: String,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            score: [0.55, 1.0],
            segment_length: ["2s".into(), "none".into()],
            max_per_clip: "none".into(),
            extra: Extra::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Weights {
    #[serde(default)]
    pub focus: f32,
    #[serde(default)]
    pub exposure: f32,
    #[serde(default)]
    pub motion: f32,
    #[serde(default)]
    pub audio: f32,
    #[serde(default)]
    pub content: f32,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

impl Default for Weights {
    fn default() -> Self {
        Weights { focus: 35.0, exposure: 30.0, motion: 25.0, audio: 10.0, content: 0.0, extra: Extra::new() }
    }
}

/// A sub-metric preference: bipolar number, `false` (off), or a target spec.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(untagged)]
pub enum SubPref {
    Pref(f32),
    Switch(bool),
    Target(TargetPref),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct TargetPref {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(alias = "target_lufs")]
    pub target: f32,
    #[serde(alias = "tolerance_lu")]
    pub tolerance: f32,
    #[serde(default = "unit")]
    pub strength: f32,
}

fn yes() -> bool {
    true
}
fn unit() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CameraFilterToml {
    #[serde(default)]
    pub enabled: bool,
    /// Puck radius 0–1 (0 = direction unconstrained).
    #[serde(default)]
    pub radius: f32,
    /// Puck direction in degrees (0 = pan right, 90 = tilt up).
    #[serde(default)]
    pub direction: f32,
    /// Zoom slider −1..1 (positive = zoom in).
    #[serde(default)]
    pub zoom: f32,
    /// Rotate slider −1..1 (positive = counter-clockwise).
    #[serde(default)]
    pub rotate: f32,
    #[serde(default)]
    pub static_only: bool,
    #[serde(default)]
    pub mirror: bool,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

impl Default for CameraFilterToml {
    fn default() -> Self {
        CameraFilterToml {
            enabled: false,
            radius: 0.0,
            direction: 0.0,
            zoom: 0.0,
            rotate: 0.0,
            static_only: false,
            mirror: false,
            extra: Extra::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LongSegmentMode {
    #[default]
    Split,
    TrimBest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Advanced {
    #[serde(default = "gap_default")]
    pub gap_tolerance: String,
    #[serde(default)]
    pub centre_weighting: f32,
    #[serde(default)]
    pub stability_emphasis: f32,
    #[serde(default = "handles_default")]
    pub handles_samples: u32,
    #[serde(default)]
    pub snap_to_sentences: bool,
    #[serde(default)]
    pub long_segment_mode: LongSegmentMode,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

fn gap_default() -> String {
    "0.75s".into()
}
fn handles_default() -> u32 {
    3
}

impl Default for Advanced {
    fn default() -> Self {
        Advanced {
            gap_tolerance: gap_default(),
            centre_weighting: 0.2,
            stability_emphasis: 0.4,
            handles_samples: 3,
            snap_to_sentences: false,
            long_segment_mode: LongSegmentMode::Split,
            extra: Extra::new(),
        }
    }
}

/// Filter master switches (SPEC TUN-7).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Switches {
    #[serde(default = "yes")]
    pub metrics: bool,
    #[serde(default = "yes")]
    pub flags: bool,
    #[serde(default = "yes")]
    pub camera_filter: bool,
}

impl Default for Switches {
    fn default() -> Self {
        Switches { metrics: true, flags: true, camera_filter: true }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    #[error("preset TOML error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("preset serialisation error: {0}")]
    Ser(#[from] toml::ser::Error),
    #[error("{0}")]
    Invalid(String),
}

impl Preset {
    pub fn parse(text: &str) -> Result<Preset, PresetError> {
        let p: Preset = toml::from_str(text)?;
        p.validate()?;
        Ok(p)
    }

    pub fn to_toml(&self) -> Result<String, PresetError> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// Checks value ranges and known keys; unknown sub-metric or flag keys are errors.
    pub fn validate(&self) -> Result<(), PresetError> {
        let bad = |m: String| Err(PresetError::Invalid(m));
        let [lo, hi] = self.thresholds.score;
        if !(0.0..=1.0).contains(&lo) || !(0.0..=1.0).contains(&hi) || lo > hi {
            return bad(format!("score thresholds must satisfy 0 ≤ low ≤ high ≤ 1, got [{lo}, {hi}]"));
        }
        let lmin = clippro_types::typed::parse_duration(&self.thresholds.segment_length[0])
            .map_err(|e| PresetError::Invalid(format!("segment_length min: {e}")))?;
        let lmax = clippro_types::typed::parse_duration(&self.thresholds.segment_length[1])
            .map_err(|e| PresetError::Invalid(format!("segment_length max: {e}")))?;
        if let (Some(a), Some(b)) = (lmin, lmax) {
            if a > b {
                return bad("segment_length min is greater than max".into());
            }
        }
        if lmin.is_none() {
            return bad("segment_length min can't be unlimited".into());
        }
        clippro_types::typed::parse_count(&self.thresholds.max_per_clip)
            .map_err(|e| PresetError::Invalid(format!("max_per_clip: {e}")))?;
        clippro_types::typed::parse_duration(&self.advanced.gap_tolerance)
            .map_err(|e| PresetError::Invalid(format!("gap_tolerance: {e}")))?
            .ok_or_else(|| PresetError::Invalid("gap_tolerance can't be unlimited".into()))?;
        for w in [self.weights.focus, self.weights.exposure, self.weights.motion, self.weights.audio, self.weights.content] {
            if !(0.0..=1000.0).contains(&w) {
                return bad(format!("weights must be in 0–1000, got {w}"));
            }
        }
        use crate::submetric::SubMetric;
        for (group, map) in [
            ("focus", &self.focus),
            ("exposure", &self.exposure),
            ("motion", &self.motion),
            ("audio", &self.audio),
            ("content", &self.content),
        ] {
            for (k, v) in map {
                let Some(sm) = SubMetric::from_key(k) else {
                    return bad(format!("unknown sub-metric '{group}.{k}'"));
                };
                if sm.group().label().to_lowercase() != group {
                    return bad(format!("sub-metric '{k}' doesn't belong to [{group}]"));
                }
                if let SubPref::Pref(p) = v {
                    if !(-1.0..=1.0).contains(p) {
                        return bad(format!("{group}.{k} must be in −1..1, got {p}"));
                    }
                }
            }
        }
        for k in self.flags.keys() {
            if FlagKind::from_key(k).is_none() {
                return bad(format!("unknown flag '{k}'"));
            }
        }
        let c = &self.camera_filter;
        if !(0.0..=1.0).contains(&c.radius) || !(-1.0..=1.0).contains(&c.zoom) || !(-1.0..=1.0).contains(&c.rotate)
        {
            return bad("camera_filter values out of range".into());
        }
        for v in [self.advanced.centre_weighting, self.advanced.stability_emphasis] {
            if !(0.0..=1.0).contains(&v) {
                return bad("advanced weights must be in 0–1".into());
            }
        }
        Ok(())
    }
}

/// Built-in presets (ALGORITHMS §9), embedded from `presets/*.toml`.
pub const BUILTIN: &[(&str, &str)] = &[
    ("all-rounder", include_str!("../../../presets/all-rounder.toml")),
    ("talking-head", include_str!("../../../presets/talking-head.toml")),
    ("social-media", include_str!("../../../presets/social-media.toml")),
    ("cleanup", include_str!("../../../presets/cleanup.toml")),
    ("action-sport", include_str!("../../../presets/action-sport.toml")),
    ("long-takes", include_str!("../../../presets/long-takes.toml")),
    ("drone-aerial", include_str!("../../../presets/drone-aerial.toml")),
    ("whip-pans", include_str!("../../../presets/whip-pans.toml")),
    ("locked-off", include_str!("../../../presets/locked-off.toml")),
    ("twist", include_str!("../../../presets/twist.toml")),
    ("tilt", include_str!("../../../presets/tilt.toml")),
    ("pan", include_str!("../../../presets/pan.toml")),
    ("interview", include_str!("../../../presets/interview.toml")),
    ("wedding-event", include_str!("../../../presets/wedding-event.toml")),
    ("b-roll", include_str!("../../../presets/b-roll.toml")),
    ("documentary-verite", include_str!("../../../presets/documentary-verite.toml")),
    ("product-tabletop", include_str!("../../../presets/product-tabletop.toml")),
    ("gimbal-walk", include_str!("../../../presets/gimbal-walk.toml")),
    ("audio-first", include_str!("../../../presets/audio-first.toml")),
];

/// All built-in presets, parsed.
pub fn builtins() -> Vec<Preset> {
    BUILTIN.iter().filter_map(|(_, t)| Preset::parse(t).ok()).collect()
}

pub fn builtin(name: &str) -> Option<Preset> {
    let want = name.trim().to_lowercase();
    builtins().into_iter().find(|p| {
        p.preset.name.to_lowercase() == want
            || p.preset.name.to_lowercase().replace([' ', '/'], "-").replace("--", "-") == want
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_parses_and_validates() {
        for (file, text) in BUILTIN {
            let p = Preset::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
            assert!(!p.preset.name.is_empty());
        }
        assert_eq!(builtins().len(), BUILTIN.len());
    }

    #[test]
    fn unknown_keys_survive_roundtrip() {
        let text = format!("{}\n[future_section]\nanswer = 42\n", BUILTIN[0].1)
            .replace("[preset]\n", "[preset]\nfuture_meta = \"x\"\n");
        let p = Preset::parse(&text).unwrap();
        let back = Preset::parse(&p.to_toml().unwrap()).unwrap();
        assert_eq!(p, back);
        let out = p.to_toml().unwrap();
        assert!(out.contains("future_section") && out.contains("answer = 42"));
        assert!(out.contains("future_meta"));
    }

    #[test]
    fn rejects_bad_values() {
        let t = BUILTIN[0].1.replace("score = [0.55, 1.0]", "score = [0.9, 0.2]");
        assert!(Preset::parse(&t).is_err());
        let t = format!("{}\n", BUILTIN[0].1).replace("[focus]\n", "[focus]\nbogus = 0.5\n");
        assert!(Preset::parse(&t).is_err());
    }

    #[test]
    fn lookup_by_name() {
        assert!(builtin("Talking Head").is_some());
        assert!(builtin("talking-head").is_some());
        assert!(builtin("Drone / Aerial").is_some());
        assert!(builtin("nope").is_none());
    }
}
