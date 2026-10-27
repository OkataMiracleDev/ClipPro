// Source list + clip table (SPEC §14.1, P5.3).

import { AlertCircle, CircleDashed, Film, ListChecks } from "lucide-react";
import { useMemo } from "react";
import type { Clip } from "../api";
import { cx, scoreColor, Spinner } from "../components/ui";
import { formatClock } from "../lib/typed";
import { useApp } from "../store";

export function Sidebar() {
  const { clips, scored, selectsOnly, toggleSelectsOnly } = useApp();
  const item = (active: boolean) =>
    cx("flex h-7 w-full items-center gap-2 rounded-md px-2 text-left text-[13px]", active ? "bg-accent-soft text-text" : "text-text-2 hover:bg-control-hover");
  return (
    <nav className="flex shrink-0 flex-col gap-4 px-2 py-3" aria-label="Library">
      <div>
        <div className="section-header mb-1 px-2">Library</div>
        <button className={item(!selectsOnly)} onClick={() => selectsOnly && toggleSelectsOnly()}>
          <Film size={14} /> <span className="flex-1">All clips</span>
          <span className="font-mono text-[11px] text-text-3 tabular">{clips.length}</span>
        </button>
        <button className={item(selectsOnly)} onClick={() => !selectsOnly && toggleSelectsOnly()}>
          <ListChecks size={14} /> <span className="flex-1">Selects</span>
          <span className="font-mono text-[11px] text-text-3 tabular">{scored?.summary.count ?? 0}</span>
        </button>
      </div>
    </nav>
  );
}

function StateIcon({ c }: { c: Clip }) {
  if (c.state === "failed") return <AlertCircle size={13} className="text-danger" aria-label="Failed" />;
  if (c.state === "running") return <Spinner size={12} />;
  if (c.state === "queued" || c.state === "paused") return <CircleDashed size={13} className="text-text-3" aria-label="Waiting" />;
  return null;
}

export function ClipList() {
  const { clips, scored, selectedClip, selectClip } = useApp();
  const stats = useMemo(() => {
    const m = new Map<number, { selects: number; best: number; duration: number }>();
    for (const c of scored?.clips ?? []) {
      const best = c.segments.reduce((a, s) => Math.max(a, s.mean), -Infinity);
      m.set(c.clip, { selects: c.segments.length, best, duration: c.durationUs });
    }
    return m;
  }, [scored]);
  if (!clips.length) return null;
  return (
    <div className="h-full overflow-y-auto px-2 py-1" role="listbox" aria-label="Clips">
      {clips.map((c) => {
        const st = stats.get(c.clip);
        const active = c.clip === selectedClip;
        const hasBest = st && Number.isFinite(st.best);
        return (
          <div
            key={c.clip}
            role="option"
            aria-selected={active}
            onClick={() => selectClip(c.clip)}
            title={c.error ?? c.path}
            className={cx("flex cursor-default items-center gap-2 rounded-md px-2 py-1.5", active ? "bg-accent text-accent-fg" : "hover:bg-control-hover")}
          >
            <span
              className="h-7 w-1 shrink-0 rounded-full"
              style={{ background: hasBest ? scoreColor(st!.best) : "var(--c-separator-strong)" }}
              aria-hidden
            />
            <span className="min-w-0 flex-1">
              <span className="flex items-center gap-1.5">
                <span className="truncate text-[12px] font-medium">{c.name}</span>
                <StateIcon c={c} />
              </span>
              <span className={cx("block font-mono text-[10.5px] tabular", active ? "opacity-80" : "text-text-3")}>
                {st ? formatClock(st.duration) : c.state === "failed" ? "Failed" : "Not analysed yet"}
                {st ? ` · ${st.selects} select${st.selects === 1 ? "" : "s"}` : ""}
                {hasBest ? ` · best ${Math.round(st!.best * 100)}` : ""}
              </span>
            </span>
          </div>
        );
      })}
    </div>
  );
}
