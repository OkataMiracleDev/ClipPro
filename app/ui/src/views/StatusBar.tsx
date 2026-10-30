// Status bar (SPEC P5.3): analysis progress and ETA, decode path, re-scoring indicator, and the
// selects summary.

import { Cpu, Gauge } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { Spinner } from "../components/ui";
import { formatDuration } from "../lib/typed";
import { useApp } from "../store";
import { JobStatus } from "./Jobs";

let info: Promise<{ hwDecode: boolean; version: string }> | null = null;

/** Remaining time from the progress rate over the last minute or so. */
function useEta(fraction: number, active: boolean): number | null {
  const hist = useRef<{ t: number; f: number }[]>([]);
  const [eta, setEta] = useState<number | null>(null);
  useEffect(() => {
    if (!active) {
      hist.current = [];
      setEta(null);
      return;
    }
    const now = performance.now();
    const h = hist.current;
    if (!h.length || h[h.length - 1]!.f !== fraction) h.push({ t: now, f: fraction });
    while (h.length > 2 && now - h[0]!.t > 60_000) h.shift();
    const a = h[0]!;
    const rate = (fraction - a.f) / Math.max(1, now - a.t); // fraction per ms
    setEta(h.length >= 2 && rate > 0 && now - a.t > 3000 ? (1 - fraction) / rate / 1000 : null);
  }, [fraction, active]);
  return eta;
}

export function StatusBar() {
  const { analysis, clips, scored, scoring } = useApp();
  const [sys, setSys] = useState<{ hwDecode: boolean; version: string } | null>(null);
  useEffect(() => {
    info ??= api.systemInfo();
    info.then(setSys, () => setSys(null));
  }, []);
  const total = clips.length;
  const done = clips.filter((c) => c.state === "done").length;
  const failed = clips.filter((c) => c.state === "failed").length;
  const working = analysis.active && clips.some((c) => c.state === "running" || c.state === "queued");
  const partial = Object.values(analysis.jobs).filter((f) => f < 1).reduce((a, b) => a + b, 0);
  const fraction = total ? Math.min(1, (done + failed + partial) / total) : 0;
  const eta = useEta(fraction, working && !analysis.paused);
  const s = scored?.summary;

  return (
    <footer className="flex h-6 shrink-0 items-center gap-4 border-t border-separator bg-window px-3 text-[11px] text-text-3" aria-label="Status">
      <span className="flex items-center gap-1.5 tabular">
        {working && !analysis.paused && <Spinner size={10} />}
        {total === 0
          ? "No clips"
          : working
            ? `${analysis.paused ? "Paused" : "Analysing"} · ${done} of ${total} done · ${Math.round(fraction * 100)}%${eta !== null && !analysis.paused ? ` · about ${formatDuration(Math.max(1, Math.round(eta)))} left` : ""}`
            : `${done} of ${total} analysed${failed ? ` · ${failed} failed` : ""}`}
      </span>
      <JobStatus />
      {scoring && (
        <span className="flex items-center gap-1">
          <Spinner size={10} /> Scoring…
        </span>
      )}
      <span className="flex-1" />
      {s && total > 0 && (
        <span className="tabular">
          {s.count} select{s.count === 1 ? "" : "s"} · {formatDuration(Math.round(s.selectsUs / 1e6))} · {s.percent.toFixed(1)}% of footage
        </span>
      )}
      {sys && (
        <span className="flex items-center gap-1" title={sys.hwDecode ? "Video decodes on the GPU (D3D11VA) when the codec allows" : "No hardware decoder found: video decodes on the CPU"}>
          {sys.hwDecode ? <Gauge size={11} /> : <Cpu size={11} />}
          {sys.hwDecode ? "GPU decode" : "CPU decode"}
        </span>
      )}
    </footer>
  );
}
