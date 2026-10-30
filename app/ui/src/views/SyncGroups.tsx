// Multicam sync groups (SPEC EXP-11, P11.5): pick clips, sync them by audio or timecode, check
// the offsets, save the group, and export it as a multicam clip or stacked tracks.

import * as RDialog from "@radix-ui/react-dialog";
import { AlertTriangle, Layers, Plus, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { create } from "zustand";
import { api, type SyncGroup, type SyncMethod } from "../api";
import { Button, cx, IconButton, Segmented, Spinner, Tip } from "../components/ui";
import { pickExportPath } from "../lib/dialogs";
import { useApp } from "../store";

interface SyncSheetState {
  open: boolean;
  clips: number[];
  group: SyncGroup | null;
  groups: SyncGroup[];
  show: (clips: number[], group?: SyncGroup | null) => void;
  close: () => void;
  refresh: () => Promise<void>;
}

export const useSync = create<SyncSheetState>((set) => ({
  open: false,
  clips: [],
  group: null,
  groups: [],
  show: (clips, group = null) => set({ open: true, clips: group ? group.members.map((m) => m.clip) : clips, group }),
  close: () => set({ open: false }),
  refresh: async () => {
    try {
      set({ groups: await api.syncGroups() });
    } catch {
      set({ groups: [] });
    }
  },
}));

/** Signed seconds: "+3.48 s", "−0.04 s". */
export function signedSeconds(us: number): string {
  const s = us / 1e6;
  const v = Math.abs(s) < 0.005 ? "0.00" : Math.abs(s).toFixed(2);
  return `${s < -0.0049 ? "−" : s > 0.0049 ? "+" : ""}${v} s`;
}

/** Sidebar section: saved groups; + starts a new one from the selected clip. */
export function SyncGroupsSection() {
  const project = useApp((s) => s.project);
  const selected = useApp((s) => s.selectedClip);
  const { groups, refresh, show } = useSync();
  useEffect(() => {
    if (project) void refresh();
  }, [project, refresh]);
  if (!project) return null;
  return (
    <div>
      <div className="mb-1 flex items-center justify-between px-2">
        <span className="section-header">Sync groups</span>
        <button aria-label="New sync group" title="Sync clips…" className="rounded p-0.5 text-text-3 hover:bg-control-hover hover:text-text" onClick={() => show(selected === null ? [] : [selected])}>
          <Plus size={13} />
        </button>
      </div>
      {groups.length === 0 ? (
        <div className="px-2 text-[11px] text-text-3">Line up cameras by their audio or timecode, then export a multicam clip.</div>
      ) : (
        groups.map((g) => (
          <div key={g.id} className="group flex h-7 items-center gap-2 rounded-md px-2 text-[12px] text-text-2 hover:bg-control-hover">
            <Layers size={13} className="shrink-0 text-accent" />
            <button className="min-w-0 flex-1 truncate text-left" onClick={() => show([], g)}>
              {g.name}
            </button>
            <span className="font-mono text-[11px] text-text-3 tabular">{g.members.length}</span>
            <button
              aria-label={`Delete ${g.name}`}
              className="hidden rounded p-0.5 text-text-3 group-hover:block hover:text-text"
              onClick={() => void api.syncGroupDelete(g.id!).then(refresh)}
            >
              <X size={12} />
            </button>
          </div>
        ))
      )}
    </div>
  );
}

function Offsets({ g, durations }: { g: SyncGroup; durations: Map<number, number> }) {
  const placed = g.members.filter((m) => m.offsetUs !== null);
  const lo = Math.min(0, ...placed.map((m) => m.offsetUs!));
  const hi = Math.max(...placed.map((m) => m.offsetUs! + (durations.get(m.clip) ?? 0)), 1);
  const span = Math.max(1, hi - lo);
  return (
    <div className="flex flex-col gap-1.5" role="table" aria-label="Offsets">
      {[...g.members].sort((a, b) => Number(b.clip === g.reference) - Number(a.clip === g.reference) || (a.offsetUs ?? Infinity) - (b.offsetUs ?? Infinity)).map((m) => {
        const dur = durations.get(m.clip) ?? 0;
        const isRef = m.clip === g.reference;
        return (
          <div key={m.clip} role="row" className="grid grid-cols-[9rem_1fr_5rem_6.5rem] items-center gap-3">
            <span role="cell" className="truncate text-[12px]" title={m.name}>
              {m.angle}
            </span>
            <span role="cell" className="relative h-5 rounded bg-control-hover/50 ring-1 ring-separator ring-inset">
              {m.offsetUs === null ? (
                <span className="absolute inset-0 grid place-items-center text-[10.5px] text-text-3">Not synced</span>
              ) : (
                <span
                  className={cx("absolute inset-y-0.5 truncate rounded-[3px] px-1.5 text-[10.5px] leading-4 font-medium", isRef ? "bg-accent text-accent-fg" : "bg-accent-soft text-text ring-1 ring-accent/60 ring-inset")}
                  style={{ left: `${((m.offsetUs - lo) / span) * 100}%`, width: `max(4px, ${(Math.max(dur, span * 0.01) / span) * 100}%)` }}
                >
                  {m.name}
                </span>
              )}
            </span>
            <span role="cell" className="text-right font-mono text-[12px] tabular">
              {m.offsetUs === null ? "—" : signedSeconds(m.offsetUs)}
            </span>
            <span role="cell" className="text-right text-[11px] text-text-3">
              {m.psr !== null ? (
                <Tip label="Peak-to-sidelobe ratio of the audio match; 6 or more is accepted.">
                  <span className={cx("font-mono tabular", m.psr >= 10 ? "text-[var(--c-score-hi)]" : "text-[var(--c-score-mid)]")}>PSR {m.psr.toFixed(1)}</span>
                </Tip>
              ) : m.tcOffsetUs !== null && g.method === "timecode" ? (
                "timecode"
              ) : isRef ? (
                "Reference"
              ) : (
                "—"
              )}
              {m.disagreementUs !== null && m.disagreementUs > 0 && !isRef && <span className="block font-mono text-[10.5px] tabular">TC Δ {Math.round(m.disagreementUs / 1000)} ms</span>}
            </span>
          </div>
        );
      })}
    </div>
  );
}

const EXPORTS = [
  { value: "fcpxml", label: "FCPXML multicam", title: "Final Cut Pro multicam clip", ext: "fcpxml", filter: "FCPXML" },
  { value: "xmeml", label: "XML tracks", title: "Stacked tracks for Premiere Pro and Resolve", ext: "xml", filter: "FCP7 XML" },
  { value: "otio", label: "OTIO tracks", title: "Stacked tracks in OpenTimelineIO", ext: "otio", filter: "OpenTimelineIO" },
] as const;

export function SyncSheet() {
  const { open, clips: initial, group: saved, close, refresh, groups } = useSync();
  const { clips, scored, toast } = useApp();
  const [name, setName] = useState("");
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const [method, setMethod] = useState<SyncMethod>("audio");
  const [filter, setFilter] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<SyncGroup | null>(null);
  const [format, setFormat] = useState<(typeof EXPORTS)[number]["value"]>("fcpxml");

  useEffect(() => {
    if (!open) return;
    setPicked(new Set(initial));
    setResult(saved);
    setName(saved?.name ?? `Sync group ${groups.length + 1}`);
    setMethod(saved?.method ?? "audio");
    setFilter("");
    // Only when the sheet opens.
  }, [open]);

  const durations = useMemo(() => new Map((scored?.clips ?? []).map((c) => [c.clip, c.durationUs])), [scored]);
  const shown = clips.filter((c) => !filter || c.name.toLowerCase().includes(filter.toLowerCase()) || (c.camera ?? "").toLowerCase().includes(filter.toLowerCase()));

  async function run() {
    setBusy(true);
    try {
      const g = await api.syncClips(name.trim() || "Sync group", [...picked], method);
      setResult({ ...g, id: saved?.id ?? null });
    } catch (e) {
      toast(`Sync failed: ${e}`, "error");
    } finally {
      setBusy(false);
    }
  }

  async function save() {
    if (!result) return;
    try {
      const id = await api.syncGroupSave({ ...result, name: name.trim() || result.name });
      setResult({ ...result, id, name: name.trim() || result.name });
      await refresh();
      toast(`Saved “${name.trim() || result.name}”`);
    } catch (e) {
      toast(`Couldn’t save: ${e}`, "error");
    }
  }

  async function exportIt() {
    if (!result) return;
    const f = EXPORTS.find((x) => x.value === format)!;
    const path = await pickExportPath(`${(name.trim() || result.name).replace(/[\\/:*?"<>|]/g, "-")} multicam`, f.ext, f.filter, "Export multicam");
    if (!path) return;
    try {
      const r = await api.multicamExport({ ...result, name: name.trim() || result.name }, format, path);
      toast(`Exported ${r.selects} angle${r.selects === 1 ? "" : "s"} to ${r.path.split(/[\\/]/).pop()}`);
    } catch (e) {
      toast(`Export failed: ${e}`, "error");
    }
  }

  const editing = result === null;
  return (
    <RDialog.Root open={open} onOpenChange={(o) => !o && close()}>
      <RDialog.Portal>
        <RDialog.Overlay className="fixed inset-0 z-40 bg-black/25" />
        <RDialog.Content
          aria-describedby={undefined}
          onOpenAutoFocus={(e) => e.preventDefault()}
          className="fixed top-(--h-titlebar) left-1/2 z-50 flex max-h-[84vh] w-[720px] max-w-[94vw] -translate-x-1/2 flex-col gap-3 rounded-b-(--radius-sheet) bg-raised p-5 shadow-(--shadow-float) ring-1 ring-separator-strong data-[state=open]:animate-[sheet_220ms_var(--ease-spring)]"
        >
          <div className="flex items-center gap-3">
            <RDialog.Title className="text-[15px] font-semibold">Sync clips</RDialog.Title>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              aria-label="Group name"
              className="h-6 min-w-0 flex-1 rounded border border-separator-strong bg-control px-1.5 text-[12.5px]"
            />
            <RDialog.Close asChild>
              <IconButton label="Close">
                <X size={15} />
              </IconButton>
            </RDialog.Close>
          </div>
          {editing ? (
            <>
              <div className="flex items-center gap-3">
                <Segmented
                  label="Sync by"
                  value={method}
                  onChange={setMethod}
                  options={[
                    { value: "audio", label: "Audio", title: "Match the sound recorded by each camera" },
                    { value: "timecode", label: "Timecode", title: "Line up start timecodes" },
                  ]}
                />
                <input
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                  placeholder="Filter clips"
                  aria-label="Filter clips"
                  className="h-6 min-w-0 flex-1 rounded border border-separator-strong bg-control px-1.5 text-[12px]"
                />
                <span className="text-[12px] text-text-3 tabular">{picked.size} chosen</span>
              </div>
              <div className="min-h-0 flex-1 overflow-y-auto rounded-(--radius-control) ring-1 ring-separator" role="listbox" aria-multiselectable aria-label="Clips">
                {shown.map((c) => {
                  const on = picked.has(c.clip);
                  return (
                    <button
                      key={c.clip}
                      role="option"
                      aria-selected={on}
                      onClick={() =>
                        setPicked((p) => {
                          const n = new Set(p);
                          if (on) n.delete(c.clip);
                          else n.add(c.clip);
                          return n;
                        })
                      }
                      className="flex h-7 w-full items-center gap-2 border-b border-separator px-2.5 text-left text-[12.5px] last:border-b-0 hover:bg-control-hover/60"
                    >
                      <span className={cx("grid h-3.5 w-3.5 shrink-0 place-items-center rounded-[4px] ring-1", on ? "bg-accent ring-accent" : "ring-separator-strong")}>
                        {on && (
                          <svg width="9" height="9" viewBox="0 0 10 10" aria-hidden>
                            <path d="M2 5.2l2 2L8 3" stroke="var(--c-accent-fg)" strokeWidth="1.6" fill="none" />
                          </svg>
                        )}
                      </span>
                      <span className="min-w-0 flex-1 truncate">{c.name}</span>
                      <span className="shrink-0 text-[11px] text-text-3">{c.camera ?? ""}</span>
                    </button>
                  );
                })}
              </div>
              <div className="flex items-center justify-end gap-2">
                {busy && <Spinner />}
                <Button variant="primary" disabled={picked.size < 2 || busy} onClick={() => void run()}>
                  {method === "audio" ? "Sync by audio" : "Sync by timecode"}
                </Button>
              </div>
            </>
          ) : (
            <>
              <p className="text-[12px] text-text-3">
                {result.method === "audio" ? "Synced by audio" : "Synced by timecode"} · offsets from the start of the reference clip
              </p>
              <Offsets g={result} durations={durations} />
              {result.warnings.length > 0 && (
                <ul className="flex flex-col gap-1 rounded-(--radius-control) bg-control-hover/50 p-2.5 text-[12px] text-text-2">
                  {result.warnings.map((w) => (
                    <li key={w} className="flex items-start gap-1.5">
                      <AlertTriangle size={12} className="mt-0.5 shrink-0 text-[var(--c-score-mid)]" />
                      {w}
                    </li>
                  ))}
                </ul>
              )}
              <div className="flex items-center gap-2">
                <Button variant="plain" className="whitespace-nowrap" onClick={() => setResult(null)}>
                  Change clips…
                </Button>
                <span className="flex-1" />
                <Segmented size="sm" label="Export as" value={format} onChange={setFormat} options={EXPORTS.map((e) => ({ value: e.value, label: e.label, title: e.title }))} />
                <Button className="whitespace-nowrap" onClick={() => void exportIt()}>
                  Export…
                </Button>
                <Button variant="primary" className="whitespace-nowrap" onClick={() => void save()}>
                  {result.id === null ? "Save group" : "Save changes"}
                </Button>
              </div>
            </>
          )}
        </RDialog.Content>
      </RDialog.Portal>
    </RDialog.Root>
  );
}
