// Background proxy and render jobs (P13.2/P13.3): progress in the status bar, a toast when done.

import { X } from "lucide-react";
import { useEffect } from "react";
import { create } from "zustand";
import { api, type JobProgress } from "../api";
import { Spinner } from "../components/ui";
import { useApp } from "../store";

export const useJob = create<{ job: JobProgress | null }>(() => ({ job: null }));

let listening = false;
export function listenJobs() {
  if (listening) return;
  listening = true;
  void api.onJob((p) => {
    const { toast, refreshClips } = useApp.getState();
    const plural = (n: number) => `${n} ${p.job === "render" ? (n === 1 ? "select" : "selects") : n === 1 ? "proxy" : "proxies"}`;
    if (p.kind === "progress") {
      useJob.setState({ job: p });
      return;
    }
    useJob.setState({ job: null });
    if (p.kind === "finished") {
      toast(p.job === "render" ? `Rendered ${plural(p.outputs.length)}` : `Made ${plural(p.outputs.length)}; analysing from ${p.outputs.length === 1 ? "it" : "them"}`);
      if (p.job === "proxy") void refreshClips();
    } else if (p.kind === "cancelled") toast(p.job === "render" ? "Rendering stopped" : "Proxy stopped");
    else toast(`${p.job === "render" ? "Render" : "Proxy"} failed: ${p.message ?? "unknown error"}`, "error");
  });
}

/** "Rendering 3 of 12 · 45%" with a stop button, while a job runs. */
export function JobStatus() {
  const job = useJob((s) => s.job);
  useEffect(() => listenJobs(), []);
  if (!job) return null;
  const pct = Math.round(((job.index + job.fraction) / Math.max(1, job.count)) * 100);
  return (
    <span className="flex items-center gap-1.5 text-text-2 tabular">
      <Spinner size={10} />
      {job.job === "render" ? "Rendering" : "Making proxy"} {Math.min(job.index + 1, job.count)} of {job.count} · {pct}%
      {job.name ? <span className="max-w-40 truncate text-text-3">· {job.name}</span> : null}
      <button aria-label="Stop" title="Stop" className="rounded p-0.5 hover:bg-control-hover hover:text-text" onClick={() => void api.jobCancel()}>
        <X size={11} />
      </button>
    </span>
  );
}
