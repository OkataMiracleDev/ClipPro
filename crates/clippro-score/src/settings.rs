//! Resolved, typed scoring settings (what the engine consumes). Built from a `Preset`.

use crate::preset::{LongSegmentMode, Preset, PresetError, SubPref};
use crate::submetric::{SubDefault, SubMetric};
use clippro_types::typed::{parse_count, parse_duration};
use clippro_types::{FlagKind, MetricGroup, Tier};
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

/// One sub-metric's state. Bipolar sub-metrics use `pref`; target-type ones use `target`/`tol`.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubSetting {
    pub enabled: bool,
    pub pref: f32,
    pub target: f32,
    pub tol: f32,
    pub strength: f32,
}

impl SubSetting {
    /// Weight ω in the group blend (0 = excluded).
    pub fn weight(&self, sm: SubMetric) -> f32 {
        if !self.enabled {
            0.0
        } else if sm.is_target() {
            self.strength.max(0.0)
        } else {
            self.pref.abs()
        }
    }
}

#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct CameraFilter {
    pub enabled: bool,
    pub radius: f32,
    pub direction: f32,
    pub zoom: f32,
    pub rotate: f32,
    pub static_only: bool,
    pub mirror: bool,
}

#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MasterSwitches {
    pub metrics: bool,
    pub flags: bool,
    pub camera_filter: bool,
}

#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub preset_name: String,
    pub score_lo: f32,
    pub score_hi: f32,
    pub len_min_s: f64,
    pub len_max_s: Option<f64>,
    pub max_per_clip: Option<u32>,
    /// Indexed by `MetricGroup as usize`.
    pub weights: [f32; 5],
    /// Indexed by `SubMetric as usize`.
    pub subs: Vec<SubSetting>,
    /// Indexed by `FlagKind as usize`.
    pub tiers: Vec<Tier>,
    pub camera: CameraFilter,
    pub gap_s: f64,
    pub centre_weighting: f32,
    pub stability_emphasis: f32,
    pub handles_samples: u32,
    pub long_mode: LongSegmentMode,
    pub snap_to_sentences: bool,
    pub switches: MasterSwitches,
    /// Shot types rewarded by the Shot-type match sub-metric.
    pub shot_types: Vec<String>,
    /// Keep only the best select of each take group (TUN-13; needs the AI modules).
    #[serde(default)]
    pub best_take_only: bool,
}

fn default_sub(sm: SubMetric) -> SubSetting {
    match sm.default_state() {
        SubDefault::Pref(p) => SubSetting { enabled: true, pref: p, target: 0.5, tol: 0.2, strength: 1.0 },
        SubDefault::Off => SubSetting { enabled: false, pref: 0.0, target: 0.5, tol: 0.2, strength: 1.0 },
        SubDefault::Target(on, t, tol) => SubSetting { enabled: on, pref: 0.0, target: t, tol, strength: 1.0 },
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            preset_name: "Custom".into(),
            score_lo: 0.55,
            score_hi: 1.0,
            len_min_s: 2.0,
            len_max_s: None,
            max_per_clip: None,
            weights: [35.0, 30.0, 25.0, 10.0, 0.0],
            subs: SubMetric::ALL.iter().map(|s| default_sub(*s)).collect(),
            tiers: FlagKind::ALL.iter().map(|f| f.default_tier()).collect(),
            camera: CameraFilter::default(),
            gap_s: 0.75,
            centre_weighting: 0.2,
            stability_emphasis: 0.4,
            handles_samples: 3,
            long_mode: LongSegmentMode::Split,
            snap_to_sentences: false,
            switches: MasterSwitches { metrics: true, flags: true, camera_filter: true },
            shot_types: Vec::new(),
            best_take_only: false,
        }
    }
}

impl Settings {
    pub fn from_preset(p: &Preset) -> Result<Settings, PresetError> {
        p.validate()?;
        let mut s = Settings { preset_name: p.preset.name.clone(), ..Settings::default() };
        let inv = |m: String| PresetError::Invalid(m);
        s.score_lo = p.thresholds.score[0];
        s.score_hi = p.thresholds.score[1];
        s.len_min_s = parse_duration(&p.thresholds.segment_length[0])
            .map_err(|e| inv(e.to_string()))?
            .unwrap_or(0.0);
        s.len_max_s = parse_duration(&p.thresholds.segment_length[1]).map_err(|e| inv(e.to_string()))?;
        s.max_per_clip = parse_count(&p.thresholds.max_per_clip).map_err(|e| inv(e.to_string()))?;
        let w = &p.weights;
        s.weights = [w.focus, w.exposure, w.motion, w.audio, w.content];
        for map in [&p.focus, &p.exposure, &p.motion, &p.audio, &p.content] {
            for (k, v) in map {
                let Some(sm) = SubMetric::from_key(k) else { continue };
                let cur = &mut s.subs[sm as usize];
                match v {
                    SubPref::Pref(x) => {
                        cur.enabled = true;
                        cur.pref = *x;
                    }
                    SubPref::Switch(on) => {
                        cur.enabled = *on;
                        if *on && cur.pref == 0.0 && !sm.is_target() {
                            cur.pref = 0.5;
                        }
                    }
                    SubPref::Target(t) => {
                        cur.enabled = t.enabled;
                        cur.target = t.target;
                        cur.tol = t.tolerance.max(1e-6);
                        cur.strength = t.strength;
                    }
                }
            }
        }
        for (k, tier) in &p.flags {
            if let Some(f) = FlagKind::from_key(k) {
                s.tiers[f as usize] = *tier;
            }
        }
        let c = &p.camera_filter;
        s.camera = CameraFilter {
            enabled: c.enabled,
            radius: c.radius,
            direction: c.direction,
            zoom: c.zoom,
            rotate: c.rotate,
            static_only: c.static_only,
            mirror: c.mirror,
        };
        let a = &p.advanced;
        s.gap_s = parse_duration(&a.gap_tolerance).map_err(|e| inv(e.to_string()))?.unwrap_or(0.0);
        s.centre_weighting = a.centre_weighting;
        s.stability_emphasis = a.stability_emphasis;
        s.handles_samples = a.handles_samples;
        s.long_mode = a.long_segment_mode;
        s.snap_to_sentences = a.snap_to_sentences;
        s.switches = MasterSwitches {
            metrics: p.switches.metrics,
            flags: p.switches.flags,
            camera_filter: p.switches.camera_filter,
        };
        Ok(s)
    }

    pub fn sub(&self, sm: SubMetric) -> &SubSetting {
        &self.subs[sm as usize]
    }

    pub fn sub_mut(&mut self, sm: SubMetric) -> &mut SubSetting {
        &mut self.subs[sm as usize]
    }

    pub fn weight(&self, g: MetricGroup) -> f32 {
        self.weights[g as usize]
    }

    pub fn tier(&self, f: FlagKind) -> Tier {
        self.tiers[f as usize]
    }

    /// True if the group has weight but every sub-metric is off (UI amber state).
    pub fn group_starved(&self, g: MetricGroup) -> bool {
        self.weight(g) > 0.0 && SubMetric::of_group(g).all(|sm| self.sub(sm).weight(sm) == 0.0)
    }

    /// Key for stages S1–S4 (per-sample scores).
    pub fn key_frame(&self) -> u64 {
        let mut h = KeyHasher::default();
        for s in &self.subs {
            (s.enabled, s.pref.to_bits(), s.target.to_bits(), s.tol.to_bits(), s.strength.to_bits()).hash(&mut h);
        }
        for w in self.weights {
            w.to_bits().hash(&mut h);
        }
        for t in &self.tiers {
            (*t as u8).hash(&mut h);
        }
        let c = &self.camera;
        (c.enabled, c.radius.to_bits(), c.direction.to_bits(), c.zoom.to_bits(), c.rotate.to_bits(), c.static_only, c.mirror)
            .hash(&mut h);
        (self.centre_weighting.to_bits(), self.stability_emphasis.to_bits()).hash(&mut h);
        (self.switches.metrics, self.switches.flags, self.switches.camera_filter).hash(&mut h);
        self.shot_types.hash(&mut h);
        h.finish()
    }

    /// Key for stage S5 (candidate runs), including the S1–S4 key.
    pub fn key_runs(&self) -> u64 {
        let mut h = KeyHasher::default();
        (self.key_frame(), self.score_lo.to_bits(), self.gap_s.to_bits()).hash(&mut h);
        h.finish()
    }

    /// Key for stage S6 (segments), including S5.
    pub fn key_segments(&self) -> u64 {
        let mut h = KeyHasher::default();
        (
            self.key_runs(),
            self.score_hi.to_bits(),
            self.len_min_s.to_bits(),
            self.len_max_s.map(f64::to_bits),
            self.handles_samples,
            self.long_mode as u8,
            self.snap_to_sentences,
        )
            .hash(&mut h);
        h.finish()
    }
}

/// FNV-1a hasher: stable across runs and platforms (unlike `DefaultHasher`).
#[derive(Default)]
pub struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        let mut h = if self.0 == 0 { 0xcbf2_9ce4_8422_2325 } else { self.0 };
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        self.0 = h;
    }
}

impl Preset {
    /// A preset that reproduces `s` exactly (custom presets, SPEC P9.4): every sub-metric, flag
    /// tier and advanced value is written out, so later default changes don't alter it.
    pub fn from_settings(s: &Settings, name: &str, description: &str) -> Preset {
        use crate::preset::*;
        let secs = |v: f64| format!("{v}s");
        let mut groups: [std::collections::BTreeMap<String, SubPref>; 5] = Default::default();
        for sm in SubMetric::ALL {
            let cur = &s.subs[sm as usize];
            let v = if sm.is_target() {
                SubPref::Target(TargetPref { enabled: cur.enabled, target: cur.target, tolerance: cur.tol, strength: cur.strength })
            } else if !cur.enabled {
                SubPref::Switch(false)
            } else {
                SubPref::Pref(cur.pref)
            };
            groups[sm.group() as usize].insert(sm.key().to_string(), v);
        }
        let [focus, exposure, motion, audio, content] = groups;
        Preset {
            preset: PresetMeta { name: name.to_string(), version: 1, description: description.to_string(), extra: Extra::new() },
            thresholds: Thresholds {
                score: [s.score_lo, s.score_hi],
                segment_length: [secs(s.len_min_s), s.len_max_s.map(secs).unwrap_or_else(|| "none".into())],
                max_per_clip: s.max_per_clip.map(|n| n.to_string()).unwrap_or_else(|| "none".into()),
                extra: Extra::new(),
            },
            weights: Weights { focus: s.weights[0], exposure: s.weights[1], motion: s.weights[2], audio: s.weights[3], content: s.weights[4], extra: Extra::new() },
            focus,
            exposure,
            motion,
            audio,
            content,
            flags: FlagKind::ALL.iter().map(|f| (f.key().to_string(), s.tier(*f))).collect(),
            camera_filter: CameraFilterToml {
                enabled: s.camera.enabled,
                radius: s.camera.radius,
                direction: s.camera.direction,
                zoom: s.camera.zoom,
                rotate: s.camera.rotate,
                static_only: s.camera.static_only,
                mirror: s.camera.mirror,
                extra: Extra::new(),
            },
            advanced: Advanced {
                gap_tolerance: secs(s.gap_s),
                centre_weighting: s.centre_weighting,
                stability_emphasis: s.stability_emphasis,
                handles_samples: s.handles_samples,
                snap_to_sentences: s.snap_to_sentences,
                long_segment_mode: s.long_mode,
                extra: Extra::new(),
            },
            switches: Switches { metrics: s.switches.metrics, flags: s.switches.flags, camera_filter: s.switches.camera_filter },
            extra: Extra::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn presets_round_trip_through_settings() {
        for p in crate::builtins() {
            let s = Settings::from_preset(&p).unwrap();
            let back = Preset::from_settings(&s, &p.preset.name, &p.preset.description);
            let text = back.to_toml().unwrap();
            let s2 = Settings::from_preset(&Preset::parse(&text).unwrap()).unwrap();
            assert_eq!(s2, s, "{} doesn't round-trip:
{text}", p.preset.name);
        }
    }

    use super::*;
    use crate::preset::builtin;

    #[test]
    fn talking_head_resolves() {
        let s = Settings::from_preset(&builtin("Talking Head").unwrap()).unwrap();
        assert_eq!(s.score_lo, 0.62);
        assert_eq!(s.len_min_s, 3.0);
        assert_eq!(s.len_max_s, None);
        assert_eq!(s.sub(SubMetric::CameraMotion).pref, -0.6);
        assert_eq!(s.sub(SubMetric::Sharpness).pref, 0.7); // default kept
        assert_eq!(s.tier(FlagKind::Duplicate), Tier::ForceExclude);
        assert_eq!(s.gap_s, 1.0);
    }

    #[test]
    fn audio_first_loudness_target() {
        let s = Settings::from_preset(&builtin("Audio-First").unwrap()).unwrap();
        let l = s.sub(SubMetric::Loudness);
        assert!(l.enabled && l.target == -16.0 && l.tol == 8.0);
        assert_eq!(s.tier(FlagKind::Silence), Tier::ForceExclude);
    }

    #[test]
    fn stage_keys_are_layered() {
        let a = Settings::default();
        let mut b = a.clone();
        b.score_hi = 0.9;
        assert_eq!(a.key_frame(), b.key_frame());
        assert_eq!(a.key_runs(), b.key_runs());
        assert_ne!(a.key_segments(), b.key_segments());
        let mut c = a.clone();
        c.score_lo = 0.3;
        assert_eq!(a.key_frame(), c.key_frame());
        assert_ne!(a.key_runs(), c.key_runs());
        let mut d = a.clone();
        d.weights[0] = 1.0;
        assert_ne!(a.key_frame(), d.key_frame());
    }

    #[test]
    fn starved_group() {
        let mut s = Settings::default();
        for sm in SubMetric::of_group(MetricGroup::Focus) {
            s.sub_mut(sm).enabled = false;
        }
        assert!(s.group_starved(MetricGroup::Focus));
        assert!(!s.group_starved(MetricGroup::Exposure));
    }
}
