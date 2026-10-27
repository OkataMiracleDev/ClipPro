import { ArrowDownUp, Check, FolderOpen, ListFilter, PanelRight, Pause, Play, Plus, Share } from "lucide-react";
import { useEffect, useState } from "react";
import { ExportSheet } from "./ExportSheet";
import { isTauri, type SortOrder } from "../api";
import { cx, IconButton, Popover, PopoverClose, Spinner } from "../components/ui";
import { useApp } from "../store";
import { pickMedia, pickProjectToOpen } from "../lib/dialogs";

async function win() {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow();
}

function TrafficLights() {
  const btn = "no-drag group grid h-3 w-3 place-items-center rounded-full ring-1 ring-black/15";
  return (
    <div className="flex items-center gap-2 pl-4" role="group" aria-label="Window controls">
      <button aria-label="Close" className={cx(btn, "bg-(--c-tl-close)")} onClick={async () => isTauri && (await win()).close()}>
        <svg width="6" height="6" viewBox="0 0 6 6" className="opacity-0 group-hover:opacity-60" aria-hidden>
          <path d="M1 1l4 4M5 1L1 5" stroke="black" strokeWidth="1.2" />
        </svg>
      </button>
      <button aria-label="Minimise" className={cx(btn, "bg-(--c-tl-min)")} onClick={async () => isTauri && (await win()).minimize()}>
        <svg width="6" height="6" viewBox="0 0 6 6" className="opacity-0 group-hover:opacity-60" aria-hidden>
          <path d="M0.5 3h5" stroke="black" strokeWidth="1.2" />
        </svg>
      </button>
      <button aria-label="Maximise" className={cx(btn, "bg-(--c-tl-max)")} onClick={async () => isTauri && (await win()).toggleMaximize()}>
        <svg width="6" height="6" viewBox="0 0 6 6" className="opacity-0 group-hover:opacity-60" aria-hidden>
          <path d="M1 1.5v3.5h3.5M5 4.5V1H1.5" stroke="black" strokeWidth="1.1" fill="none" />
        </svg>
      </button>
    </div>
  );
}

const SORTS: { value: SortOrder; label: string }[] = [
  { value: "timeline", label: "Timeline order" },
  { value: "score_desc", label: "Score: high to low" },
  { value: "score_asc", label: "Score: low to high" },
  { value: "duration_desc", label: "Duration: long to short" },
  { value: "duration_asc", label: "Duration: short to long" },
];

function AnalysisStatus() {
  const { analysis, clips, togglePause } = useApp();
  const total = clips.length;
  const done = clips.filter((c) => c.state === "done").length;
  const running = clips.filter((c) => c.state === "running" || c.state === "queued").length;
  if (!analysis.active || running === 0) {
    return total ? <span className="text-[12px] text-text-3">{done === total ? `${total} clip${total === 1 ? "" : "s"} analysed` : `${done} of ${total} analysed`}</span> : null;
  }
  // Finished clips count fully; running jobs add their partial progress.
  const partial = Object.values(analysis.jobs).filter((f) => f < 1).reduce((a, b) => a + b, 0);
  const pct = Math.round((100 * (done + partial)) / Math.max(1, total));
  return (
    <span className="no-drag flex items-center gap-2 text-[12px] text-text-2">
      {analysis.paused ? <span className="text-text-3">Paused</span> : <Spinner size={12} />}
      <span className="tabular">
        Analysing · {pct}%
      </span>
      <IconButton label={analysis.paused ? "Resume analysis" : "Pause analysis"} onClick={() => void togglePause()}>
        {analysis.paused ? <Play size={14} /> : <Pause size={14} />}
      </IconButton>
    </span>
  );
}

export function TitleBar() {
  const { project, importPaths, openProject, selectsOnly, toggleSelectsOnly, sort, setSort, inspectorOpen, toggleInspector } = useApp();
  const [exportOpen, setExportOpen] = useState(false);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key.toLowerCase() === "e" && project) {
        e.preventDefault();
        setExportOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [project]);
  return (
    <header
      data-tauri-drag-region
      className="drag flex h-(--h-titlebar) shrink-0 items-center gap-2 border-b border-separator"
      onDoubleClick={async (e) => {
        if (e.target === e.currentTarget && isTauri) (await win()).toggleMaximize();
      }}
    >
      <TrafficLights />
      <div className="no-drag ml-3 flex items-center gap-0.5">
        <IconButton label="Open project…" onClick={async () => {
          const p = await pickProjectToOpen();
          if (p) await openProject(p);
        }}>
          <FolderOpen size={16} />
        </IconButton>
        <IconButton label="Add media…" disabled={!project} onClick={async () => {
          const paths = await pickMedia();
          if (paths.length) await importPaths(paths);
        }}>
          <Plus size={17} />
        </IconButton>
      </div>
      <div data-tauri-drag-region className="pointer-events-none flex flex-1 justify-center">
        <span className="truncate text-[13px] font-semibold text-text">{project?.name ?? "ClipPro"}</span>
      </div>
      <AnalysisStatus />
      <div className="no-drag flex items-center gap-0.5 pr-3">
        <IconButton label="Selects only" active={selectsOnly} onClick={toggleSelectsOnly} disabled={!project}>
          <ListFilter size={16} />
        </IconButton>
        <Popover
          align="end"
          trigger={
            <span>
              <IconButton label="Sort" disabled={!project}>
                <ArrowDownUp size={16} />
              </IconButton>
            </span>
          }
        >
          <div className="w-52 py-1">
            {SORTS.map((s) => (
              <PopoverClose asChild key={s.value}>
                <button onClick={() => setSort(s.value)} className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] hover:bg-accent hover:text-accent-fg">
                  <span className="w-3.5">{sort === s.value && <Check size={13} />}</span>
                  {s.label}
                </button>
              </PopoverClose>
            ))}
          </div>
        </Popover>
        <IconButton label="Export selects… (Ctrl+E)" disabled={!project} onClick={() => setExportOpen(true)}>
          <Share size={16} />
        </IconButton>
        <IconButton label={inspectorOpen ? "Hide inspector" : "Show inspector"} active={inspectorOpen} onClick={toggleInspector}>
          <PanelRight size={16} />
        </IconButton>
      </div>
      <ExportSheet open={exportOpen} onOpenChange={setExportOpen} />
    </header>
  );
}
