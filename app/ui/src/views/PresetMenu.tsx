// Preset picker and custom-preset management (SPEC P9.4): built-in and custom presets, save /
// save as / rename / duplicate / delete / import / export, and the "modified" diff with revert.

import { Check, ChevronDown, Download, FilePlus2, Pencil, RotateCcw, Save, Trash2, Upload } from "lucide-react";
import { useState } from "react";
import { api } from "../api";
import { Button, cx, Popover, PopoverClose, Sheet } from "../components/ui";
import { pickPresetExport, pickPresetFile } from "../lib/dialogs";
import { diffSettings } from "../lib/diff";
import { useApp } from "../store";
import { useSubMetrics } from "./Weighting";

type Mode = { kind: "save-as" } | { kind: "rename"; name: string } | { kind: "duplicate"; name: string } | null;

function NameSheet({ mode, onClose }: { mode: Mode; onClose: () => void }) {
  const { savePreset, renamePreset, presets } = useApp();
  const initial = mode?.kind === "rename" ? mode.name : mode?.kind === "duplicate" ? `${mode.name} copy` : "";
  const [name, setName] = useState(initial);
  const [description, setDescription] = useState("");
  const [busy, setBusy] = useState(false);
  const taken = presets.some((p) => p.name === name.trim() && !(mode?.kind === "rename" && p.name === mode.name));
  const title = mode?.kind === "rename" ? "Rename preset" : mode?.kind === "duplicate" ? "Duplicate preset" : "Save as new preset";
  async function submit() {
    const n = name.trim();
    if (!n || taken || !mode) return;
    setBusy(true);
    let ok = false;
    if (mode.kind === "rename") ok = await renamePreset(mode.name, n);
    else if (mode.kind === "duplicate") {
      const st = useApp.getState();
      const keep = st.settings;
      // Duplicate saves the chosen preset's own settings, not the current tuning.
      const s = await api.presetSettings(mode.name);
      useApp.setState({ settings: s });
      ok = await savePreset(n, description, false);
      if (!ok) useApp.setState({ settings: keep });
    } else ok = await savePreset(n, description, false);
    setBusy(false);
    if (ok) onClose();
  }
  return (
    <Sheet open={mode !== null} onOpenChange={(o) => !o && onClose()} title={title}>
      <form
        className="flex flex-col gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <label className="flex flex-col gap-1">
          <span className="text-[12px] text-text-2">Name</span>
          <input autoFocus value={name} onChange={(e) => setName(e.target.value)} className="h-7 rounded border border-separator-strong bg-control px-2 text-[13px]" />
          {taken && <span className="text-[11px] text-danger">That name is already used.</span>}
        </label>
        {mode?.kind !== "rename" && (
          <label className="flex flex-col gap-1">
            <span className="text-[12px] text-text-2">Description (optional)</span>
            <input value={description} onChange={(e) => setDescription(e.target.value)} className="h-7 rounded border border-separator-strong bg-control px-2 text-[13px]" />
          </label>
        )}
        <div className="flex justify-end gap-2 pt-1">
          <Button type="button" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" type="submit" disabled={busy || !name.trim() || taken}>
            {mode?.kind === "rename" ? "Rename" : "Save"}
          </Button>
        </div>
      </form>
    </Sheet>
  );
}

export function PresetPicker() {
  const { presets, presetName, modified, setPreset, savePreset, deletePreset, importPreset, toast } = useApp();
  const [mode, setMode] = useState<Mode>(null);
  const current = presets.find((p) => p.name === presetName);
  const custom = current && !current.builtin;
  const row = "flex w-full items-start gap-2 rounded-md px-2 py-1.5 text-left hover:bg-accent hover:text-accent-fg";
  const action = "flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-[12px] hover:bg-accent hover:text-accent-fg disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-inherit";
  const list = (builtin: boolean) =>
    presets
      .filter((p) => p.builtin === builtin)
      .map((p) => (
        <PopoverClose asChild key={p.name}>
          <button onClick={() => void setPreset(p.name)} className={row}>
            <span className="mt-0.5 w-3.5">{p.name === presetName && <Check size={13} />}</span>
            <span>
              <span className="block text-[13px]">{p.name}</span>
              {p.description && <span className="block text-[11px] opacity-70">{p.description}</span>}
            </span>
          </button>
        </PopoverClose>
      ));
  const mine = presets.filter((p) => !p.builtin);

  async function exportPreset() {
    const s = useApp.getState().settings;
    if (!s) return;
    const path = await pickPresetExport(presetName);
    if (!path) return;
    try {
      await api.presetExport(presetName, current?.description ?? "", s, path);
      toast(`Exported ${path.split(/[\\/]/).pop()}`);
    } catch (e) {
      toast(`Export failed: ${e}`, "error");
    }
  }

  return (
    <>
      <Popover
        trigger={
          <button className="flex h-7 w-full items-center justify-between rounded-(--radius-control) border border-separator-strong bg-control px-2.5 text-[13px] hover:bg-control-hover">
            <span className="flex items-center gap-1.5 truncate">
              {presetName}
              {modified && <span className="h-1.5 w-1.5 rounded-full bg-accent" title="Modified" />}
            </span>
            <ChevronDown size={14} className="text-text-3" />
          </button>
        }
      >
        <div className="max-h-[70vh] w-[290px] overflow-y-auto py-1">
          {mine.length > 0 && (
            <>
              <div className="section-header px-2 pt-1 pb-0.5">My presets</div>
              {list(false)}
              <div className="my-1 h-px bg-separator" />
              <div className="section-header px-2 pt-1 pb-0.5">Built in</div>
            </>
          )}
          {list(true)}
          <div className="my-1 h-px bg-separator" />
          <PopoverClose asChild>
            <button className={action} disabled={!custom || !modified} onClick={() => void savePreset(presetName, current?.description ?? "", true)}>
              <Save size={13} /> Save changes to “{presetName}”
            </button>
          </PopoverClose>
          <PopoverClose asChild>
            <button className={action} onClick={() => setMode({ kind: "save-as" })}>
              <FilePlus2 size={13} /> Save as new preset…
            </button>
          </PopoverClose>
          <PopoverClose asChild>
            <button className={action} onClick={() => setMode({ kind: "duplicate", name: presetName })}>
              <FilePlus2 size={13} /> Duplicate “{presetName}”…
            </button>
          </PopoverClose>
          <PopoverClose asChild>
            <button className={action} disabled={!custom} onClick={() => setMode({ kind: "rename", name: presetName })}>
              <Pencil size={13} /> Rename…
            </button>
          </PopoverClose>
          <PopoverClose asChild>
            <button className={cx(action, "hover:bg-danger!")} disabled={!custom} onClick={() => void deletePreset(presetName)}>
              <Trash2 size={13} /> Delete “{presetName}”
            </button>
          </PopoverClose>
          <div className="my-1 h-px bg-separator" />
          <PopoverClose asChild>
            <button
              className={action}
              onClick={() =>
                void pickPresetFile().then((p) => {
                  if (p) void importPreset(p);
                })
              }
            >
              <Upload size={13} /> Import preset…
            </button>
          </PopoverClose>
          <PopoverClose asChild>
            <button className={action} onClick={() => void exportPreset()}>
              <Download size={13} /> Export current tuning…
            </button>
          </PopoverClose>
        </div>
      </Popover>
      {mode && <NameSheet key={JSON.stringify(mode)} mode={mode} onClose={() => setMode(null)} />}
    </>
  );
}

/** The modified indicator: what changed from the preset, with revert. */
export function PresetChanges() {
  const { settings, baseSettings, modified, presetName, revertPreset } = useApp();
  const infos = useSubMetrics();
  if (!modified || !settings || !baseSettings) return null;
  const changes = diffSettings(baseSettings, settings, infos);
  return (
    <Popover
      align="end"
      trigger={
        <button className="rounded px-1.5 py-1 text-[11px] text-accent hover:bg-control-hover" title="What changed from the preset">
          {changes.length} change{changes.length === 1 ? "" : "s"}
        </button>
      }
    >
      <div className="w-[300px] p-1">
        <div className="section-header px-1 pb-1">Changed from {presetName}</div>
        <div className="flex max-h-64 flex-col gap-0.5 overflow-y-auto">
          {changes.length === 0 ? (
            <div className="px-1 text-[12px] text-text-3">Nothing — the tuning matches the preset.</div>
          ) : (
            changes.map((c) => (
              <div key={c.what} className="grid grid-cols-[1fr_auto] gap-2 px-1 text-[12px]">
                <span className="truncate text-text-2">{c.what}</span>
                <span className="font-mono text-[11px] tabular">
                  <span className="text-text-3 line-through">{c.from}</span> {c.to}
                </span>
              </div>
            ))
          )}
        </div>
        <PopoverClose asChild>
          <button className="mt-2 flex w-full items-center gap-2 rounded-md px-2 py-1 text-[12px] hover:bg-accent hover:text-accent-fg" onClick={revertPreset}>
            <RotateCcw size={13} /> Revert to {presetName}
          </button>
        </PopoverClose>
      </div>
    </Popover>
  );
}
