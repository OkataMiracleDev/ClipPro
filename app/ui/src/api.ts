// Thin adapter over the generated bindings (bindings.ts). Unwraps results into exceptions,
// normalises `number | null` floats, decodes per-sample arrays, and falls back to a mock
// backend when running outside Tauri (browser preview, tests).

import { commands, events } from "./bindings";
import type {
  ClipRow_Serialize as RawClip,
  ColourProfile,
  ExportRequest,
  ExportResult,
  ImportResult,
  InspectResult_Serialize as RawInspect,
  MediaSource_Serialize as RawMediaSource,
  PresetInfo,
  ProjectInfo,
  ScoreResult,
  Settings_Serialize as RawSettings,
  SortOrder,
} from "./bindings";
import { mockBackend } from "./mock";

export type { ColourProfile, ExportRequest, ExportResult, ImportResult, PresetInfo, ProjectInfo, SortOrder };
export type Settings = RawSettings;

/** How the viewer shows a clip (ADR-002). */
export interface MediaSource {
  /** URL for `<video>` when the codec may play natively; frames are the fallback. */
  nativeUrl: string | null;
  durationUs: number;
  fps: number;
  width: number;
  height: number;
  codec: string;
  hasAudio: boolean;
}

export type ClipState = "queued" | "running" | "paused" | "done" | "failed";

export interface Clip {
  clip: number;
  media: number;
  name: string;
  path: string;
  state: ClipState;
  progress: number;
  error: string | null;
  profile: ColourProfile;
}

export interface Segment {
  startUs: number;
  endUs: number;
  inUs: number;
  outUs: number;
  mean: number;
  firstSample: number;
  lastSample: number;
  kind: "auto" | "forced" | "manual";
}

export interface ClipScore {
  clip: number;
  durationUs: number;
  rateHz: number;
  /** 0–1 per sample, NaN = no data. */
  bar: Float32Array;
  flags: Uint32Array;
  segments: Segment[];
}

export interface Select {
  clip: number;
  clipIndex: number;
  segment: Segment;
}

export interface Scored {
  clips: ClipScore[];
  selects: Select[];
  summary: { count: number; selectsUs: number; footageUs: number; percent: number; flagRuns: [string, number][] };
}

export interface AnalysisEvent {
  kind: "progress" | "finished" | "failed" | "paused" | "resumed";
  job: number | null;
  done: number | null;
  total: number | null;
  error: string | null;
}

/** Frame inspector data for one sample (SPEC VIE-2/4/5). NaN = unavailable. */
export interface Inspection {
  clip: number;
  sample: number;
  timeUs: number;
  pass: boolean;
  select: number | null;
  reasons: string[];
  score: number;
  overall: number;
  tierMultiplier: number;
  cameraFactor: number;
  forceIn: boolean;
  forceOut: boolean;
  flags: string[];
  groups: { key: string; label: string; score: number; weight: number }[];
  subs: { key: string; label: string; group: string; value: number; score: number; weight: number }[];
  raw: { key: string; value: number }[];
  tiles: { cols: number; rows: number; values: Float32Array };
  mvs: { cols: number; rows: number; values: Int16Array };
  hist: number[];
  working: { w: number; h: number };
}

export interface Backend {
  projectNew(path: string, name: string): Promise<ProjectInfo>;
  projectOpen(path: string): Promise<ProjectInfo>;
  projectClose(): Promise<void>;
  mediaImport(paths: string[]): Promise<ImportResult>;
  analysisStart(): Promise<number>;
  analysisPause(): Promise<void>;
  analysisResume(): Promise<void>;
  clipsList(): Promise<Clip[]>;
  presetsList(): Promise<PresetInfo[]>;
  presetSettings(name: string): Promise<Settings>;
  score(settings: Settings, sort: SortOrder): Promise<Scored>;
  exportSelects(settings: Settings, sort: SortOrder, request: ExportRequest): Promise<ExportResult>;
  onAnalysis(cb: (e: AnalysisEvent) => void): Promise<() => void>;
  mediaSource(media: number): Promise<MediaSource>;
  inspect(settings: Settings, clip: number, sample: number): Promise<Inspection>;
  /** Audio for frame playback (the viewer follows its clock). False when there's no audio. */
  audioPlay(media: number, us: number): Promise<boolean>;
  audioStop(): Promise<void>;
  /** Media time being heard (µs), null when no audio is playing. */
  audioClock(): Promise<number | null>;
  setProfile(media: number, profile: ColourProfile): Promise<void>;
  /** Built-in display transform for a log profile (decode LUT + gamut matrix). */
  previewColour(profile: ColourProfile): Promise<{ lut: Float32Array; matrix: Float32Array }>;
  /** Text of a user `.cube` file. */
  readLut(path: string): Promise<string>;
  /** Context menu (P5.3). `retry` resolves false when the clip can't be analysed. */
  retryClip(clip: number, cpu: boolean): Promise<boolean>;
  bumpClip(clip: number): Promise<void>;
  linkProxy(clip: number, path: string): Promise<void>;
  reveal(path: string): Promise<void>;
  systemInfo(): Promise<{ hwDecode: boolean; version: string }>;
  /** URL of a decoded preview frame (JPEG) at `us`, at most `width` pixels wide. */
  frameUrl(media: number, us: number, width: number): string;
}

export const n = (v: number | null | undefined, fallback = 0): number => (v === null || v === undefined || Number.isNaN(v) ? fallback : v);

function unwrap<T>(r: { status: "ok"; data: T } | { status: "error"; error: string }): T {
  if (r.status === "error") throw new Error(r.error);
  return r.data;
}

function b64(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export function decodeBar(s: string): Float32Array {
  const bytes = b64(s);
  const out = new Float32Array(bytes.length);
  for (let i = 0; i < bytes.length; i++) out[i] = bytes[i] === 255 ? NaN : bytes[i]! / 254;
  return out;
}

export function decodeFlags(s: string): Uint32Array {
  const bytes = b64(s);
  return new Uint32Array(bytes.buffer, bytes.byteOffset, Math.floor(bytes.length / 4)).slice();
}

type RawSegment = ScoreResult["clips"][number]["segments"][number];

const seg = (s: RawSegment): Segment => ({
  startUs: n(s.start_us),
  endUs: n(s.end_us),
  inUs: n(s.in_us),
  outUs: n(s.out_us),
  mean: n(s.mean),
  firstSample: n(s.first_sample),
  lastSample: n(s.last_sample),
  kind: s.kind,
});

export function adaptScore(r: ScoreResult): Scored {
  return {
    clips: r.clips.map((c) => ({
      clip: n(c.clip),
      durationUs: n(c.duration_us),
      rateHz: n(c.rate_hz, 4),
      bar: decodeBar(c.bar),
      flags: decodeFlags(c.flags),
      segments: c.segments.map(seg),
    })),
    selects: r.selects.map((s) => ({ clip: n(s.clip), clipIndex: n(s.clip_index), segment: seg(s.segment) })),
    summary: {
      count: n(r.summary.segment_count),
      selectsUs: n(r.summary.selects_us),
      footageUs: n(r.summary.footage_us),
      percent: n(r.summary.percent),
      flagRuns: r.summary.flag_runs.map(([f, c]) => [f, n(c)] as [string, number]),
    },
  };
}

const adaptClip = (c: RawClip): Clip => ({
  clip: n(c.clip),
  media: n(c.media),
  name: c.name,
  path: c.path,
  state: (c.state as ClipState) ?? "queued",
  progress: n(c.progress),
  error: c.error ?? null,
  profile: c.profile,
});

function adaptSource(r: RawMediaSource, toUrl: (path: string) => string): MediaSource {
  return {
    nativeUrl: r.native_path ? toUrl(r.native_path) : null,
    durationUs: n(r.duration_us),
    fps: n(r.fps, 25),
    width: r.width,
    height: r.height,
    codec: r.codec,
    hasAudio: r.has_audio,
  };
}

const f = (v: number | null | undefined) => (v === null || v === undefined ? NaN : v);

export function adaptInspect(clip: number, r: RawInspect): Inspection {
  return {
    clip,
    sample: r.sample,
    timeUs: n(r.time_us),
    pass: r.pass,
    select: r.select ?? null,
    reasons: r.reasons,
    score: f(r.score),
    overall: f(r.overall),
    tierMultiplier: f(r.tier_multiplier),
    cameraFactor: f(r.camera_factor),
    forceIn: r.force_in,
    forceOut: r.force_out,
    flags: r.flags,
    groups: r.groups.map((g) => ({ key: g.key, label: g.label, score: f(g.score), weight: f(g.weight) })),
    subs: r.subs.map((s) => ({ key: s.key, label: s.label, group: s.group, value: f(s.value), score: f(s.score), weight: f(s.weight) })),
    raw: r.raw.map((x) => ({ key: x.key, value: f(x.value) })),
    tiles: { cols: r.tile_cols, rows: r.tile_rows, values: Float32Array.from(r.tiles, f) },
    mvs: { cols: r.mv_cols, rows: r.mv_rows, values: Int16Array.from(r.mvs) },
    hist: r.hist,
    working: { w: r.working_w, h: r.working_h },
  };
}

const tauriBackend: Backend = {
  projectNew: async (path, name) => unwrap(await commands.projectNew(path, name)),
  projectOpen: async (path) => unwrap(await commands.projectOpen(path)),
  projectClose: async () => void unwrap(await commands.projectClose()),
  mediaImport: async (paths) => unwrap(await commands.mediaImport(paths)),
  analysisStart: async () => n(unwrap(await commands.analysisStart())),
  analysisPause: async () => void unwrap(await commands.analysisPause()),
  analysisResume: async () => void unwrap(await commands.analysisResume()),
  clipsList: async () => unwrap(await commands.clipsList()).map(adaptClip),
  presetsList: async () => unwrap(await commands.presetsList()),
  presetSettings: async (name) => unwrap(await commands.presetSettings(name)),
  score: async (settings, sort) => adaptScore(unwrap(await commands.score(settings, sort))),
  exportSelects: async (settings, sort, request) => unwrap(await commands.exportSelects(settings, sort, request)),
  mediaSource: async (media) => {
    const { convertFileSrc } = await import("@tauri-apps/api/core");
    return adaptSource(unwrap(await commands.mediaSource(media)), (p) => convertFileSrc(p));
  },
  inspect: async (settings, clip, sample) => adaptInspect(clip, unwrap(await commands.inspectSample(settings, clip, sample))),
  audioPlay: async (media, us) => unwrap(await commands.audioPlay(media, us)),
  audioStop: async () => void unwrap(await commands.audioStop()),
  audioClock: async () => unwrap(await commands.audioClock()) ?? null,
  setProfile: async (media, profile) => void unwrap(await commands.mediaSetProfile(media, profile)),
  previewColour: async (profile) => {
    const r = unwrap(await commands.previewColour(profile));
    return { lut: Float32Array.from(r.lut, (v) => n(v)), matrix: Float32Array.from(r.matrix, (v) => n(v)) };
  },
  readLut: async (path) => unwrap(await commands.lutRead(path)),
  retryClip: async (clip, cpu) => unwrap(await commands.clipRetry(clip, cpu)),
  bumpClip: async (clip) => void unwrap(await commands.clipBump(clip)),
  linkProxy: async (clip, path) => void unwrap(await commands.clipLinkProxy(clip, path)),
  reveal: async (path) => void unwrap(await commands.revealInExplorer(path)),
  systemInfo: async () => {
    const r = unwrap(await commands.systemInfo());
    return { hwDecode: r.hw_decode, version: r.version };
  },
  frameUrl: (media, us, width) => `http://clippro.localhost/frame/${media}/${Math.max(0, Math.round(us))}?w=${Math.round(width)}`,
  onAnalysis: async (cb) =>
    events.analysisEvent.listen((e) =>
      cb({
        kind: e.payload.kind as AnalysisEvent["kind"],
        job: e.payload.job ?? null,
        done: e.payload.done ?? null,
        total: e.payload.total ?? null,
        error: e.payload.error ?? null,
      }),
    ),
};

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const api: Backend = isTauri ? tauriBackend : mockBackend;
