// Mock backend for the browser preview and component tests. Deterministic demo data with a
// simplified selects rule (threshold + length + cap); the real engine lives in Rust.

import type { AnalysisEvent, Backend, Clip, Scored, Segment, Settings } from "./api";
import type { PresetInfo } from "./bindings";

const PRESETS: PresetInfo[] = [
  ["All-Rounder", "Balanced default for any footage."],
  ["Talking Head", "Interviews and dialogue: steady, sharp, well exposed."],
  ["Social Media", "Short, punchy, well-exposed moments for short-form edits."],
  ["Cleanup", "Keeps almost everything, removing only junk."],
  ["Action / Sport", "Chases movement and energy."],
  ["Long Takes", "Holds out for long, stable shots."],
  ["Drone / Aerial", "Favours smooth, sharp gimbal and drone footage."],
  ["Whip Pans", "Finds fast horizontal whip pans in either direction."],
  ["Locked-Off", "Only tripod-steady, static shots."],
  ["Twist", "Rolling, rotating camera moves."],
  ["Tilt", "Vertical camera moves, up or down."],
  ["Pan", "Horizontal camera moves, left or right."],
  ["Interview", "Talking Head plus faces in focus and complete sentences."],
  ["Wedding / Event", "Tolerant of dim venues, wary of LED flicker."],
  ["B-roll", "Clean, steady cutaways of a usable length."],
  ["Documentary Verité", "Handheld observational footage."],
  ["Product / Tabletop", "Pin-sharp, well-exposed product shots."],
  ["Gimbal Walk", "Smooth moving gimbal shots."],
  ["Audio-First", "Podcasts and vlogs: clean, present speech."],
].map(([name, description]) => ({ name: name!, description: description! }));

const NAMES = [
  "A001C003_241012_R1AB", "A001C004_241012_R1AB", "A001C007_241012_R1AB", "B002C001_241012_R2CD",
  "B002C002_241012_R2CD", "INTERVIEW_SARAH_WIDE", "INTERVIEW_SARAH_CU", "DRONE_COAST_01",
  "DRONE_COAST_02", "GIMBAL_MARKET_03", "BROLL_HANDS_05", "BROLL_STREET_11", "TIMELAPSE_DUSK", "SLATE_TEST",
];

function rng(seed: number) {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 2 ** 32;
  };
}

interface DemoClip {
  clip: Clip;
  durationUs: number;
  bar: Float32Array;
  flags: Uint32Array;
}

function makeDemo(): DemoClip[] {
  return NAMES.map((name, i) => {
    const r = rng(i * 7919 + 3);
    const dur = (20 + Math.floor(r() * 140)) * 1e6;
    const n = Math.floor((dur / 1e6) * 4);
    const bar = new Float32Array(n);
    const flags = new Uint32Array(n);
    let v = 0.4 + r() * 0.4;
    for (let k = 0; k < n; k++) {
      v += (r() - 0.5) * 0.08 + (0.62 - v) * 0.02;
      if (r() < 0.01) v = 0.15 + r() * 0.2; // a bad moment
      bar[k] = Math.min(1, Math.max(0, v));
      if (bar[k]! < 0.3 && r() < 0.6) flags[k] = 1 << 5; // shaky
      if (name === "SLATE_TEST" && k < n / 3) {
        bar[k] = 0.05;
        flags[k] = 1 << 2; // solid colour
      }
    }
    return {
      clip: { clip: i + 1, media: i + 1, name, path: `D:/Footage/${name}.mov`, state: "done", progress: 1, error: null },
      durationUs: dur,
      bar,
      flags,
    };
  });
}

const demo = makeDemo();
const listeners = new Set<(e: AnalysisEvent) => void>();

function defaultSettings(name: string): Settings {
  const lo: Record<string, number> = { Cleanup: 0.25, "Talking Head": 0.62, "Social Media": 0.6, "Whip Pans": 0.45 };
  return {
    preset_name: name,
    score_lo: lo[name] ?? 0.55,
    score_hi: 1,
    len_min_s: name === "Long Takes" ? 10 : 2,
    len_max_s: name === "Social Media" ? 15 : null,
    max_per_clip: name === "Social Media" ? 3 : null,
    weights: [35, 30, 25, 10, 0],
    subs: Array.from({ length: 23 }, () => ({ enabled: true, pref: 0.5, target: 0.5, tol: 0.2, strength: 1 })),
    tiers: Array.from({ length: 18 }, (_, i) => (i === 1 || i === 2 ? "force_exclude" : i === 0 || (i >= 4 && i <= 6) ? "disfavour" : "neutral")),
    camera: { enabled: false, radius: 0, direction: 0, zoom: 0, rotate: 0, static_only: false, mirror: false },
    gap_s: 0.75,
    centre_weighting: 0.2,
    stability_emphasis: 0.4,
    handles_samples: 3,
    long_mode: "split",
    snap_to_sentences: false,
    switches: { metrics: true, flags: true, camera_filter: true },
    shot_types: [],
  };
}

function score(s: Settings): Scored {
  const lo = s.score_lo ?? 0.55;
  const minUs = (s.len_min_s ?? 0) * 1e6;
  const maxUs = s.len_max_s === null || s.len_max_s === undefined ? Infinity : s.len_max_s * 1e6;
  const clips = demo.map((d, ci) => {
    const segs: Segment[] = [];
    let start = -1;
    const flush = (end: number) => {
      if (start < 0) return;
      const sUs = start * 250_000;
      let eUs = Math.min(d.durationUs, (end + 1) * 250_000);
      if (eUs - sUs > maxUs) eUs = sUs + maxUs;
      if (eUs - sUs >= minUs) {
        let sum = 0;
        for (let k = start; k <= end; k++) sum += d.bar[k]!;
        segs.push({ startUs: sUs, endUs: eUs, inUs: Math.max(0, sUs - 750_000), outUs: Math.min(d.durationUs, eUs + 750_000), mean: sum / (end - start + 1), firstSample: start, lastSample: end, kind: "auto" });
      }
      start = -1;
    };
    for (let k = 0; k < d.bar.length; k++) {
      const excluded = (d.flags[k]! & (1 << 2)) !== 0;
      if (d.bar[k]! >= lo && !excluded) {
        if (start < 0) start = k;
      } else flush(k - 1);
    }
    flush(d.bar.length - 1);
    const cap = s.max_per_clip;
    const kept = cap === null || cap === undefined ? segs : [...segs].sort((a, b) => b.mean - a.mean).slice(0, cap).sort((a, b) => a.startUs - b.startUs);
    return { ci, clip: d.clip.clip, durationUs: d.durationUs, rateHz: 4, bar: d.bar, flags: d.flags, segments: kept };
  });
  const selects = clips.flatMap((c) => c.segments.map((segment) => ({ clip: c.clip, clipIndex: c.ci, segment })));
  const selectsUs = selects.reduce((a, s) => a + s.segment.endUs - s.segment.startUs, 0);
  const footageUs = demo.reduce((a, d) => a + d.durationUs, 0);
  return {
    clips: clips.map(({ ci: _ci, ...c }) => c),
    selects,
    summary: { count: selects.length, selectsUs, footageUs, percent: (100 * selectsUs) / footageUs, flagRuns: [["shaky", 12], ["solid_colour", 1]] },
  };
}

export const mockBackend: Backend = {
  projectNew: async (path, name) => ({ path, name, recovered: false }),
  projectOpen: async (path) => ({ path, name: "Demo Project", recovered: false }),
  projectClose: async () => {},
  mediaImport: async (paths) => ({ added: paths.length, duplicates: 0, proxies_linked: 0, cache_hits: 0, failed: [] }),
  analysisStart: async () => 0,
  analysisPause: async () => {},
  analysisResume: async () => {},
  clipsList: async () => demo.map((d) => d.clip),
  presetsList: async () => PRESETS,
  presetSettings: async (name) => defaultSettings(name),
  score: async (settings) => score(settings),
  onAnalysis: async (cb) => {
    listeners.add(cb);
    return () => listeners.delete(cb);
  },
};
