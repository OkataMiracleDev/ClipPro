// Thin adapter over the generated bindings (bindings.ts). Unwraps results into exceptions,
// normalises `number | null` floats, decodes per-sample arrays, and falls back to a mock
// backend when running outside Tauri (browser preview, tests).

import { commands, events } from "./bindings";
import type {
  ClipRow_Serialize as RawClip,
  ImportResult,
  PresetInfo,
  ProjectInfo,
  ScoreResult,
  Settings_Serialize as RawSettings,
  SortOrder,
} from "./bindings";
import { mockBackend } from "./mock";

export type { ImportResult, PresetInfo, ProjectInfo, SortOrder };
export type Settings = RawSettings;

export type ClipState = "queued" | "running" | "paused" | "done" | "failed";

export interface Clip {
  clip: number;
  media: number;
  name: string;
  path: string;
  state: ClipState;
  progress: number;
  error: string | null;
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
  onAnalysis(cb: (e: AnalysisEvent) => void): Promise<() => void>;
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
});

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
