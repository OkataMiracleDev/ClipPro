import { create } from "zustand";
import type { CubeLut } from "./lib/cube";
import type { Overlay } from "./lib/glview";
import type { MeasureOverlayKind } from "./views/MeasureOverlay";
import { api, isTauri, type AnalysisEvent, type Clip, type PresetInfo, type ProjectInfo, type Scored, type Settings, type SortOrder } from "./api";

export interface Toast {
  id: number;
  text: string;
  tone: "info" | "error";
}

interface AppState {
  project: ProjectInfo | null;
  clips: Clip[];
  presets: PresetInfo[];
  settings: Settings | null;
  /** Name of the preset the settings came from (settings may since have been edited). */
  presetName: string;
  modified: boolean;
  scored: Scored | null;
  scoring: boolean;
  sort: SortOrder;
  selectsOnly: boolean;
  selectedClip: number | null;
  selectedSelect: number | null;
  playheadUs: number;
  playing: boolean;
  /** Shuttle rate: ±1, ±2, ±4, ±8 (JKL). */
  rate: number;
  /** Loop the select under the playhead (or the clip) during playback. */
  loop: boolean;
  analysis: { active: boolean; paused: boolean; jobs: Record<number, number> };
  inspectorOpen: boolean;
  overlay: Overlay;
  /** Viewer LUT chosen by the user (replaces the built-in profile transform). */
  userLut: { name: string; lut: CubeLut } | null;
  /** Before/after split position (fraction of width), null = off. */
  split: number | null;
  /** A/B compare (P6.6): a second select shown beside the current one, synced from `anchorUs`. */
  compare: { clip: number; startUs: number; endUs: number; anchorUs: number } | null;
  measureOverlay: MeasureOverlayKind;
  rightTab: "tuning" | "frame";
  toasts: Toast[];

  init(): Promise<void>;
  newProject(path: string, name: string): Promise<void>;
  openProject(path: string): Promise<void>;
  closeProject(): Promise<void>;
  importPaths(paths: string[]): Promise<void>;
  startAnalysis(): Promise<void>;
  togglePause(): Promise<void>;
  refreshClips(): Promise<void>;
  setPreset(name: string): Promise<void>;
  updateSettings(fn: (s: Settings) => Settings): void;
  rescore(immediate?: boolean): void;
  setSort(s: SortOrder): void;
  toggleSelectsOnly(): void;
  selectClip(clip: number | null): void;
  selectSelect(i: number | null): void;
  setPlayhead(clip: number, us: number): void;
  setPlaying(p: boolean): void;
  setRate(r: number): void;
  toggleLoop(): void;
  toggleInspector(): void;
  /** Turns an overlay on, or off when it is already showing. */
  toggleOverlay(o: Overlay): void;
  setUserLut(l: { name: string; lut: CubeLut } | null): void;
  setSplit(x: number | null): void;
  setCompare(c: AppState["compare"]): void;
  setClipProfile(media: number, profile: import("./api").ColourProfile): Promise<void>;
  toggleMeasureOverlay(o: MeasureOverlayKind): void;
  setRightTab(t: "tuning" | "frame"): void;
  toast(text: string, tone?: Toast["tone"]): void;
  dismissToast(id: number): void;
}

let scoreSeq = 0;
let scoreTimer: ReturnType<typeof setTimeout> | null = null;
let toastId = 0;
let unlisten: (() => void) | null = null;

export const useApp = create<AppState>((set, get) => ({
  project: null,
  clips: [],
  presets: [],
  settings: null,
  presetName: "All-Rounder",
  modified: false,
  scored: null,
  scoring: false,
  sort: "timeline",
  selectsOnly: false,
  selectedClip: null,
  selectedSelect: null,
  playheadUs: 0,
  playing: false,
  rate: 1,
  loop: false,
  analysis: { active: false, paused: false, jobs: {} },
  inspectorOpen: true,
  overlay: "none",
  userLut: null,
  split: null,
  compare: null,
  measureOverlay: "none",
  rightTab: "tuning",
  toasts: [],

  async init() {
    try {
      const presets = await api.presetsList();
      set({ presets });
      await get().setPreset(get().presetName);
      if (!unlisten) unlisten = await api.onAnalysis((e) => onAnalysisEvent(e, set, get));
      // Browser preview: open the mock demo project straight away.
      if (!isTauri) await get().openProject("Demo.clippro");
    } catch (e) {
      get().toast(String(e), "error");
    }
  },

  async newProject(path, name) {
    try {
      const project = await api.projectNew(path, name);
      set({ project, clips: [], scored: null, selectedClip: null, playing: false });
    } catch (e) {
      get().toast(`Couldn't create the project: ${e}`, "error");
    }
  },

  async openProject(path) {
    try {
      const project = await api.projectOpen(path);
      set({ project, selectedClip: null, selectedSelect: null, playing: false });
      if (project.recovered) get().toast("ClipPro didn't close cleanly last time. The project was restored to its last saved state.");
      await get().refreshClips();
      get().rescore(true);
      await get().startAnalysis();
    } catch (e) {
      get().toast(`Couldn't open the project: ${e}`, "error");
    }
  },

  async closeProject() {
    await api.projectClose();
    set({ project: null, clips: [], scored: null, selectedClip: null, selectedSelect: null, playing: false });
  },

  async importPaths(paths) {
    try {
      const r = await api.mediaImport(paths);
      const parts = [`Added ${r.added} clip${r.added === 1 ? "" : "s"}`];
      if (r.duplicates) parts.push(`${r.duplicates} already in the project`);
      if (r.proxies_linked) parts.push(`${r.proxies_linked} proxy link${r.proxies_linked === 1 ? "" : "s"}`);
      if (r.cache_hits) parts.push(`${r.cache_hits} reused from earlier analysis`);
      get().toast(parts.join(" · "));
      for (const f of r.failed) get().toast(`Couldn't import ${f.path.split(/[\\/]/).pop()}: ${f.error}`, "error");
      await get().refreshClips();
      await get().startAnalysis();
      get().rescore(true);
    } catch (e) {
      get().toast(`Import failed: ${e}`, "error");
    }
  },

  async startAnalysis() {
    const n = await api.analysisStart();
    if (n > 0) set({ analysis: { active: true, paused: false, jobs: {} } });
    await get().refreshClips();
  },

  async togglePause() {
    const { analysis } = get();
    if (analysis.paused) await api.analysisResume();
    else await api.analysisPause();
    set({ analysis: { ...analysis, paused: !analysis.paused } });
  },

  async refreshClips() {
    if (!get().project) return;
    try {
      const clips = await api.clipsList();
      set({ clips });
      if (get().selectedClip === null && clips[0]) set({ selectedClip: clips[0].clip });
    } catch {
      /* no project open */
    }
  },

  async setPreset(name) {
    const settings = await api.presetSettings(name);
    set({ settings, presetName: name, modified: false });
    get().rescore(true);
  },

  updateSettings(fn) {
    const s = get().settings;
    if (!s) return;
    set({ settings: fn(structuredClone(s)), modified: true });
    get().rescore();
  },

  rescore(immediate = false) {
    if (scoreTimer) clearTimeout(scoreTimer);
    const run = async () => {
      const { settings, sort } = get();
      if (!settings) return;
      const seq = ++scoreSeq;
      set({ scoring: true });
      try {
        const scored = await api.score(settings, sort);
        if (seq === scoreSeq) set({ scored, scoring: false });
      } catch (e) {
        if (seq === scoreSeq) set({ scoring: false });
        if (get().project) get().toast(`Scoring failed: ${e}`, "error");
      }
    };
    if (immediate) void run();
    else scoreTimer = setTimeout(() => void run(), 16);
  },

  setSort(sort) {
    set({ sort });
    get().rescore(true);
  },
  toggleSelectsOnly: () => set((s) => ({ selectsOnly: !s.selectsOnly })),
  selectClip: (clip) => set((s) => ({ selectedClip: clip, selectedSelect: null, playing: s.playing && s.selectedClip === clip })),
  selectSelect: (i) => {
    const sel = i === null ? null : get().scored?.selects[i];
    set({ selectedSelect: i, ...(sel ? { selectedClip: sel.clip, playheadUs: sel.segment.startUs } : {}) });
  },
  setPlayhead: (clip, us) => set((s) => ({ selectedClip: clip, playheadUs: us, playing: s.playing && s.selectedClip === clip })),
  setPlaying: (playing) => set({ playing }),
  setRate: (rate) => set({ rate }),
  toggleLoop: () => set((s) => ({ loop: !s.loop })),
  toggleInspector: () => set((s) => ({ inspectorOpen: !s.inspectorOpen })),
  toggleOverlay: (o) => set((s) => ({ overlay: s.overlay === o ? "none" : o })),
  setUserLut: (userLut) => set({ userLut }),
  setSplit: (split) => set({ split }),
  setCompare: (compare) => set({ compare }),
  setClipProfile: async (media, profile) => {
    try {
      await api.setProfile(media, profile);
      set((s) => ({ clips: s.clips.map((c) => (c.media === media ? { ...c, profile } : c)) }));
      get().rescore(true);
    } catch (e) {
      get().toast(`Couldn't set the profile: ${e}`, "error");
    }
  },
  toggleMeasureOverlay: (o) => set((s) => ({ measureOverlay: s.measureOverlay === o ? "none" : o })),
  setRightTab: (rightTab) => set({ rightTab, inspectorOpen: true }),
  toast(text, tone = "info") {
    const id = ++toastId;
    set((s) => ({ toasts: [...s.toasts, { id, text, tone }] }));
    setTimeout(() => get().dismissToast(id), tone === "error" ? 8000 : 4000);
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

function onAnalysisEvent(e: AnalysisEvent, set: (p: Partial<AppState>) => void, get: () => AppState) {
  const a = get().analysis;
  if (e.kind === "progress" && e.job !== null) {
    set({ analysis: { ...a, active: true, jobs: { ...a.jobs, [e.job]: (e.done ?? 0) / Math.max(1, e.total ?? 1) } } });
  } else if (e.kind === "finished" || e.kind === "failed") {
    if (e.job !== null) set({ analysis: { ...a, jobs: { ...a.jobs, [e.job]: 1 } } });
    void get().refreshClips().then(() => {
      const busy = get().clips.some((c) => c.state === "queued" || c.state === "running");
      if (!busy) set({ analysis: { active: false, paused: false, jobs: {} } });
    });
    get().rescore();
    if (e.kind === "failed" && e.error) get().toast(`Analysis failed: ${e.error}`, "error");
  } else if (e.kind === "paused" || e.kind === "resumed") {
    set({ analysis: { ...get().analysis, paused: e.kind === "paused" } });
  }
}
