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
      clip: { clip: i + 1, media: i + 1, name, path: `D:/Footage/${name}.mov`, state: "done", progress: 1, error: null, profile: name.startsWith("DRONE") ? "d_log" : name.startsWith("A001") ? "s_log3" : "rec709", bin: null, camera: name.startsWith("DRONE") ? "DJI Mavic 3" : name.startsWith("A") ? "Sony FX6" : name.startsWith("B") ? "Canon C70" : null, created: `2026-10-${String(10 + (i % 5)).padStart(2, "0")}T09:00:00Z`, codec: name.startsWith("DRONE") ? "hevc" : "prores", missing: false },
      durationUs: dur,
      bar,
      flags,
    };
  });
}

const demo = makeDemo();
let mockSettings: AppSettings = { theme: "system", window_controls: "mac", reopen_last: true, last_project: null, recent_projects: ["D:/Projects/Coast Doc.clippro", "D:/Projects/Sarah Interview.clippro", "E:/Weddings/Okafor.clippro"], workers: 0, gpu_decode: true, use_cache: true, sample_rate: "standard", pair_sampling: true, keybindings: {}, colour_blind_scores: false, tour_done: !new URLSearchParams(location.search).has("tour"), ai_consent: true, ai_faces: true, ai_shots: true, ai_transcripts: true, ai_speakers: true, whisper_model: "whisper-small", ai_language: "", ai_deterministic: false };
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
const mockSyncGroups: import("./api").SyncGroup[] = [];
const jobListeners = new Set<(p: import("./api").JobProgress) => void>();
let mockJobCancel = false;
/** A demo proxy/render job: about a second per item. */
function mockJob(job: "proxy" | "render", count: number, dir = "D:/Footage/Proxy") {
  mockJobCancel = false;
  let t = 0;
  const emit = (p: Partial<import("./api").JobProgress>) => jobListeners.forEach((l) => l({ job, kind: "progress", index: 0, count, fraction: 0, name: null, message: null, outputs: [], ...p }));
  const tick = () => {
    if (mockJobCancel) return emit({ kind: "cancelled" });
    t += 0.2;
    const index = Math.floor(t);
    if (index >= count) return emit({ kind: "finished", index: count, fraction: 1, outputs: Array.from({ length: count }, (_, i) => `${dir}/select_${String(i + 1).padStart(3, "0")}.mov`) });
    emit({ index, fraction: t - index, name: demo[index % demo.length]!.clip.name });
    setTimeout(tick, 200);
  };
  setTimeout(tick, 200);
}
// AI (P12): demo People, shot types and two interview transcripts.
const aiListeners = new Set<(p: import("./api").AiProgress) => void>();
const mockModels: import("./api").AiModel[] = [
  ["yunet-2023mar", "faces", "YuNet face detector", 232_589, "MIT"],
  ["sface-2021dec", "faces", "SFace face recogniser", 38_696_353, "Apache-2.0"],
  ["clip-b32-vision", "clip", "CLIP ViT-B/32 image encoder (8-bit)", 88_224_011, "MIT"],
  ["clip-b32-text", "clip", "CLIP ViT-B/32 text encoder (8-bit)", 64_177_617, "MIT"],
  ["clip-b32-tokenizer", "clip", "CLIP tokenizer", 2_224_041, "MIT"],
  ["whisper-tiny", "transcripts", "Whisper tiny", 77_691_713, "MIT"],
  ["whisper-base", "transcripts", "Whisper base", 147_951_465, "MIT"],
  ["whisper-small", "transcripts", "Whisper small", 487_601_967, "MIT"],
  ["whisper-medium", "transcripts", "Whisper medium", 1_533_763_059, "MIT"],
  ["whisper-large-v3-turbo", "transcripts", "Whisper large-v3 turbo", 1_624_555_275, "MIT"],
  ["ecapa-voxceleb-512", "speakers", "ECAPA-TDNN speaker embeddings", 24_948_830, "Apache-2.0"],
].map(([id, module, name, size, licence]) => ({ id: id as string, module: module as import("./api").AiModel["module"], name: name as string, size: size as number, licence: licence as string, source: "", installed: id !== "whisper-medium" && id !== "whisper-large-v3-turbo", downloading: false }));
const mockPeople: import("./api").Person[] = [
  { id: 1, name: "Sarah", clips: [6, 7], cover: { clip: 7, us: 4e6, box: [0.38, 0.18, 0.24, 0.42] } },
  { id: 2, name: "Person 2", clips: [10], cover: { clip: 10, us: 2e6, box: [0.55, 0.25, 0.12, 0.2] } },
];
const SHOTS: Record<string, import("./api").ShotType> = { INTERVIEW_SARAH_WIDE: "medium", INTERVIEW_SARAH_CU: "close_up", DRONE_COAST_01: "aerial", DRONE_COAST_02: "aerial", BROLL_HANDS_05: "insert", BROLL_STREET_11: "wide", GIMBAL_MARKET_03: "medium", TIMELAPSE_DUSK: "wide" };
const LINES = [
  "So we started the project about three years ago.",
  "Um, at the time nobody really believed it would work.",
  "The first prototype was, the first prototype was built in a garage.",
  "And honestly that's where most of the good ideas came from.",
  "Today we ship to forty countries.",
];
function mockTranscript(clip: number): import("./api").Transcript | null {
  if (clip !== 6 && clip !== 7) return null;
  const lines = LINES.map((text, i) => ({ startUs: i * 4.2e6 + 0.5e6, endUs: i * 4.2e6 + 4e6, text, speaker: i === 4 && clip === 6 ? 1 : 0 }));
  const words = lines.flatMap((l) => {
    const ws = l.text.split(" ");
    const step = (l.endUs - l.startUs) / ws.length;
    return ws.map((w, k) => ({ startUs: l.startUs + k * step, endUs: l.startUs + (k + 1) * step, text: w, prob: 0.9, filler: /^um,?$/i.test(w) }));
  });
  return { language: "en", words, lines, flubs: [[lines[2]!.startUs, lines[2]!.startUs + 1.6e6]] };
}

const offloadListeners = new Set<(p: import("./api").OffloadProgress) => void>();
let mockOffloadCancel = false;
let mockSyncId = 0;

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
    best_take_only: false,
    switches: { metrics: true, flags: true, camera_filter: true },
    shot_types: [],
  };
}

/** Demo sync: the longest clip is the reference; one clip without audio match (P11.5). */
function mockSync(name: string, clips: number[], method: import("./api").SyncMethod): import("./api").SyncGroup {
  const ds = clips.map((c) => demo.find((d) => d.clip.clip === c)!).filter(Boolean);
  if (ds.length < 2) throw new Error("Choose at least two clips to sync.");
  const ref = ds.reduce((a, b) => (b.durationUs > a.durationUs ? b : a));
  const r = rng(clips.reduce((a, b) => a * 31 + b, 7));
  const warnings: string[] = [];
  const members = ds.map((d) => {
    const isRef = d === ref;
    const lost = !isRef && d.clip.name.startsWith("TIMELAPSE");
    if (lost) warnings.push(`${d.clip.name} could not be synced (no confident audio match).`);
    const off = isRef ? 0 : Math.round((r() - 0.35) * 0.6 * ref.durationUs / 40_000) * 40_000;
    const tc = method === "timecode" || !isRef ? off + (r() < 0.2 ? 80_000 : 0) : 0;
    return {
      clip: d.clip.clip,
      name: d.clip.name,
      angle: d.clip.camera ?? d.clip.name,
      offsetUs: lost ? null : off,
      psr: isRef || method === "timecode" || lost ? null : 7 + r() * 14,
      tcOffsetUs: lost ? null : tc,
      disagreementUs: method === "audio" && !lost ? Math.abs(off - tc) : null,
    };
  });
  for (const m of members) if ((m.disagreementUs ?? 0) > 40_000) warnings.push(`${m.name}: audio and timecode disagree by ${Math.round(m.disagreementUs! / 1000)} ms.`);
  // Same camera twice: number the angles.
  for (const m of members) if (members.filter((x) => x.angle === m.angle).length > 1) m.angle = `${m.angle} · ${m.name}`;
  return { id: null, name, method, reference: ref.clip.clip, members, warnings };
}

/** A demo editor's cut over the demo clips, reviewed like the engine would (P11.3). */
function mockReview(path: string, weakBelow: number): import("./api").Review {
  const fps = 25;
  const cut: [string, number, number][] = [
    ["INTERVIEW_SARAH_WIDE", 4, 9], ["BROLL_HANDS_05", 2, 5.5], ["INTERVIEW_SARAH_CU", 12, 18], ["DRONE_COAST_01", 6, 10],
    ["SLATE_TEST", 1, 4], ["GIMBAL_MARKET_03", 8, 12.5], ["A001C004_241012_R1AB", 3, 7], ["DRONE_COAST_02", 14, 19], ["BROLL_STREET_11", 5, 8],
  ];
  const mean = (d: DemoClip, a: number, b: number) => {
    const s = d.bar.subarray(Math.floor(a * 4), Math.max(Math.floor(a * 4) + 1, Math.floor(b * 4)));
    return s.reduce((x, y) => x + y, 0) / Math.max(1, s.length);
  };
  const family = (n: string) => n.replace(/_(WIDE|CU|0\d|\d\d)$/, "").replace(/C\d{3}_.*/, "");
  let at = 0;
  const items = cut.map(([name, a, b]) => {
    const d = demo.find((x) => x.clip.name === name)!;
    const len = Math.round((b - a) * fps);
    const m = mean(d, a, b);
    const k0 = Math.floor(a * 4);
    const k1 = Math.max(k0 + 1, Math.floor(b * 4));
    const bits = d.flags.subarray(k0, k1).reduce((x, y) => x | y, 0);
    const excluded = (bits & (1 << 2)) !== 0;
    const flags = [bits & (1 << 5) ? "Shaky" : "", excluded ? "Solid colour" : ""].filter(Boolean);
    const alternatives: import("./api").ReviewAlt[] = [];
    for (const o of demo) {
      const same = o === d;
      if (!same && family(o.clip.name) !== family(name)) continue;
      // Best window of the same length elsewhere.
      let best = -1;
      let bestAt = 0;
      const w = b - a;
      for (let t = 0; t + w <= o.durationUs / 1e6; t += 0.5) {
        if (same && Math.abs(t - a) < w) continue;
        const v = mean(o, t, t + w);
        if (v > best) [best, bestAt] = [v, t];
      }
      if (best > m + 0.02) alternatives.push({ clip: o.clip.clip, clipName: o.clip.name, startUs: bestAt * 1e6, endUs: (bestAt + w) * 1e6, mean: best, kind: same ? "same_clip" : "take_group", similarity: same ? 1 : 0.86 });
    }
    alternatives.sort((x, y) => y.mean - x.mean);
    const bars = Array.from(d.bar.subarray(k0, k1));
    const item = { track: 0, name, recordIn: at, recordOut: at + len, clip: d.clip.clip, srcInUs: a * 1e6, srcOutUs: b * 1e6, mean: m, bars, flags, excluded, weak: m < weakBelow || excluded, alternatives: alternatives.slice(0, 3) };
    at += len;
    return item;
  });
  const offline = { track: 0, name: "MUSIC_BED_FINAL", recordIn: at, recordOut: at + 75, clip: null, srcInUs: 0, srcOutUs: 3e6, mean: null, bars: [], flags: [], excluded: false, weak: false, alternatives: [] };
  const title = path.split(/[\\/]/).pop()!.replace(/\.[^.]+$/, "");
  return { name: title, format: path.endsWith(".edl") ? "edl" : path.endsWith(".fcpxml") ? "fcpxml" : path.endsWith(".otio") ? "otio" : "xmeml", fps, startTcFrames: 90000, dropFrame: false, duration: at + 75, tracks: 1, items: [...items, offline], linked: items.length, missing: ["MUSIC_BED_FINAL"] };
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
    // The two interview angles' first selects are takes of the same answer.
    takes: [6, 7].flatMap((c, i) => {
      const seg = clips.find((x) => x.clip === c)?.segments[0];
      return seg ? [{ clip: c, startUs: seg.startUs, group: 0, rank: i + 1, size: 2 }] : [];
    }),
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
  cacheInfo: async () => ({ path: "C:/Users/you/AppData/Local/ClipPro/cache/measure-cache.sqlite", entries: 312, bytes: 211_000_000 }),
  cacheClear: async () => 312,
  audioStreams: async (clip) => ({
    streams:
      clip % 3 === 0
        ? [
            { index: 1, codec: "pcm_s24le", channels: 2, layout: "stereo", language: "" },
            { index: 2, codec: "pcm_s24le", channels: 2, layout: "stereo", language: "" },
          ]
        : [{ index: 1, codec: "aac", channels: 2, layout: "stereo", language: "eng" }],
    selected: null,
  }),
  audioStreamSet: async () => {},
  syncClips: async (name, clips, method) => mockSync(name, clips, method),
  syncGroups: async () => mockSyncGroups.map((g) => ({ ...g })),
  syncGroupSave: async (group) => {
    const id = group.id ?? ++mockSyncId;
    const i = mockSyncGroups.findIndex((g) => g.id === id);
    if (i >= 0) mockSyncGroups[i] = { ...group, id };
    else mockSyncGroups.push({ ...group, id });
    return id;
  },
  syncGroupDelete: async (id) => {
    const i = mockSyncGroups.findIndex((g) => g.id === id);
    if (i >= 0) mockSyncGroups.splice(i, 1);
  },
  multicamExport: async (group, _format, path) => ({ path, selects: group.members.filter((m) => m.offsetUs !== null).length }),
  timelineReview: async (_settings, path, weakBelow) => mockReview(path, weakBelow),
  timelineExportBack: async (_swaps, _markers, _format, path) => ({ path, selects: 9 }),
  audioLevels: async () => ({ levels: [-12 - Math.random() * 10, -13 - Math.random() * 10], sourceChannels: 2 }),
  audioChannelGains: async () => {},
  audioWaveforms: async () =>
    demo
      .filter((d) => !d.clip.name.startsWith("TIMELAPSE"))
      .map((d, i) => {
        const r = rng(d.clip.clip * 31);
        const nb = Math.floor(d.durationUs / 100_000);
        const peaks = new Float32Array(nb);
        let v = 0.3;
        for (let k = 0; k < nb; k++) {
          v += (r() - 0.5) * 0.15 + (0.35 - v) * 0.05;
          peaks[k] = Math.min(1, Math.max(0.02, v * (d.clip.name.startsWith("INTERVIEW") ? 1.4 : 0.8) * (0.6 + 0.4 * Math.abs(Math.sin(k / 3 + i)))));
        }
        return { clip: d.clip.clip, startUs: 0, peaks, lufs: -23 + (i % 5) };
      }),
  projectSaveAs: async (path) => ({ path, name: path.split(/[\\/]/).pop()!.replace(/\.clippro$/i, ""), recovered: false }),
  mediaRelink: async () => {},
  mediaRelinkFolder: async () => 0,
  settingsExport: async () => {},
  settingsImport: async () => mockSettings,
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
  // A demo offload: 3.2 GB over about 4 s, one checksum failure on the second destination.
  offloadStart: async (_source, destinations) => {
    const total = 3.2e9;
    let done = 0;
    mockOffloadCancel = false;
    const emit = (p: import("./api").OffloadProgress) => offloadListeners.forEach((l) => l(p));
    emit({ kind: "started", done: 0, total, files: 24, index: 0, file: null, message: null, summary: null });
    const tick = () => {
      if (mockOffloadCancel) return emit({ kind: "cancelled", done, total, files: 24, index: 0, file: null, message: null, summary: null });
      done = Math.min(total, done + total / 40);
      const index = Math.min(23, Math.floor((done / total) * 24));
      if (done < total) {
        emit({ kind: "progress", done, total, files: 24, index, file: `CLIP/A002C${String(index + 1).padStart(3, "0")}_241012_R2EC.MXF`, message: null, summary: null });
        setTimeout(tick, 100);
      } else {
        emit({
          kind: "finished",
          done,
          total,
          files: 24,
          index: 24,
          file: null,
          message: null,
          summary: { files: 24, bytes: total, copied: 23, skipped: 0, failed: destinations.length > 1 ? [`CLIP/A002C014_241012_R2EC.MXF → ${destinations[1]}: checksum mismatch`] : [], manifests: destinations.map((d) => `${d}/A002R2EC/ascmhl/0001_A002R2EC_2026-10-30_101500Z.mhl`), seconds: 4.1, imported: 0 },
        });
      }
    };
    setTimeout(tick, 100);
  },
  offloadCancel: async () => {
    mockOffloadCancel = true;
  },
  proxyCodecs: async () => [
    { value: "prores_proxy", label: "ProRes 422 Proxy" },
    { value: "prores_lt", label: "ProRes 422 LT" },
    { value: "dnxhr_lb", label: "DNxHR LB" },
    { value: "h264", label: "H.264" },
  ],
  proxiesMake: async (clips) => mockJob("proxy", clips.length),
  renderSelects: async (settings, _sort, request) => mockJob("render", Math.min(12, scoreAll(settings).selects.length), request.out_dir),
  diagnosticsLog: async () => [
    "2026-10-30 09:00:01Z INFO  ClipPro 0.1.0 started on windows (8 threads)",
    "2026-10-30 09:00:03Z INFO  project opened: <path>",
    "2026-10-30 09:04:40Z WARN  analysis job 12 failed: no video stream",
    "2026-10-30 09:12:09Z INFO  offload: 24 files, 23 copied, 0 skipped, 1 failed",
    "2026-10-30 09:20:15Z INFO  render job finished: 12 output(s)",
  ],
  diagnosticsSave: async () => {},
  diagnosticsOpenFolder: async () => {},
  jobCancel: async () => {
    mockJobCancel = true;
  },
  aiModels: async () => ({ models: mockModels.map((m) => ({ ...m })), dir: "C:/Users/you/AppData/Local/ClipPro/models", bytes: mockModels.filter((m) => m.installed).reduce((a, m) => a + m.size, 0) }),
  aiModelDownload: async (ids) => {
    for (const id of ids) {
      const m = mockModels.find((x) => x.id === id);
      if (!m || m.installed) continue;
      m.downloading = true;
      let f = 0;
      const tick = () => {
        f += 0.1;
        if (f >= 1) {
          m.downloading = false;
          m.installed = true;
          aiListeners.forEach((l) => l({ kind: "downloaded", model: id, clip: null, stage: null, fraction: 1, index: 0, count: 0, message: null }));
          return;
        }
        aiListeners.forEach((l) => l({ kind: "download", model: id, clip: null, stage: null, fraction: f, index: 0, count: 0, message: null }));
        setTimeout(tick, 150);
      };
      setTimeout(tick, 150);
    }
  },
  aiDownloadCancel: async () => {},
  aiModelDelete: async (id) => {
    const m = mockModels.find((x) => x.id === id);
    if (m) m.installed = false;
  },
  startupProject: async () => null,
  aiRun: async () => 0,
  aiCancel: async () => {},
  aiRunning: async () => false,
  aiClipInfo: async () =>
    demo.map((d) => ({
      clip: d.clip.clip,
      people: mockPeople.filter((p) => p.clips.includes(d.clip.clip)).map((p) => p.id),
      shot: SHOTS[d.clip.name] ?? (d.clip.name.startsWith("A") || d.clip.name.startsWith("B") ? "wide" : null),
      transcript: mockTranscript(d.clip.clip) !== null,
      language: mockTranscript(d.clip.clip) ? "en" : null,
      speakers: d.clip.clip === 6 ? 2 : d.clip.clip === 7 ? 1 : 0,
      facesDone: true,
      clipDone: true,
    })),
  peopleList: async () => mockPeople.map((p) => ({ ...p })),
  personRename: async (id, name) => {
    const p = mockPeople.find((x) => x.id === id);
    if (p) p.name = name;
  },
  personMerge: async (into, from) => {
    const a = mockPeople.find((x) => x.id === into);
    const i = mockPeople.findIndex((x) => x.id === from);
    if (a && i >= 0) {
      a.clips = [...new Set([...a.clips, ...mockPeople[i]!.clips])];
      mockPeople.splice(i, 1);
    }
  },
  transcriptGet: async (clip) => mockTranscript(clip),
  aiSearch: async (query, visual) => {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    if (visual)
      return demo
        .map((d, i) => ({ clip: d.clip.clip, startUs: 2e6, endUs: 6e6, text: d.clip.name, score: 0.3 - ((i * 7) % 10) / 100 + (q.split(" ").some((w) => d.clip.name.toLowerCase().includes(w)) ? 0.1 : 0) }))
        .sort((a, b) => b.score - a.score)
        .slice(0, 6);
    return [6, 7].flatMap((c) => mockTranscript(c)!.lines.filter((l) => q.split(/\s+/).every((w) => l.text.toLowerCase().includes(w))).map((l) => ({ clip: c, startUs: l.startUs, endUs: l.endUs, text: l.text, score: 1 })));
  },
  transcriptExport: async () => {},
  aiClear: async () => {},
  onAi: async (cb) => {
    aiListeners.add(cb);
    return () => aiListeners.delete(cb);
  },
  onJob: async (cb) => {
    jobListeners.add(cb);
    return () => jobListeners.delete(cb);
  },
  onOffload: async (cb) => {
    offloadListeners.add(cb);
    return () => offloadListeners.delete(cb);
  },
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
