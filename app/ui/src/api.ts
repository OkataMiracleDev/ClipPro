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

export interface RenderRequest {
  out_dir: string;
  template: string;
  stream_copy: boolean;
  allow_nearest_keyframe: boolean;
  /** prores422 | prores_hq | dnxhr_hq | h264 */
  codec: string;
  handles: boolean;
}

export interface JobProgress {
  job: "proxy" | "render";
  kind: "progress" | "finished" | "failed" | "cancelled";
  index: number;
  count: number;
  fraction: number;
  name: string | null;
  message: string | null;
  outputs: string[];
}

/** The Data folder: AI models and the measurement cache. */
export interface DataInfo {
  root: string;
  defaultRoot: string;
  isDefault: boolean;
  modelsDir: string;
  /** CLIPPRO_MODELS overrides the models folder (development setups). */
  modelsEnv: boolean;
  cachePath: string;
  cacheMovePending: boolean;
}

/** Local AI (P12): models, the AI queue, People, transcripts and search. */
export interface AiModel {
  id: string;
  module: "faces" | "clip" | "transcripts" | "speakers";
  name: string;
  size: number;
  licence: string;
  source: string;
  installed: boolean;
  downloading: boolean;
}

export interface AiProgress {
  kind: "download" | "downloaded" | "download_failed" | "progress" | "finished" | "failed" | "cancelled";
  model: string | null;
  clip: number | null;
  stage: "vision" | "transcript" | "speakers" | null;
  fraction: number;
  index: number;
  count: number;
  message: string | null;
}

export type ShotType = "wide" | "medium" | "close_up" | "extreme_close_up" | "insert" | "aerial" | "over_the_shoulder";

export interface ClipAi {
  clip: number;
  people: number[];
  shot: ShotType | null;
  transcript: boolean;
  language: string | null;
  speakers: number;
  facesDone: boolean;
  clipDone: boolean;
  /** Outdoor and night probabilities (AI-4); null before shot analysis. */
  outdoor: number | null;
  night: number | null;
}

export interface Person {
  id: number;
  name: string;
  clips: number[];
  cover: { clip: number; us: number; box: [number, number, number, number] | null } | null;
}

export interface Transcript {
  language: string;
  words: { startUs: number; endUs: number; text: string; prob: number; filler: boolean }[];
  lines: { startUs: number; endUs: number; text: string; speaker: number | null }[];
  flubs: [number, number][];
}

export interface SearchHit {
  clip: number;
  startUs: number;
  endUs: number;
  text: string;
  score: number;
}

export interface OffloadSummary {
  files: number;
  bytes: number;
  copied: number;
  skipped: number;
  failed: string[];
  manifests: string[];
  seconds: number;
  imported: number;
}

export interface OffloadProgress {
  kind: "started" | "progress" | "finished" | "failed" | "cancelled";
  done: number;
  total: number;
  files: number;
  index: number;
  file: string | null;
  message: string | null;
  summary: OffloadSummary | null;
}

export type SyncMethod = "audio" | "timecode";

export interface SyncMember {
  clip: number;
  name: string;
  angle: string;
  /** Start on the reference clip's clock (µs); null = not synced. */
  offsetUs: number | null;
  psr: number | null;
  tcOffsetUs: number | null;
  disagreementUs: number | null;
}

export interface SyncGroup {
  id: number | null;
  name: string;
  method: SyncMethod;
  reference: number;
  members: SyncMember[];
  warnings: string[];
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
  /** Take groups (AI, P12.6): rank 1 is the best take of its group. */
  takes: { clip: number; startUs: number; group: number; rank: number; size: number }[];
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
  /** A project passed on the command line (a double-clicked `.clippro` file). */
  startupProject(): Promise<string | null>;
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
  /** Multicam sync groups (P11.5). */
  syncClips(name: string, clips: number[], method: SyncMethod): Promise<SyncGroup>;
  syncGroups(): Promise<SyncGroup[]>;
  syncGroupSave(group: SyncGroup): Promise<number>;
  syncGroupDelete(id: number): Promise<void>;
  multicamExport(group: SyncGroup, format: string, path: string): Promise<ExportResult>;
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
  /** Offload (P13.1): runs in the background; progress arrives through `onOffload`. */
  offloadStart(source: string, destinations: string[], hash: string, addToProject: boolean): Promise<void>;
  offloadCancel(): Promise<void>;
  onOffload(cb: (e: OffloadProgress) => void): Promise<() => void>;
  /** Proxies and rendered selects (P13.2/P13.3): background jobs reported through `onJob`. */
  proxyCodecs(): Promise<{ value: string; label: string }[]>;
  proxiesMake(clips: number[], codec: string, maxHeight: number, outDir: string | null): Promise<void>;
  renderSelects(settings: Settings, sort: SortOrder, request: RenderRequest): Promise<void>;
  jobCancel(): Promise<void>;
  /** Diagnostics (P14.5): log tail, report file, log folder. */
  diagnosticsLog(lines: number): Promise<string[]>;
  diagnosticsSave(path: string): Promise<void>;
  diagnosticsOpenFolder(): Promise<void>;
  onJob(cb: (e: JobProgress) => void): Promise<() => void>;
  dataInfo(): Promise<DataInfo>;
  /** Changes the data folder (null = default), moving the models and cache when asked. */
  dataDirSet(path: string | null, moveExisting: boolean): Promise<{ info: DataInfo; moved: number; notes: string[] }>;
  aiModels(): Promise<{ models: AiModel[]; dir: string; bytes: number }>;
  aiModelDownload(ids: string[]): Promise<void>;
  aiDownloadCancel(id: string): Promise<void>;
  aiModelDelete(id: string): Promise<void>;
  /** Starts the AI queue now; resolves with the number of clips queued. */
  aiRun(): Promise<number>;
  aiCancel(): Promise<void>;
  aiRunning(): Promise<boolean>;
  aiClipInfo(): Promise<ClipAi[]>;
  peopleList(): Promise<Person[]>;
  personRename(id: number, name: string): Promise<void>;
  personMerge(into: number, from: number): Promise<void>;
  transcriptGet(clip: number): Promise<Transcript | null>;
  aiSearch(query: string, visual: boolean): Promise<SearchHit[]>;
  /** SRT or WebVTT by the file extension; a span exports just that part. */
  transcriptExport(clip: number, path: string, startUs: number | null, endUs: number | null): Promise<void>;
  aiClear(clip: number): Promise<void>;
  onAi(cb: (e: AiProgress) => void): Promise<() => void>;
  /** URL of a decoded preview frame (JPEG) at `us`, at most `width` pixels wide. */
  frameUrl(media: number, us: number, width: number): string;
}

export const n = (v: number | null | undefined, fallback = 0): number => (v === null || v === undefined || Number.isNaN(v) ? fallback : v);

function adaptSync(g: import("./bindings").SyncGroupDto_Serialize): SyncGroup {
  const opt = (v: number | null | undefined) => (v === null || v === undefined || Number.isNaN(v) ? null : v);
  return {
    id: opt(g.id),
    name: g.name,
    method: g.method === "timecode" ? "timecode" : "audio",
    reference: n(g.reference),
    warnings: g.warnings,
    members: g.members.map((m) => ({ clip: n(m.clip), name: m.name, angle: m.angle, offsetUs: opt(m.offset_us), psr: opt(m.psr), tcOffsetUs: opt(m.tc_offset_us), disagreementUs: opt(m.disagreement_us) })),
  };
}

function syncDto(g: SyncGroup): import("./bindings").SyncGroupDto_Deserialize {
  return {
    id: g.id,
    name: g.name,
    method: g.method,
    reference: g.reference,
    warnings: g.warnings,
    members: g.members.map((m) => ({ clip: m.clip, name: m.name, angle: m.angle, offset_us: m.offsetUs, psr: m.psr, tc_offset_us: m.tcOffsetUs, disagreement_us: m.disagreementUs })),
  };
}

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
    takes: r.takes.map((t) => ({ clip: n(t.clip), startUs: n(t.start_us), group: t.group, rank: t.rank, size: t.size })),
  };
}

const adaptData = (d: import("./bindings").DataInfo): DataInfo => ({
  root: d.root,
  defaultRoot: d.default_root,
  isDefault: d.is_default,
  modelsDir: d.models_dir,
  modelsEnv: d.models_env,
  cachePath: d.cache_path,
  cacheMovePending: d.cache_move_pending,
});

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
  syncClips: async (name, clips, method) => adaptSync(unwrap(await commands.syncClips(name, clips, method))),
  syncGroups: async () => unwrap(await commands.syncGroupsList()).map(adaptSync),
  syncGroupSave: async (group) => n(unwrap(await commands.syncGroupSave(syncDto(group)))),
  syncGroupDelete: async (id) => void unwrap(await commands.syncGroupDelete(id)),
  multicamExport: async (group, format, path) => unwrap(await commands.multicamExport(syncDto(group), format, path)),
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
  offloadStart: async (source, destinations, hash, addToProject) => void unwrap(await commands.offloadStart(source, destinations, hash, addToProject)),
  offloadCancel: async () => void (await commands.offloadCancel()),
  proxyCodecs: async () => unwrap(await commands.proxyCodecs()),
  proxiesMake: async (clips, codec, maxHeight, outDir) => void unwrap(await commands.proxiesMake(clips, codec, maxHeight, outDir)),
  renderSelects: async (settings, sort, request) => void unwrap(await commands.renderSelects(settings, sort, request)),
  jobCancel: async () => void (await commands.jobCancel()),
  diagnosticsLog: async (lines) => await commands.diagnosticsLog(lines),
  diagnosticsSave: async (path) => void unwrap(await commands.diagnosticsSave(path)),
  diagnosticsOpenFolder: async () => void unwrap(await commands.diagnosticsOpenFolder()),
  onJob: async (cb) =>
    events.jobProgress.listen((e) => {
      const p = e.payload;
      cb({ job: p.job === "render" ? "render" : "proxy", kind: p.kind as JobProgress["kind"], index: p.index, count: p.count, fraction: n(p.fraction), name: p.name ?? null, message: p.message ?? null, outputs: p.outputs });
    }),
  onOffload: async (cb) =>
    events.offloadProgress.listen((e) => {
      const p = e.payload;
      const sm = p.summary;
      cb({
        kind: p.kind as OffloadProgress["kind"],
        done: n(p.done),
        total: n(p.total),
        files: p.files,
        index: p.index,
        file: p.file ?? null,
        message: p.message ?? null,
        summary: sm ? { ...sm, bytes: n(sm.bytes), seconds: n(sm.seconds) } : null,
      });
    }),
  dataInfo: async () => adaptData(await commands.dataInfo()),
  dataDirSet: async (path, moveExisting) => {
    const r = unwrap(await commands.dataDirSet(path, moveExisting));
    return { info: adaptData(r.info), moved: r.moved, notes: r.notes };
  },
  aiModels: async () => {
    const r = await commands.aiModels();
    return { models: r.models.map((m) => ({ ...m, module: m.module as AiModel["module"], size: n(m.size) })), dir: r.dir, bytes: n(r.bytes) };
  },
  aiModelDownload: async (ids) => void unwrap(await commands.aiModelDownload(ids)),
  aiDownloadCancel: async (id) => void (await commands.aiDownloadCancel(id)),
  aiModelDelete: async (id) => void unwrap(await commands.aiModelDelete(id)),
  aiRun: async () => unwrap(await commands.aiRun()),
  aiCancel: async () => void (await commands.aiCancel()),
  aiRunning: async () => await commands.aiRunning(),
  aiClipInfo: async () =>
    unwrap(await commands.aiClipInfo()).map((c) => ({
      clip: n(c.clip),
      people: c.people.map((p) => n(p)),
      shot: (c.shot ?? null) as ShotType | null,
      transcript: c.transcript,
      language: c.language ?? null,
      speakers: c.speakers,
      facesDone: c.faces_done,
      clipDone: c.clip_done,
      outdoor: c.outdoor ?? null,
      night: c.night ?? null,
    })),
  peopleList: async () =>
    unwrap(await commands.peopleList()).map((p) => ({
      id: n(p.id),
      name: p.name,
      clips: p.clips.map((c) => n(c)),
      cover:
        p.cover_clip === null || p.cover_clip === undefined
          ? null
          : { clip: n(p.cover_clip), us: n(p.cover_us), box: p.cover_box && p.cover_box.length === 4 ? (p.cover_box.map((v) => n(v)) as [number, number, number, number]) : null },
    })),
  personRename: async (id, name) => void unwrap(await commands.personRename(id, name)),
  personMerge: async (into, from) => void unwrap(await commands.personMerge(into, from)),
  transcriptGet: async (clip) => {
    const t = unwrap(await commands.transcriptGet(clip));
    if (!t) return null;
    return {
      language: t.language,
      words: t.words.map((w) => ({ startUs: n(w.start_us), endUs: n(w.end_us), text: w.text, prob: n(w.prob), filler: w.filler })),
      lines: t.lines.map((l) => ({ startUs: n(l.start_us), endUs: n(l.end_us), text: l.text, speaker: l.speaker ?? null })),
      flubs: t.flubs.map(([a, b]) => [n(a), n(b)] as [number, number]),
    };
  },
  aiSearch: async (query, visual) =>
    unwrap(await commands.aiSearch(query, visual)).map((h) => ({ clip: n(h.clip), startUs: n(h.start_us), endUs: n(h.end_us), text: h.text, score: n(h.score) })),
  transcriptExport: async (clip, path, startUs, endUs) => void unwrap(await commands.transcriptExport(clip, path, startUs, endUs)),
  aiClear: async (clip) => void unwrap(await commands.aiClear(clip)),
  onAi: async (cb) =>
    events.aiProgress.listen((e) => {
      const p = e.payload;
      cb({ kind: p.kind as AiProgress["kind"], model: p.model ?? null, clip: p.clip ?? null, stage: (p.stage ?? null) as AiProgress["stage"], fraction: n(p.fraction), index: p.index, count: p.count, message: p.message ?? null });
    }),
  startupProject: async () => (await commands.startupProject()) ?? null,
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
