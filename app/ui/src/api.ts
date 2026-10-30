// Thin adapter over the generated bindings (bindings.ts). Unwraps results into exceptions,
// normalises `number | null` floats, decodes per-sample arrays, and falls back to a mock
// backend when running outside Tauri (browser preview, tests).

import { commands, events } from "./bindings";
import type {
  AppSettings_Serialize as AppSettings,
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

export type { AppSettings, ColourProfile, ExportRequest, ExportResult, ImportResult, PresetInfo, ProjectInfo, SortOrder };
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

export interface ReviewAlt {
  clip: number;
  clipName: string;
  startUs: number;
  endUs: number;
  mean: number;
  kind: "same_clip" | "take_group";
  similarity: number;
}

export interface ReviewItem {
  track: number;
  name: string;
  /** Timeline frames from the start. */
  recordIn: number;
  recordOut: number;
  clip: number | null;
  srcInUs: number;
  srcOutUs: number;
  mean: number | null;
  bars: number[];
  flags: string[];
  excluded: boolean;
  weak: boolean;
  alternatives: ReviewAlt[];
}

export interface Review {
  name: string;
  format: string;
  fps: number;
  startTcFrames: number;
  dropFrame: boolean;
  duration: number;
  tracks: number;
  items: ReviewItem[];
  linked: number;
  missing: string[];
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
  bin: number | null;
  camera: string | null;
  created: string | null;
  codec: string;
  missing: boolean;
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

export interface Bin {
  id: number;
  name: string;
  kind: string;
  query: string | null;
}

export interface OverrideInfo {
  id: number;
  clip: number;
  kind: "force_in" | "force_out" | "trim" | "manual";
  startUs: number;
  endUs: number;
}

export interface Annotation {
  clip: number;
  startUs: number;
  endUs: number;
  rating: number;
  label: string | null;
  note: string;
}

export interface WatchFolder {
  path: string;
  subfolders: boolean;
  extensions: string[];
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
  /** Custom presets (P9.4). */
  presetSave(name: string, description: string, settings: Settings, overwrite: boolean): Promise<void>;
  presetDelete(name: string): Promise<void>;
  presetRename(old: string, name: string): Promise<void>;
  presetImport(path: string): Promise<string>;
  presetExport(name: string, description: string, settings: Settings, path: string): Promise<void>;
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
  /** Sub-metrics in `Settings.subs` order. */
  submetrics(): Promise<import("./lib/submetrics").SubMetricInfo[]>;
  /** Clips best matching the camera-movement filter (live preview). */
  cameraMatches(settings: Settings, count: number): Promise<{ clip: number; name: string; score: number }[]>;
  getSettings(): Promise<AppSettings>;
  /** Global measurement cache (Settings → Cache). */
  cacheInfo(): Promise<{ path: string; entries: number; bytes: number }>;
  /** Audio peak envelopes per clip (100 ms bins, 0–1). */
  /** Audio streams of a clip and the chosen one (null = automatic). */
  audioStreams(clip: number): Promise<{ streams: { index: number; codec: string; channels: number; layout: string; language: string }[]; selected: number | null }>;
  audioStreamSet(clip: number, stream: number | null): Promise<void>;
  /** Imported-timeline review (P11.3) and export back with swaps and score markers (P11.4). */
  timelineReview(settings: Settings, path: string, weakBelow: number): Promise<Review>;
  timelineExportBack(swaps: { item: number; alternative: number }[], markers: boolean, format: string, path: string): Promise<ExportResult>;
  audioLevels(): Promise<{ levels: number[]; sourceChannels: number }>;
  audioChannelGains(gains: number[]): Promise<void>;
  audioWaveforms(): Promise<{ clip: number; startUs: number; peaks: Float32Array; lufs: number | null }[]>;
  projectSaveAs(path: string): Promise<ProjectInfo>;
  mediaRelink(media: number, path: string): Promise<void>;
  mediaRelinkFolder(folder: string): Promise<number>;
  cacheClear(): Promise<number>;
  settingsExport(path: string): Promise<void>;
  settingsImport(path: string): Promise<AppSettings>;
  setSettings(s: AppSettings): Promise<AppSettings>;
  openSettings(): Promise<void>;
  onSettings(cb: (s: AppSettings) => void): Promise<() => void>;
  /** Watch folders of the open project (P9.5). */
  watchFoldersGet(): Promise<WatchFolder[]>;
  /** Named tuning snapshots and the saved current tuning (P9.6); `json` is a TuningDoc. */
  snapshotsList(): Promise<{ id: number; name: string; json: string; created: string }[]>;
  snapshotSave(name: string, json: string): Promise<void>;
  snapshotDelete(id: number): Promise<void>;
  tuningSave(json: string): Promise<void>;
  tuningLoad(): Promise<string | null>;
  /** Bins and smart collections (P9.8); `query` is a SmartQuery document. */
  binsList(): Promise<Bin[]>;
  binAdd(name: string, kind: "manual" | "smart", query: string | null): Promise<number>;
  binUpdate(id: number, name: string, query: string | null): Promise<void>;
  binDelete(id: number): Promise<void>;
  clipsSetBin(clips: number[], bin: number | null): Promise<void>;
  /** Project templates: tuning and watch folders to start a project from. */
  templatesList(): Promise<{ name: string; json: string }[]>;
  templateSave(t: { name: string; json: string }): Promise<void>;
  templateDelete(name: string): Promise<void>;
  /** Manual overrides and annotations (P9.7). */
  overridesList(): Promise<OverrideInfo[]>;
  overrideAdd(clip: number, kind: OverrideInfo["kind"], startUs: number, endUs: number): Promise<number>;
  overrideRemove(id: number): Promise<void>;
  annotationsList(): Promise<Annotation[]>;
  annotationSet(a: Annotation): Promise<void>;
  watchFoldersSet(folders: WatchFolder[]): Promise<void>;
  onMediaAdded(cb: (e: { added: number; names: string[] }) => void): Promise<() => void>;
  /** URL of a decoded preview frame (JPEG) at `us`, at most `width` pixels wide. */
  frameUrl(media: number, us: number, width: number): string;
}

export const n = (v: number | null | undefined, fallback = 0): number => (v === null || v === undefined || Number.isNaN(v) ? fallback : v);

function adaptReview(r: import("./bindings").ReviewResult_Serialize): Review {
  return {
    name: r.name,
    format: r.format,
    fps: n(r.fps, 25),
    startTcFrames: n(r.start_tc_frames),
    dropFrame: r.drop_frame,
    duration: n(r.duration),
    tracks: r.tracks,
    linked: r.linked,
    missing: r.missing,
    items: r.items.map((i) => ({
      track: i.track,
      name: i.name,
      recordIn: n(i.record_in),
      recordOut: n(i.record_out),
      clip: i.clip ?? null,
      srcInUs: n(i.src_in_us),
      srcOutUs: n(i.src_out_us),
      mean: i.mean ?? null,
      bars: i.bars.map((v) => n(v)),
      flags: i.flags,
      excluded: i.excluded,
      weak: i.weak,
      alternatives: i.alternatives.map((a) => ({
        clip: n(a.clip),
        clipName: a.clip_name,
        startUs: n(a.start_us),
        endUs: n(a.end_us),
        mean: n(a.mean),
        kind: a.kind === "same_clip" ? "same_clip" : "take_group",
        similarity: n(a.similarity),
      })),
    })),
  };
}

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
  bin: c.bin === null || c.bin === undefined ? null : n(c.bin),
  camera: c.camera ?? null,
  created: c.created ?? null,
  codec: c.codec,
  missing: c.missing,
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
  presetSave: async (name, description, settings, overwrite) => void unwrap(await commands.presetSave(name, description, settings, overwrite)),
  presetDelete: async (name) => void unwrap(await commands.presetDelete(name)),
  presetRename: async (old, name) => void unwrap(await commands.presetRename(old, name)),
  presetImport: async (path) => unwrap(await commands.presetImport(path)),
  presetExport: async (name, description, settings, path) => void unwrap(await commands.presetExport(name, description, settings, path)),
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
  getSettings: async () => unwrap(await commands.settingsGet()),
  cacheInfo: async () => {
    const r = unwrap(await commands.cacheInfo());
    return { path: r.path, entries: n(r.entries), bytes: n(r.bytes) };
  },
  cacheClear: async () => n(unwrap(await commands.cacheClear())),
  audioStreams: async (clip) => {
    const r = unwrap(await commands.audioStreams(clip));
    return { streams: r.streams, selected: r.selected ?? null };
  },
  audioStreamSet: async (clip, stream) => void unwrap(await commands.audioStreamSet(clip, stream)),
  timelineReview: async (settings, path, weakBelow) => adaptReview(unwrap(await commands.timelineReview(settings, path, weakBelow))),
  timelineExportBack: async (swaps, markers, format, path) => unwrap(await commands.timelineExportBack(swaps, markers, format, path)),
  audioLevels: async () => {
    const r = unwrap(await commands.audioLevels());
    return { levels: r.levels.map((v) => n(v, -120)), sourceChannels: r.source_channels };
  },
  audioChannelGains: async (gains) => void unwrap(await commands.audioChannelGains(gains)),
  audioWaveforms: async () =>
    unwrap(await commands.audioWaveforms()).map((w) => ({ clip: n(w.clip), startUs: n(w.start_us), peaks: Float32Array.from(b64(w.peaks), (v) => v / 255), lufs: w.lufs ?? null })),
  projectSaveAs: async (path) => unwrap(await commands.projectSaveAs(path)),
  mediaRelink: async (media, path) => void unwrap(await commands.mediaRelink(media, path)),
  mediaRelinkFolder: async (folder) => unwrap(await commands.mediaRelinkFolder(folder)),
  settingsExport: async (path) => void unwrap(await commands.settingsExport(path)),
  settingsImport: async (path) => unwrap(await commands.settingsImport(path)),
  submetrics: async () => unwrap(await commands.submetricsList()),
  cameraMatches: async (settings, count) => unwrap(await commands.cameraMatches(settings, count)).map((m) => ({ clip: n(m.clip), name: m.name, score: n(m.score) })),
  setSettings: async (s) => unwrap(await commands.settingsSet(s)),
  openSettings: async () => void unwrap(await commands.openSettings()),
  onSettings: async (cb) => events.appSettings.listen((e) => cb(e.payload)),
  watchFoldersGet: async () => unwrap(await commands.watchFoldersGet()),
  snapshotsList: async () => unwrap(await commands.snapshotsList()).map((s) => ({ ...s, id: n(s.id) })),
  snapshotSave: async (name, json) => void unwrap(await commands.snapshotSave(name, json)),
  snapshotDelete: async (id) => void unwrap(await commands.snapshotDelete(id)),
  tuningSave: async (json) => void unwrap(await commands.tuningSave(json)),
  tuningLoad: async () => unwrap(await commands.tuningLoad()) ?? null,
  binsList: async () => unwrap(await commands.binsList()).map((b) => ({ id: n(b.id), name: b.name, kind: b.kind, query: b.query ?? null })),
  binAdd: async (name, kind, query) => n(unwrap(await commands.binAdd(name, kind, query))),
  binUpdate: async (id, name, query) => void unwrap(await commands.binUpdate(id, name, query)),
  binDelete: async (id) => void unwrap(await commands.binDelete(id)),
  clipsSetBin: async (clips, bin) => void unwrap(await commands.clipsSetBin(clips, bin)),
  templatesList: async () => unwrap(await commands.templatesList()),
  templateSave: async (t) => void unwrap(await commands.templateSave(t)),
  templateDelete: async (name) => void unwrap(await commands.templateDelete(name)),
  overridesList: async () =>
    unwrap(await commands.overridesList()).map((o) => ({ id: n(o.id), clip: n(o.clip), kind: o.kind as OverrideInfo["kind"], startUs: n(o.start_us), endUs: n(o.end_us) })),
  overrideAdd: async (clip, kind, startUs, endUs) => n(unwrap(await commands.overrideAdd(clip, kind, startUs, endUs))),
  overrideRemove: async (id) => void unwrap(await commands.overrideRemove(id)),
  annotationsList: async () =>
    unwrap(await commands.annotationsList()).map((a) => ({ clip: n(a.clip), startUs: n(a.start_us), endUs: n(a.end_us), rating: a.rating, label: a.label ?? null, note: a.note })),
  annotationSet: async (a) => void unwrap(await commands.annotationSet({ clip: a.clip, start_us: a.startUs, end_us: a.endUs, rating: a.rating, label: a.label, note: a.note })),
  watchFoldersSet: async (folders) => void unwrap(await commands.watchFoldersSet(folders)),
  onMediaAdded: async (cb) => events.mediaAdded.listen((e) => cb(e.payload)),
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
