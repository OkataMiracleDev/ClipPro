import { create } from "zustand";
import type { CubeLut } from "./lib/cube";
import { History } from "./lib/history";
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
  /** Settings as loaded from the preset (for the diff view). */
  baseSettings: Settings | null;
  modified: boolean;
  canUndo: boolean;
  canRedo: boolean;
  snapshots: { id: number; name: string; json: string; created: string }[];
  /** A/B compare of tunings: the other side, and which side is showing. */
  ab: { other: TuningDoc; showing: "a" | "b" } | null;
  overrides: import("./api").OverrideInfo[];
  bins: import("./api").Bin[];
  /** What the clip list shows (sidebar selection). */
  library: { kind: "all" } | { kind: "bin"; id: number } | { kind: "smart"; id: number };
  annotations: import("./api").Annotation[];
  /** In/out marks for a manual segment (I / O). */
  marks: { clip: number; inUs: number | null; outUs: number | null } | null;
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
  appSettings: import("./api").AppSettings | null;
  /** Viewer LUT chosen by the user (replaces the built-in profile transform). */
  userLut: { name: string; lut: CubeLut } | null;
  /** Before/after split position (fraction of width), null = off. */
  split: number | null;
  /** Luma waveform monitor (W). */
  scope: boolean;
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
  /** Context menu actions (P5.3). */
  retryClip(clip: number, cpu: boolean): Promise<void>;
  bumpClip(clip: number): Promise<void>;
  linkProxy(clip: number, path: string): Promise<void>;
  setPreset(name: string): Promise<void>;
  /** Saves the current tuning as a custom preset and switches to it. */
  savePreset(name: string, description: string, overwrite: boolean): Promise<boolean>;
  deletePreset(name: string): Promise<void>;
  renamePreset(old: string, name: string): Promise<boolean>;
  importPreset(path: string): Promise<void>;
  revertPreset(): void;
  undo(): void;
  redo(): void;
  refreshSnapshots(): Promise<void>;
  saveSnapshot(name: string): Promise<void>;
  applySnapshot(id: number): void;
  deleteSnapshot(id: number): Promise<void>;
  /** First press keeps the current tuning as A and carries on editing B; later presses swap. */
  toggleAB(): void;
  refreshOverrides(): Promise<void>;
  refreshBins(): Promise<void>;
  setLibrary(l: AppState["library"]): void;
  addBin(name: string, kind: "manual" | "smart", query?: import("./lib/smart").SmartQuery): Promise<void>;
  updateBin(id: number, name: string, query?: import("./lib/smart").SmartQuery): Promise<void>;
  deleteBin(id: number): Promise<void>;
  moveToBin(clips: number[], bin: number | null): Promise<void>;
  /** Project templates (P9.8): the open project's tuning and watch folders. */
  saveTemplate(name: string): Promise<void>;
  newProjectFromTemplate(path: string, name: string, template: { name: string; json: string }): Promise<void>;
  /** F: toggles a force-out on the select under the playhead, or force-in around it. */
  toggleForce(): Promise<void>;
  /** [ / ]: trims the select under the playhead so it starts / ends here. */
  trimSelect(side: "start" | "end"): Promise<void>;
  setMark(which: "in" | "out"): void;
  /** M: adds a manual segment between the in and out marks. */
  addManualSegment(): Promise<void>;
  removeOverride(id: number): Promise<void>;
  setAnnotation(a: import("./api").Annotation): Promise<void>;
  endAB(): void;
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
  applySettings(s: import("./api").AppSettings): void;
  setUserLut(l: { name: string; lut: CubeLut } | null): void;
  setSplit(x: number | null): void;
  toggleScope(): void;
  setCompare(c: AppState["compare"]): void;
  setClipProfile(media: number, profile: import("./api").ColourProfile): Promise<void>;
  toggleMeasureOverlay(o: MeasureOverlayKind): void;
  setRightTab(t: "tuning" | "frame"): void;
  toast(text: string, tone?: Toast["tone"]): void;
  dismissToast(id: number): void;
}

/** `system` follows the OS; `?theme=` in the URL (screenshots) wins over the setting. */
function applyTheme(theme: string) {
  if (new URLSearchParams(location.search).has("theme")) return;
  if (theme === "light" || theme === "dark") document.documentElement.dataset.theme = theme;
  else delete document.documentElement.dataset.theme;
}

/** What undo, snapshots and A/B save and restore. */
export interface TuningDoc {
  presetName: string;
  settings: Settings;
  baseSettings: Settings | null;
}

const history = new History<TuningDoc>(500, 400);
const undoFlags = () => ({ canUndo: history.canUndo, canRedo: history.canRedo });

function doc(s: { presetName: string; settings: Settings | null; baseSettings: Settings | null }): TuningDoc {
  return structuredClone({ presetName: s.presetName, settings: s.settings!, baseSettings: s.baseSettings });
}

function sameSettings(a: Settings | null, b: Settings | null) {
  return JSON.stringify(a) === JSON.stringify(b);
}

function applyDoc(set: (p: Partial<AppState>) => void, d: TuningDoc) {
  set({
    presetName: d.presetName,
    settings: structuredClone(d.settings),
    baseSettings: d.baseSettings ? structuredClone(d.baseSettings) : null,
    modified: !sameSettings(d.settings, d.baseSettings),
    ...undoFlags(),
  });
}

let tuningTimer: ReturnType<typeof setTimeout> | null = null;
/** Persists the tuning in effect to the project (debounced). */
function saveTuningSoon(get: () => AppState) {
  if (tuningTimer) clearTimeout(tuningTimer);
  tuningTimer = setTimeout(() => {
    const s = get();
    if (s.project && s.settings) void api.tuningSave(JSON.stringify(doc(s))).catch(() => {});
  }, 800);
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
  baseSettings: null,
  canUndo: false,
  canRedo: false,
  snapshots: [],
  ab: null,
  overrides: [],
  bins: [],
  library: { kind: "all" },
  annotations: [],
  marks: null,
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
  appSettings: null,
  userLut: null,
  split: null,
  scope: false,
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
      const st = await api.getSettings();
      get().applySettings(st);
      void api.onSettings((s) => get().applySettings(s));
      void api.onMediaAdded((e) => {
        set((s) => ({ analysis: { ...s.analysis, active: true } }));
        void get().refreshClips();
        get().toast(`Watch folder: added ${e.added === 1 ? (e.names[0] ?? "1 clip") : `${e.added} clips`}`);
      });
      // Browser preview: open the mock demo project straight away. The app reopens the last one.
      if (!isTauri) await get().openProject("Demo.clippro");
      else if (st.reopen_last && st.last_project) await get().openProject(st.last_project);
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
      set({ project, selectedClip: null, selectedSelect: null, playing: false, ab: null });
      if (project.recovered) get().toast("ClipPro didn't close cleanly last time. The project was restored to its last saved state.");
      // Restore the project's last tuning (P9.6); fall back to the current preset.
      try {
        const saved = await api.tuningLoad();
        if (saved) applyDoc(set, JSON.parse(saved) as TuningDoc);
      } catch {
        /* keep the current tuning */
      }
      history.clear();
      set(undoFlags());
      void get().refreshSnapshots();
      void get().refreshOverrides();
      void get().refreshBins();
      set({ marks: null, library: { kind: "all" } });
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

  async retryClip(clip, cpu) {
    try {
      const ok = await api.retryClip(clip, cpu);
      if (!ok) get().toast("This clip can't be analysed (no video, or it needs a proxy).", "error");
      else set((s) => ({ analysis: { ...s.analysis, active: true } }));
      await get().refreshClips();
    } catch (e) {
      get().toast(`Retry failed: ${e}`, "error");
    }
  },

  async bumpClip(clip) {
    try {
      await api.bumpClip(clip);
    } catch (e) {
      get().toast(`${e}`, "error");
    }
  },

  async linkProxy(clip, path) {
    try {
      await api.linkProxy(clip, path);
      set((s) => ({ analysis: { ...s.analysis, active: true } }));
      get().toast(`Linked proxy ${path.split(/[\\/]/).pop()}; analysing it now`);
      await get().refreshClips();
    } catch (e) {
      get().toast(`Couldn't link the proxy: ${e instanceof Error ? e.message : e}`, "error");
    }
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
    if (get().settings) {
      history.record(doc(get()));
      history.seal();
    }
    set({ settings, baseSettings: structuredClone(settings), presetName: name, modified: false, ...undoFlags() });
    get().rescore(true);
    saveTuningSoon(get);
  },

  undo() {
    const s = get();
    if (!s.settings) return;
    const prev = history.undo(doc(s));
    if (prev) {
      applyDoc(set, prev);
      get().rescore(true);
      saveTuningSoon(get);
    }
  },

  redo() {
    const s = get();
    if (!s.settings) return;
    const next = history.redo(doc(s));
    if (next) {
      applyDoc(set, next);
      get().rescore(true);
      saveTuningSoon(get);
    }
  },

  async refreshSnapshots() {
    if (!get().project) return set({ snapshots: [] });
    try {
      set({ snapshots: await api.snapshotsList() });
    } catch {
      set({ snapshots: [] });
    }
  },

  async saveSnapshot(name) {
    const s = get();
    if (!s.settings) return;
    try {
      await api.snapshotSave(name, JSON.stringify(doc(s)));
      await get().refreshSnapshots();
      get().toast(`Saved snapshot “${name.trim()}”`);
    } catch (e) {
      get().toast(String(e instanceof Error ? e.message : e), "error");
    }
  },

  applySnapshot(id) {
    const snap = get().snapshots.find((x) => x.id === id);
    if (!snap || !get().settings) return;
    history.record(doc(get()));
    history.seal();
    applyDoc(set, JSON.parse(snap.json) as TuningDoc);
    get().rescore(true);
    saveTuningSoon(get);
  },

  async deleteSnapshot(id) {
    await api.snapshotDelete(id);
    await get().refreshSnapshots();
  },

  toggleAB() {
    const s = get();
    if (!s.settings) return;
    if (!s.ab) {
      set({ ab: { other: doc(s), showing: "b" } });
      return;
    }
    const other = s.ab.other;
    const here = doc(s);
    applyDoc(set, other);
    set({ ab: { other: here, showing: s.ab.showing === "a" ? "b" : "a" } });
    get().rescore(true);
  },

  endAB() {
    set({ ab: null });
  },

  async refreshBins() {
    if (!get().project) return set({ bins: [] });
    try {
      set({ bins: await api.binsList() });
    } catch {
      /* keep */
    }
  },

  setLibrary: (library) => set({ library, ...(library.kind !== "all" ? { selectsOnly: false } : {}) }),

  async addBin(name, kind, query) {
    try {
      const id = await api.binAdd(name, kind, query ? JSON.stringify(query) : null);
      await get().refreshBins();
      set({ library: { kind: kind === "smart" ? "smart" : "bin", id } });
    } catch (e) {
      get().toast(String(e instanceof Error ? e.message : e), "error");
    }
  },

  async updateBin(id, name, query) {
    await api.binUpdate(id, name, query ? JSON.stringify(query) : null);
    await get().refreshBins();
  },

  async deleteBin(id) {
    await api.binDelete(id);
    const l = get().library;
    if (l.kind !== "all" && l.id === id) set({ library: { kind: "all" } });
    await get().refreshBins();
    await get().refreshClips();
  },

  async saveTemplate(name) {
    const s = get();
    if (!s.settings || !s.project) return;
    try {
      const watchFolders = await api.watchFoldersGet();
      await api.templateSave({ name: name.trim(), json: JSON.stringify({ tuning: doc(s), watchFolders }) });
      get().toast(`Saved template “${name.trim()}”`);
    } catch (e) {
      get().toast(String(e instanceof Error ? e.message : e), "error");
    }
  },

  async newProjectFromTemplate(path, name, template) {
    await get().newProject(path, name);
    if (!get().project) return;
    try {
      const t = JSON.parse(template.json) as { tuning?: TuningDoc; watchFolders?: import("./api").WatchFolder[] };
      if (t.tuning) {
        applyDoc(set, t.tuning);
        history.clear();
        set(undoFlags());
        saveTuningSoon(get);
      }
      if (t.watchFolders?.length) await api.watchFoldersSet(t.watchFolders);
      get().toast(`New project from “${template.name}”`);
    } catch (e) {
      get().toast(`The template couldn't be applied: ${e}`, "error");
    }
  },

  async moveToBin(clips, bin) {
    await api.clipsSetBin(clips, bin);
    await get().refreshClips();
    const name = bin === null ? "the project" : get().bins.find((b) => b.id === bin)?.name;
    get().toast(`Moved ${clips.length === 1 ? "1 clip" : `${clips.length} clips`} to ${name}`);
  },

  async refreshOverrides() {
    if (!get().project) return set({ overrides: [], annotations: [] });
    try {
      const [overrides, annotations] = await Promise.all([api.overridesList(), api.annotationsList()]);
      set({ overrides, annotations });
    } catch {
      /* keep what we have */
    }
  },

  async toggleForce() {
    const s = get();
    if (s.selectedClip === null || !s.scored) return;
    const clip = s.selectedClip;
    const at = s.playheadUs;
    const hit = s.overrides.find((o) => o.clip === clip && (o.kind === "force_in" || o.kind === "force_out") && at >= o.startUs && at < o.endUs);
    try {
      if (hit) await api.overrideRemove(hit.id);
      else {
        const seg = s.scored.clips.find((c) => c.clip === clip)?.segments.find((g) => at >= g.startUs && at < g.endUs);
        if (seg) await api.overrideAdd(clip, "force_out", seg.startUs, seg.endUs);
        else {
          const m = s.marks?.clip === clip && s.marks.inUs !== null && s.marks.outUs !== null && s.marks.outUs > s.marks.inUs ? s.marks : null;
          const dur = s.scored.clips.find((c) => c.clip === clip)?.durationUs ?? at + 2e6;
          await api.overrideAdd(clip, "force_in", m ? m.inUs! : Math.max(0, at - 1e6), m ? m.outUs! : Math.min(dur, at + 1e6));
        }
      }
      await get().refreshOverrides();
      get().rescore(true);
    } catch (e) {
      get().toast(String(e instanceof Error ? e.message : e), "error");
    }
  },

  async trimSelect(side) {
    const s = get();
    if (s.selectedClip === null || !s.scored) return;
    const clip = s.selectedClip;
    const at = s.playheadUs;
    const seg = s.scored.clips.find((c) => c.clip === clip)?.segments.find((g) => at >= g.startUs - 1 && at <= g.endUs + 1);
    if (!seg) return get().toast("Put the playhead inside a select to trim it.", "error");
    const [a, b] = side === "start" ? [at, seg.endUs] : [seg.startUs, at];
    if (b - a < 100_000) return get().toast("That would leave less than 0.1 s.", "error");
    try {
      // A new trim replaces earlier trims of the same select.
      for (const o of s.overrides.filter((o) => o.clip === clip && o.kind === "trim" && Math.min(o.endUs, seg.endUs) > Math.max(o.startUs, seg.startUs))) await api.overrideRemove(o.id);
      await api.overrideAdd(clip, "trim", a, b);
      await get().refreshOverrides();
      get().rescore(true);
    } catch (e) {
      get().toast(String(e instanceof Error ? e.message : e), "error");
    }
  },

  setMark(which) {
    const s = get();
    if (s.selectedClip === null) return;
    const cur = s.marks?.clip === s.selectedClip ? s.marks : { clip: s.selectedClip, inUs: null, outUs: null };
    set({ marks: which === "in" ? { ...cur, inUs: s.playheadUs } : { ...cur, outUs: s.playheadUs } });
  },

  async addManualSegment() {
    const s = get();
    const m = s.marks;
    if (!m || m.inUs === null || m.outUs === null || m.outUs <= m.inUs) return get().toast("Set an in point (I) and an out point (O) first.", "error");
    try {
      await api.overrideAdd(m.clip, "manual", m.inUs, m.outUs);
      set({ marks: null });
      await get().refreshOverrides();
      get().rescore(true);
    } catch (e) {
      get().toast(String(e instanceof Error ? e.message : e), "error");
    }
  },

  async removeOverride(id) {
    await api.overrideRemove(id);
    await get().refreshOverrides();
    get().rescore(true);
  },

  async setAnnotation(a) {
    try {
      await api.annotationSet(a);
      await get().refreshOverrides();
    } catch (e) {
      get().toast(String(e instanceof Error ? e.message : e), "error");
    }
  },

  async savePreset(name, description, overwrite) {
    const { settings } = get();
    if (!settings) return false;
    try {
      await api.presetSave(name, description, settings, overwrite);
      set({ presets: await api.presetsList() });
      await get().setPreset(name);
      get().toast(`Saved preset “${name}”`);
      return true;
    } catch (e) {
      get().toast(String(e instanceof Error ? e.message : e), "error");
      return false;
    }
  },

  async deletePreset(name) {
    try {
      await api.presetDelete(name);
      set({ presets: await api.presetsList() });
      if (get().presetName === name) await get().setPreset("All-Rounder");
    } catch (e) {
      get().toast(String(e instanceof Error ? e.message : e), "error");
    }
  },

  async renamePreset(old, name) {
    try {
      await api.presetRename(old, name);
      set({ presets: await api.presetsList() });
      if (get().presetName === old) set({ presetName: name });
      return true;
    } catch (e) {
      get().toast(String(e instanceof Error ? e.message : e), "error");
      return false;
    }
  },

  async importPreset(path) {
    try {
      const name = await api.presetImport(path);
      set({ presets: await api.presetsList() });
      await get().setPreset(name);
      get().toast(`Imported preset “${name}”`);
    } catch (e) {
      get().toast(String(e instanceof Error ? e.message : e), "error");
    }
  },

  revertPreset() {
    const { baseSettings } = get();
    if (!baseSettings) return;
    history.record(doc(get()));
    history.seal();
    set({ settings: structuredClone(baseSettings), modified: false, ...undoFlags() });
    saveTuningSoon(get);
    get().rescore(true);
  },

  updateSettings(fn) {
    const s = get().settings;
    if (!s) return;
    history.record(doc(get()));
    set({ settings: fn(structuredClone(s)), modified: true, ...undoFlags() });
    get().rescore();
    saveTuningSoon(get);
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
  applySettings: (appSettings) => {
    set({ appSettings });
    applyTheme(appSettings.theme);
  },
  setUserLut: (userLut) => set({ userLut }),
  setSplit: (split) => set({ split }),
  toggleScope: () => set((s) => ({ scope: !s.scope })),
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
