// Mock backend for the browser preview and component tests. Deterministic demo data with a
// simplified selects rule (threshold + length + cap); the real engine lives in Rust.

import type { AnalysisEvent, AppSettings, Backend, Clip, Inspection, MediaSource, Scored, Segment, Settings } from "./api";
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
].map(([name, description]) => ({ name: name!, description: description!, builtin: true }));
const userPresets = new Map<string, { description: string; settings: Settings }>();

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
      clip: { clip: i + 1, media: i + 1, name, path: `D:/Footage/${name}.mov`, state: "done", progress: 1, error: null, profile: name.startsWith("DRONE") ? "d_log" : name.startsWith("A001") ? "s_log3" : "rec709", bin: null, camera: name.startsWith("DRONE") ? "DJI Mavic 3" : name.startsWith("A") ? "Sony FX6" : name.startsWith("B") ? "Canon C70" : null, created: `2026-10-${String(10 + (i % 5)).padStart(2, "0")}T09:00:00Z`, codec: name.startsWith("DRONE") ? "hevc" : "prores" },
      durationUs: dur,
      bar,
      flags,
    };
  });
}

const demo = makeDemo();
let mockSettings: AppSettings = { theme: "system", window_controls: "mac", reopen_last: true, last_project: null, recent_projects: ["D:/Projects/Coast Doc.clippro", "D:/Projects/Sarah Interview.clippro", "E:/Weddings/Okafor.clippro"], workers: 0, gpu_decode: true, use_cache: true, sample_rate: "standard", pair_sampling: true, keybindings: {}, colour_blind_scores: false };
const settingsListeners = new Set<(s: AppSettings) => void>();
const mockBins: { id: number; name: string; kind: string; query: string | null }[] = [
  { id: 1, name: "Interviews", kind: "manual", query: null },
  { id: 2, name: "Strong drone", kind: "smart", query: JSON.stringify({ match: "all", rules: [{ field: "camera", op: "contains", value: "DJI" }, { field: "score", op: ">=", value: 60 }] }) },
];
let mockBinId = 2;
const mockTemplates: { name: string; json: string }[] = [];
const mockOverrides: import("./api").OverrideInfo[] = [];
let mockOverrideId = 0;
const mockAnnotations: import("./api").Annotation[] = [];
const mockSnapshots: { id: number; name: string; json: string; created: string }[] = [];
let mockSnapId = 0;
let mockTuning: string | null = null;
let mockWatch: { path: string; subfolders: boolean; extensions: string[] }[] = [{ path: "D:/Footage/Incoming", subfolders: true, extensions: ["mov", "mxf"] }];
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
    // Mirrors SubMetric::default (clippro-score/src/submetric.rs).
    subs: [0.7, 0.3, 0.5, 0.5, -0.5, "luma", "contrast", -0.2, null, 0, 0, 0.6, "loudness", 0.6, -0.3, 0, -0.5, -0.3, null, null, null, null, null].map((d) =>
      d === "luma"
        ? { enabled: false, pref: 0, target: 0.45, tol: 0.15, strength: 1 }
        : d === "contrast"
          ? { enabled: false, pref: 0, target: 0.55, tol: 0.2, strength: 1 }
          : d === "loudness"
            ? { enabled: true, pref: 0, target: -23, tol: 8, strength: 1 }
            : { enabled: d !== null, pref: (d as number | null) ?? 0, target: 0.5, tol: 0.2, strength: 1 },
    ),
    tiers: Array.from({ length: 18 }, (_, i) => (i === 1 || i === 2 ? "force_exclude" : i === 0 || (i >= 4 && i <= 6) ? "disfavour" : "neutral")),
    camera: name === "Pan" ? { enabled: true, radius: 0.7, direction: 0, zoom: 0, rotate: 0, static_only: false, mirror: true } : { enabled: false, radius: 0, direction: 0, zoom: 0, rotate: 0, static_only: false, mirror: false },
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

function scoreAll(s: Settings): Scored {
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

/** Synthetic picture for the browser preview: a soft gradient that drifts with time. */
function mockFrame(media: number, us: number, width: number): string {
  const h = (media * 47) % 360;
  const t = us / 1e6;
  const x = 50 + 30 * Math.sin(t * 0.7 + media);
  const y = 45 + 20 * Math.cos(t * 0.5);
  const w = Math.round(width);
  const ht = Math.round((w * 9) / 16);
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${ht}" viewBox="0 0 160 90">` +
    `<defs><radialGradient id="g" cx="${x}%" cy="${y}%" r="75%"><stop offset="0" stop-color="hsl(${h} 45% 62%)"/>` +
    `<stop offset="0.55" stop-color="hsl(${(h + 30) % 360} 35% 32%)"/><stop offset="1" stop-color="hsl(${(h + 60) % 360} 30% 12%)"/></radialGradient></defs>` +
    `<rect width="160" height="90" fill="url(#g)"/>` +
    `<rect x="${(t * 9) % 180 - 20}" y="58" width="22" height="14" rx="2" fill="hsl(${h} 20% 85% / 0.55)"/></svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

const MOCK_GROUPS = [
  ["focus", "Focus", ["sharpness", "detail", "focus_stability"]],
  ["exposure", "Exposure", ["brightness", "contrast", "clipping"]],
  ["motion", "Motion", ["stability", "motion_blur", "subject_motion"]],
  ["composition", "Composition", ["horizon", "framing"]],
] as const;

function mockInspect(settings: Settings, clip: number, sample: number): Inspection {
  const d = demo.find((x) => x.clip.clip === clip);
  if (!d) throw new Error("Unknown clip");
  const k = Math.max(0, Math.min(d.bar.length - 1, sample));
  const r = rng(clip * 131 + k);
  const score = d.bar[k]!;
  const flags = (d.flags[k] ?? 0) & (1 << 5) ? ["shaky"] : (d.flags[k] ?? 0) & (1 << 2) ? ["solid_colour"] : [];
  const sc = scoreAll(settings).clips.find((c) => c.clip === clip);
  const si = sc ? sc.segments.findIndex((s) => k >= s.firstSample && k <= s.lastSample) : -1;
  const lo = settings.score_lo ?? 0.55;
  const forceOut = flags.includes("solid_colour");
  const reasons =
    si >= 0
      ? [`In select ${si + 1}`]
      : forceOut
        ? ["Force-excluded by Solid colour"]
        : score < lo
          ? [`Score ${Math.round(score * 100)} is below the threshold (${Math.round(lo * 100)})`]
          : ["Passes the score, but its run was dropped by the length, score-range or per-clip rules"];
  const groups = MOCK_GROUPS.map(([key, label], gi) => ({ key, label, score: Math.min(1, Math.max(0, score + (r() - 0.5) * 0.3)), weight: [35, 30, 25, 10][gi]! / 100 }));
  const subs = MOCK_GROUPS.flatMap(([g, , keys]) =>
    keys.map((key) => {
      const value = r();
      return { key, label: key.replace(/_/g, " ").replace(/^./, (c) => c.toUpperCase()), group: g, value, score: Math.min(1, Math.max(0, score + (value - 0.5) * 0.4)), weight: 1 / keys.length };
    }),
  );
  const tiles = Float32Array.from({ length: 8 * 5 }, (_, i) => {
    const x = (i % 8) - 3.5;
    const y = Math.floor(i / 8) - 2;
    return 1.2 + score * 1.4 - 0.12 * (x * x + y * y) + (r() - 0.5) * 0.3;
  });
  const pan = Math.sin(k / 9 + clip) * 6;
  const mvs = Int16Array.from({ length: 16 * 9 * 2 }, (_, i) => Math.round(i % 2 === 0 ? pan + (r() - 0.5) * 2 : (r() - 0.5) * 2));
  const hist = Array.from({ length: 64 }, (_, i) => Math.round(8 * Math.exp(-(((i - 20 - score * 20) / 9) ** 2))));
  return {
    clip,
    sample: k,
    timeUs: k * 250_000,
    pass: si >= 0,
    select: si >= 0 ? si + 1 : null,
    reasons,
    score,
    overall: Math.min(1, score + 0.03),
    tierMultiplier: flags.includes("shaky") ? 0.8 : 1,
    cameraFactor: 1,
    forceIn: false,
    forceOut,
    flags,
    groups,
    subs,
    raw: [
      ["sharp_full", 1.4 + score],
      ["mean", 0.42],
      ["noise_sigma", 0.012],
      ["cam_u_x", pan],
      ["cam_u_y", 0.4],
      ["cam_zr", 0.004],
      ["cam_wr", 0],
      ["cam_conf", 0.9],
    ].map(([key, value]) => ({ key: key as string, value: value as number })),
    tiles: { cols: 8, rows: 5, values: tiles },
    mvs: { cols: 16, rows: 9, values: mvs },
    hist,
    working: { w: 960, h: 540 },
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
  presetsList: async () => [...PRESETS, ...[...userPresets].map(([name, p]) => ({ name, description: p.description, builtin: false }))],
  presetSettings: async (name) => structuredClone(userPresets.get(name)?.settings) ?? defaultSettings(name),
  presetSave: async (name, description, settings, overwrite) => {
    if (PRESETS.some((p) => p.name === name)) throw new Error(`“${name}” is a built-in preset; choose another name.`);
    if (userPresets.has(name) && !overwrite) throw new Error(`A preset called “${name}” already exists.`);
    userPresets.set(name, { description, settings: { ...structuredClone(settings), preset_name: name } });
  },
  presetDelete: async (name) => void userPresets.delete(name),
  presetRename: async (old, name) => {
    const p = userPresets.get(old);
    if (!p) throw new Error(`No custom preset called “${old}”.`);
    userPresets.delete(old);
    userPresets.set(name, p);
  },
  presetImport: async () => {
    userPresets.set("Imported preset", { description: "From a file", settings: defaultSettings("All-Rounder") });
    return "Imported preset";
  },
  presetExport: async () => {},
  score: async (settings) => scoreAll(settings),
  exportSelects: async (settings, _sort, request) => ({ path: request.path, selects: scoreAll(settings).selects.length }),
  mediaSource: async (media): Promise<MediaSource> => {
    const d = demo.find((x) => x.clip.media === media);
    if (!d) throw new Error("Unknown media");
    return { nativeUrl: null, durationUs: d.durationUs, fps: 25, width: 1920, height: 1080, codec: "prores", hasAudio: false };
  },
  frameUrl: mockFrame,
  inspect: async (settings, clip, sample) => mockInspect(settings, clip, sample),
  audioPlay: async () => false,
  audioStop: async () => {},
  audioClock: async () => null,
  setProfile: async (media, profile) => {
    const d = demo.find((x) => x.clip.media === media);
    if (d) d.clip = { ...d.clip, profile };
  },
  // The browser preview shows display-referred images, so the transform is never applied.
  previewColour: async () => ({ lut: Float32Array.from({ length: 1024 }, (_, i) => (i / 1023) ** 2.4), matrix: new Float32Array([1, 0, 0, 0, 1, 0, 0, 0, 1]) }),
  retryClip: async () => true,
  bumpClip: async () => {},
  linkProxy: async () => {},
  reveal: async () => {},
  systemInfo: async () => ({ hwDecode: true, version: "0.1.0" }),
  getSettings: async () => mockSettings,
  watchFoldersGet: async () => mockWatch,
  snapshotsList: async () => [...mockSnapshots],
  snapshotSave: async (name, json) => {
    const i = mockSnapshots.findIndex((s) => s.name === name);
    if (i >= 0) mockSnapshots.splice(i, 1);
    mockSnapshots.push({ id: ++mockSnapId, name, json, created: new Date().toISOString() });
  },
  snapshotDelete: async (id) => {
    const i = mockSnapshots.findIndex((s) => s.id === id);
    if (i >= 0) mockSnapshots.splice(i, 1);
  },
  tuningSave: async (json) => {
    mockTuning = json;
  },
  tuningLoad: async () => mockTuning,
  binsList: async () => [...mockBins],
  binAdd: async (name, kind, query) => {
    mockBins.push({ id: ++mockBinId, name, kind, query });
    return mockBinId;
  },
  binUpdate: async (id, name, query) => {
    const b = mockBins.find((x) => x.id === id);
    if (b) Object.assign(b, { name, query });
  },
  binDelete: async (id) => {
    const i = mockBins.findIndex((x) => x.id === id);
    if (i >= 0) mockBins.splice(i, 1);
    demo.forEach((d) => d.clip.bin === id && (d.clip = { ...d.clip, bin: null }));
  },
  clipsSetBin: async (clips, bin) => {
    demo.forEach((d) => clips.includes(d.clip.clip) && (d.clip = { ...d.clip, bin }));
  },
  templatesList: async () => [...mockTemplates],
  templateSave: async (t) => {
    const i = mockTemplates.findIndex((x) => x.name === t.name);
    if (i >= 0) mockTemplates.splice(i, 1);
    mockTemplates.push(t);
  },
  templateDelete: async (name) => {
    const i = mockTemplates.findIndex((x) => x.name === name);
    if (i >= 0) mockTemplates.splice(i, 1);
  },
  overridesList: async () => [...mockOverrides],
  overrideAdd: async (clip, kind, startUs, endUs) => {
    mockOverrides.push({ id: ++mockOverrideId, clip, kind, startUs, endUs });
    return mockOverrideId;
  },
  overrideRemove: async (id) => {
    const i = mockOverrides.findIndex((o) => o.id === id);
    if (i >= 0) mockOverrides.splice(i, 1);
  },
  annotationsList: async () => [...mockAnnotations],
  annotationSet: async (a) => {
    const i = mockAnnotations.findIndex((x) => x.clip === a.clip && Math.min(x.endUs, a.endUs) - Math.max(x.startUs, a.startUs) > 0);
    if (i >= 0) mockAnnotations.splice(i, 1);
    if (a.rating || a.label || a.note.trim()) mockAnnotations.push(a);
  },
  watchFoldersSet: async (f) => {
    mockWatch = f;
  },
  onMediaAdded: async () => () => {},
  cameraMatches: async (s, count) => {
    const want = s.camera.static_only ? ["SLATE_TEST", "INTERVIEW_SARAH_WIDE", "INTERVIEW_SARAH_CU"] : ["DRONE_COAST_01", "DRONE_COAST_02", "GIMBAL_MARKET_03"];
    return demo
      .map((d, i) => ({ clip: d.clip.clip, name: d.clip.name, score: want.includes(d.clip.name) ? 0.9 - want.indexOf(d.clip.name) * 0.07 : 0.1 + ((i * 37) % 30) / 100 }))
      .sort((a, b) => b.score - a.score)
      .slice(0, count);
  },
  submetrics: async () =>
    ([
      ["sharpness", "Sharpness", "focus"], ["detail", "Detail", "focus"], ["focus_stability", "Focus stability", "focus"],
      ["grading_flexibility", "Grading flexibility", "exposure"], ["flicker", "Flicker", "exposure"], ["luma", "Luma", "exposure", true],
      ["contrast", "Contrast", "exposure", true], ["noise", "Noise", "exposure"], ["wb_cast", "White-balance cast", "exposure"],
      ["camera_motion", "Camera motion", "motion"], ["subject_motion", "Subject motion", "motion"], ["motion_stability", "Motion stability", "motion"],
      ["loudness", "Loudness", "audio", true], ["clean", "Clean (no clipping)", "audio"], ["noise_floor", "Noise floor", "audio"],
      ["speech_presence", "Speech presence", "audio"], ["wind", "Wind", "audio"], ["hum", "Hum", "audio"],
      ["face_focus", "Face in focus", "content"], ["eyes_open", "Eyes open", "content"], ["face_size", "Face size", "content"],
      ["aesthetic", "Aesthetic", "content"], ["shot_type_match", "Shot-type match", "content"],
    ] as [string, string, string, boolean?][]).map(([key, label, group, target]) => ({ key, label, group, target: !!target })),
  setSettings: async (s) => {
    mockSettings = s;
    settingsListeners.forEach((l) => l(s));
    return s;
  },
  // The browser preview has one window: the settings page opens with `?window=settings`.
  openSettings: async () => {
    location.search = "?window=settings";
  },
  onSettings: async (cb) => {
    settingsListeners.add(cb);
    return () => settingsListeners.delete(cb);
  },
  readLut: async () => "TITLE \"Demo\"\nLUT_1D_SIZE 2\n0 0 0\n1 1 1\n",
  onAnalysis: async (cb) => {
    listeners.add(cb);
    return () => listeners.delete(cb);
  },
};
