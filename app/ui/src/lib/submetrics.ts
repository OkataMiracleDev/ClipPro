// Sub-metric presentation (SPEC TUN-4/5, §14.2): end labels for the bipolar sliders ("what a high
// normalised value means"), target ranges for target-type sub-metrics, and the weighting maths.

export interface SubMetricInfo {
  key: string;
  label: string;
  group: string;
  target: boolean;
}

/** Weighting groups in `Settings.weights` order. Content needs the AI modules. */
export const GROUPS: { key: string; label: string; ai?: boolean }[] = [
  { key: "focus", label: "Focus" },
  { key: "exposure", label: "Exposure" },
  { key: "motion", label: "Motion" },
  { key: "audio", label: "Audio" },
  { key: "content", label: "Content", ai: true },
];

/** [low end, high end] of the normalised value. */
export const ENDS: Record<string, [string, string]> = {
  sharpness: ["Soft", "Sharp"],
  detail: ["Plain", "Detailed"],
  focus_stability: ["Hunting", "Locked"],
  grading_flexibility: ["Clipped", "Flexible"],
  flicker: ["Steady", "Flickering"],
  noise: ["Clean", "Noisy"],
  wb_cast: ["Neutral", "Tinted"],
  camera_motion: ["Still", "Moving"],
  subject_motion: ["Still", "Busy"],
  motion_stability: ["Shaky", "Smooth"],
  clean: ["Clipped", "Clean"],
  noise_floor: ["Quiet", "Noisy"],
  speech_presence: ["No speech", "Speech"],
  wind: ["Calm", "Windy"],
  hum: ["Clean", "Hum"],
  face_focus: ["Soft face", "Sharp face"],
  eyes_open: ["Closed", "Open"],
  face_size: ["Small", "Large"],
  aesthetic: ["Plain", "Striking"],
  shot_type_match: ["Off-type", "Match"],
};

/** Target-type ranges: [min, max, unit formatter]. */
export const TARGETS: Record<string, { min: number; max: number; fmt: (v: number) => string; tolMax: number }> = {
  luma: { min: 0, max: 1, fmt: (v) => `${Math.round(v * 100)}%`, tolMax: 0.5 },
  contrast: { min: 0, max: 1, fmt: (v) => `${Math.round(v * 100)}%`, tolMax: 0.5 },
  loudness: { min: -40, max: -6, fmt: (v) => `${v.toFixed(0)} LUFS`, tolMax: 16 },
};

/** Neutral detent: preferences this close to 0 snap to 0 (excluded). */
export const DETENT = 0.06;

export function snapPref(v: number): number {
  const c = Math.max(-1, Math.min(1, v));
  return Math.abs(c) < DETENT ? 0 : Math.round(c * 100) / 100;
}

/**
 * Sets one group's weight (0–100). Without `lock`, the other groups are rescaled so the total
 * stays what it was (dragging one vertex trades weight with the rest); with `lock` (Ctrl+drag)
 * the others keep their values.
 */
export function setWeight(weights: number[], i: number, value: number, lock: boolean): number[] {
  const v = Math.max(0, Math.min(100, Math.round(value)));
  if (lock) return weights.map((w, j) => (j === i ? v : w));
  const total = weights.reduce((a, b) => a + b, 0);
  const othersOld = total - weights[i]!;
  const othersNew = Math.max(0, total - v);
  return weights.map((w, j) => (j === i ? v : othersOld > 0 ? Math.round((w * othersNew) / othersOld * 10) / 10 : w));
}

/** A group is "amber" (has weight but no effect) when every one of its sub-metrics is off. */
export function amberGroups(weights: number[], subs: { enabled: boolean; pref: number; strength: number }[], infos: SubMetricInfo[]): Set<string> {
  const out = new Set<string>();
  GROUPS.forEach((g, gi) => {
    if ((weights[gi] ?? 0) <= 0) return;
    const members = infos.map((m, i) => ({ m, s: subs[i] })).filter((x) => x.m.group === g.key);
    const active = members.some(({ m, s }) => s && s.enabled && (m.target ? s.strength > 0 : s.pref !== 0));
    if (!active) out.add(g.key);
  });
  return out;
}
