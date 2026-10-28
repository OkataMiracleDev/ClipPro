// Undo/redo buttons, named snapshots and A/B tuning compare (SPEC TUN-10, P9.6).

import { Camera, Redo2, Trash2, Undo2 } from "lucide-react";
import { useState } from "react";
import { cx, Popover, PopoverClose } from "../components/ui";
import { useApp } from "../store";

export function UndoRedo() {
  const { canUndo, canRedo, undo, redo } = useApp();
  const btn = "rounded p-1 text-text-3 hover:bg-control-hover hover:text-text disabled:opacity-30 disabled:hover:bg-transparent";
  return (
    <div className="flex items-center">
      <button className={btn} disabled={!canUndo} onClick={undo} aria-label="Undo (Ctrl+Z)" title="Undo (Ctrl+Z)">
        <Undo2 size={14} />
      </button>
      <button className={btn} disabled={!canRedo} onClick={redo} aria-label="Redo (Ctrl+Shift+Z)" title="Redo (Ctrl+Shift+Z)">
        <Redo2 size={14} />
      </button>
    </div>
  );
}

export function SnapshotsMenu() {
  const { snapshots, saveSnapshot, applySnapshot, deleteSnapshot, ab, toggleAB, endAB, saveTemplate } = useApp();
  const [name, setName] = useState("");
  const [tpl, setTpl] = useState("");
  return (
    <div className="flex items-center gap-1">
      <button
        onClick={toggleAB}
        className={cx("rounded px-1.5 py-0.5 font-mono text-[10.5px] ring-1", ab ? "bg-accent text-accent-fg ring-accent" : "text-text-3 ring-separator-strong hover:text-text")}
        title={ab ? `Showing ${ab.showing.toUpperCase()} — click to switch` : "Compare A/B: keep this tuning as A and try changes as B"}
        aria-label="Compare A/B settings"
      >
        {ab ? (ab.showing === "a" ? "A" : "B") : "A/B"}
      </button>
      {ab && (
        <button className="text-[10.5px] text-text-3 hover:text-text" onClick={endAB} title="Stop comparing (keeps what's showing)">
          done
        </button>
      )}
      <Popover
        align="end"
        trigger={
          <button className="rounded p-1 text-text-3 hover:bg-control-hover hover:text-text" aria-label="Snapshots" title="Snapshots">
            <Camera size={14} />
          </button>
        }
      >
        <div className="flex w-64 flex-col gap-1 p-1">
          <div className="section-header px-1">Snapshots</div>
          <form
            className="flex gap-1 px-1"
            onSubmit={(e) => {
              e.preventDefault();
              if (name.trim()) void saveSnapshot(name).then(() => setName(""));
            }}
          >
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Name this tuning…"
              className="h-6 min-w-0 flex-1 rounded border border-separator-strong bg-control px-1.5 text-[12px]"
            />
            <button type="submit" disabled={!name.trim()} className="rounded bg-accent px-2 text-[11px] text-accent-fg disabled:opacity-40">
              Save
            </button>
          </form>
          {snapshots.length === 0 ? (
            <div className="px-1 py-1 text-[11px] text-text-3">Save the current tuning to come back to it later.</div>
          ) : (
            [...snapshots].reverse().map((s) => (
              <div key={s.id} className="group flex items-center gap-1 rounded px-1 hover:bg-control-hover">
                <PopoverClose asChild>
                  <button className="min-w-0 flex-1 truncate py-1 text-left text-[12px]" onClick={() => applySnapshot(s.id)} title={`Saved ${new Date(s.created.replace(" ", "T") + "Z").toLocaleString()}`}>
                    {s.name}
                  </button>
                </PopoverClose>
                <button className="rounded p-0.5 text-text-3 opacity-0 group-hover:opacity-100 hover:text-danger" aria-label={`Delete snapshot ${s.name}`} onClick={() => void deleteSnapshot(s.id)}>
                  <Trash2 size={12} />
                </button>
              </div>
            ))
          )}
          <div className="my-1 h-px bg-separator" />
          <div className="section-header px-1">Project template</div>
          <form
            className="flex gap-1 px-1"
            onSubmit={(e) => {
              e.preventDefault();
              if (tpl.trim()) void saveTemplate(tpl).then(() => setTpl(""));
            }}
          >
            <input
              value={tpl}
              onChange={(e) => setTpl(e.target.value)}
              placeholder="Template name…"
              title="Saves this tuning and the watch folders as a starting point for new projects"
              className="h-6 min-w-0 flex-1 rounded border border-separator-strong bg-control px-1.5 text-[12px]"
            />
            <button type="submit" disabled={!tpl.trim()} className="rounded px-2 text-[11px] ring-1 ring-separator-strong disabled:opacity-40">
              Save
            </button>
          </form>
        </div>
      </Popover>
    </div>
  );
}
