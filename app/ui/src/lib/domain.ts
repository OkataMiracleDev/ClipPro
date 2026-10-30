// UI-side constants mirroring Rust enums. Order matters: Settings index tiers by FlagKind and
// weights by MetricGroup (see clippro-types/src/domain.rs).

import type { Tier } from "../bindings";

export const FLAGS: { key: string; label: string; hint: string }[] = [
  { key: "clipping", label: "Clipping", hint: "1.5% or more of the picture at pure black or white" },
  { key: "duplicate", label: "Duplicate", hint: "Frozen or repeated frame" },
  { key: "solid_colour", label: "Solid colour", hint: "Leaders, bars, fades, lens cap" },
  { key: "hard_cut", label: "Hard cut", hint: "Sudden scene change" },
  { key: "motion_blur", label: "Motion blur", hint: "Soft picture during movement" },
  { key: "shaky", label: "Shaky", hint: "Unstable handheld motion" },
  { key: "low_light", label: "Low light", hint: "Genuinely underexposed" },
  { key: "focus_hunting", label: "Focus hunting", hint: "Focus pumping back and forth" },
  { key: "colour_bars", label: "Colour bars", hint: "Test pattern" },
  { key: "horizon_tilt", label: "Horizon tilt", hint: "Tilted horizon" },
  { key: "audio_clipping", label: "Audio clipping", hint: "Distorted audio" },
  { key: "silence", label: "Silence", hint: "No audio" },
  { key: "wind", label: "Wind", hint: "Wind noise" },
  { key: "hum", label: "Hum", hint: "Mains hum" },
  { key: "dead_channel", label: "Dead channel", hint: "An audio channel is silent" },
  { key: "channel_imbalance", label: "Channel imbalance", hint: "Left/right levels differ" },
  { key: "no_face", label: "No face", hint: "No face detected (AI)" },
  { key: "flub", label: "Flub", hint: "Filler words or restarts (AI)" },
];

/** Flags whose detectors run today (audio and AI flags arrive in later phases). */
/** Flags the AI modules raise (P12): shown when local AI is on. */
export const AI_FLAGS = new Set(["no_face", "flub"]);
export const ACTIVE_FLAGS = new Set(["clipping", "duplicate", "solid_colour", "hard_cut", "motion_blur", "shaky", "low_light", "focus_hunting", "colour_bars", "horizon_tilt"]);

export const PROFILES: { value: import("../bindings").ColourProfile; label: string }[] = [
  { value: "rec709", label: "Rec.709" },
  { value: "s_log3", label: "Sony S-Log3" },
  { value: "log_c3", label: "ARRI LogC3" },
  { value: "log_c4", label: "ARRI LogC4" },
  { value: "v_log", label: "Panasonic V-Log" },
  { value: "c_log2", label: "Canon Log 2" },
  { value: "c_log3", label: "Canon Log 3" },
  { value: "n_log", label: "Nikon N-Log" },
  { value: "f_log2", label: "Fujifilm F-Log2" },
  { value: "d_log", label: "DJI D-Log" },
  { value: "apple_log", label: "Apple Log" },
  { value: "log3_g10", label: "RED Log3G10" },
  { value: "bmd_film_gen5", label: "Blackmagic Film Gen 5" },
];

export const TIERS: { value: Tier; label: string; title: string }[] = [
  { value: "force_exclude", label: "Exclude", title: "Always drop samples with this flag" },
  { value: "disfavour", label: "−", title: "Disfavour (score ×0.6)" },
  { value: "neutral", label: "Off", title: "Neutral (informational only)" },
  { value: "favour", label: "+", title: "Favour (score ×1.2)" },
  { value: "force_include", label: "Include", title: "Always keep samples with this flag" },
];

export const GROUPS = ["Focus", "Exposure", "Motion", "Audio", "Content"] as const;
