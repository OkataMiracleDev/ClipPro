// Offload (SPEC §19, P13.1): copy a card to several drives at once, verify every copy by
// checksum, and leave ASC MHL history next to it. Runs in the background; the sheet can close.

import { CheckCircle2, FolderOpen, HardDrive, Plus, X, XCircle } from "lucide-react";
import { useEffect, useState } from "react";
import { create } from "zustand";
import { api, type OffloadProgress, type OffloadSummary } from "../api";
import { Button, cx, IconButton, Sheet, SheetFooter, Switch } from "../components/ui";
import { pickFolder } from "../lib/dialogs";
import { useApp } from "../store";

interface OffloadState {
  running: boolean;
  progress: OffloadProgress | null;
  summary: OffloadSummary | null;
  error: string | null;
  startedAt: number;
}

export const useOffload = create<OffloadState>(() => ({ running: false, progress: null, summary: null, error: null, startedAt: 0 }));

let listening = false;
/** Subscribes once to backend offload events. */
export function listenOffload() {
  if (listening) return;
  listening = true;
  void api.onOffload((p) => {
    if (p.kind === "started") useOffload.setState({ running: true, progress: p, summary: null, error: null, startedAt: performance.now() });
    else if (p.kind === "progress") useOffload.setState({ progress: p });
    else if (p.kind === "finished") {
      useOffload.setState({ running: false, progress: p, summary: p.summary });
      const s = p.summary;
      const { toast, refreshClips } = useApp.getState();
      if (s) toast(s.failed.length ? `Offload finished with ${s.failed.length} failed cop${s.failed.length === 1 ? "y" : "ies"}` : `Offload verified: ${s.files} files`, s.failed.length ? "error" : undefined);
      if (s?.imported) void refreshClips();
    } else if (p.kind === "cancelled") useOffload.setState({ running: false, error: "Cancelled. Run it again to resume where it stopped." });
    else if (p.kind === "failed") useOffload.setState({ running: false, error: p.message ?? "The offload failed." });
  });
}

export function formatBytes(b: number): string {
  if (b < 1e3) return `${Math.round(b)} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = b;
  let i = -1;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  return `${v >= 100 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

const HASHES = [
  { value: "xxh64", label: "xxHash64 (fast, recommended)" },
  { value: "xxh3", label: "xxHash3" },
  { value: "xxh128", label: "xxHash128" },
  { value: "md5", label: "MD5" },
  { value: "sha1", label: "SHA-1" },
];

function PathRow({ path, onRemove }: { path: string; onRemove?: () => void }) {
  return (
    <div className="flex h-7 items-center gap-2 rounded-md px-2 text-[12.5px] ring-1 ring-separator">
      <HardDrive size={13} className="shrink-0 text-text-3" />
      <span className="min-w-0 flex-1 truncate font-mono text-[11.5px]" title={path}>
        {path}
      </span>
      {onRemove && (
        <IconButton label="Remove" onClick={onRemove}>
          <X size={12} />
        </IconButton>
      )}
    </div>
  );
}

export function OffloadSheet({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const project = useApp((s) => s.project);
  const toast = useApp((s) => s.toast);
  const { running, progress, summary, error, startedAt } = useOffload();
  // `?offload=1|run` (screenshots) starts from a filled-in form; `run` also starts it.
  const demo = new URLSearchParams(location.search).get("offload");
  const [source, setSource] = useState<string | null>(demo ? "F:/A002R2EC" : null);
  const [dests, setDests] = useState<string[]>(demo ? ["E:/Shuttle", "G:/RAID"] : []);
  const [hash, setHash] = useState("xxh64");
  const [addToProject, setAddToProject] = useState(true);
  useEffect(() => {
    listenOffload();
    if (demo === "run") void api.offloadStart("F:/A002R2EC", ["E:/Shuttle", "G:/RAID"], "xxh64", false);
  }, []);

  async function start() {
    if (!source || !dests.length) return;
    try {
      useOffload.setState({ summary: null, error: null });
      await api.offloadStart(source, dests, hash, addToProject && !!project);
    } catch (e) {
      toast(`Couldn’t start the offload: ${e}`, "error");
    }
  }

  const card = source?.split(/[\\/]/).filter(Boolean).pop() ?? "";
  const pct = progress && progress.total > 0 ? progress.done / progress.total : 0;
  const elapsed = (performance.now() - startedAt) / 1000;
  const rate = running && elapsed > 0.5 ? (progress?.done ?? 0) / elapsed : 0;
  const eta = rate > 0 && progress ? (progress.total - progress.done) / rate : 0;
  const row = "flex items-center justify-between gap-3";

  return (
    <Sheet open={open} onOpenChange={onOpenChange} title="Offload a card">
      <div className="flex flex-col gap-3.5">
        {running || summary || error ? (
          <>
            {running && progress && (
              <div className="flex flex-col gap-2">
                <div className="h-1.5 overflow-hidden rounded-full bg-control-hover" role="progressbar" aria-valuenow={Math.round(pct * 100)} aria-valuemin={0} aria-valuemax={100}>
                  <div className="h-full rounded-full bg-accent" style={{ width: `${pct * 100}%` }} />
                </div>
                <div className="flex justify-between text-[12px] text-text-2 tabular">
                  <span>
                    {formatBytes(progress.done)} of {formatBytes(progress.total)} · file {Math.min(progress.index + 1, progress.files)} of {progress.files}
                  </span>
                  <span>{rate > 0 ? `${formatBytes(rate)}/s · ${Math.ceil(eta)} s left` : "Starting…"}</span>
                </div>
                <div className="truncate font-mono text-[11px] text-text-3">{progress.file ?? ""}</div>
                <p className="text-[11.5px] text-text-3">Every copy is read back and checked before it gets its real name. You can close this; the offload keeps going.</p>
              </div>
            )}
            {summary && (
              <div className="flex flex-col gap-2">
                <div className="flex items-center gap-2 text-[13px] font-medium">
                  {summary.failed.length ? <XCircle size={16} className="text-danger" /> : <CheckCircle2 size={16} className="text-(--c-score-hi)" />}
                  {summary.failed.length ? `${summary.failed.length} cop${summary.failed.length === 1 ? "y" : "ies"} failed verification` : "Every copy verified"}
                </div>
                <div className="text-[12px] text-text-2 tabular">
                  {summary.files} files · {formatBytes(summary.bytes)} in {summary.seconds.toFixed(1)} s{summary.seconds > 0 ? ` (${formatBytes(summary.bytes / summary.seconds)}/s)` : ""}
                  {summary.skipped ? ` · ${summary.skipped} already verified` : ""}
                  {summary.imported ? ` · ${summary.imported} added to the project` : ""}
                </div>
                {summary.failed.length > 0 && (
                  <ul className="max-h-28 overflow-y-auto rounded-(--radius-control) bg-control-hover/50 p-2 font-mono text-[11px] text-danger">
                    {summary.failed.map((f) => (
                      <li key={f}>{f}</li>
                    ))}
                  </ul>
                )}
                <div className="text-[11.5px] text-text-3">{summary.failed.length ? "Run the offload again to re-copy only what failed." : "ASC MHL history:"}</div>
                {summary.manifests.map((m) => (
                  <div key={m} className="truncate font-mono text-[11px] text-text-3" title={m}>
                    {m}
                  </div>
                ))}
              </div>
            )}
            {error && <div className="rounded-(--radius-control) bg-control-hover/50 p-2.5 text-[12px] text-text-2">{error}</div>}
            <SheetFooter className="flex justify-end gap-2">
              {running ? (
                <Button onClick={() => void api.offloadCancel()}>Cancel offload</Button>
              ) : (
                <>
                  <Button onClick={() => useOffload.setState({ summary: null, error: null, progress: null })}>New offload</Button>
                  <Button variant="primary" onClick={() => onOpenChange(false)}>
                    Done
                  </Button>
                </>
              )}
            </SheetFooter>
          </>
        ) : (
          <>
            <div className="flex flex-col gap-1.5">
              <div className={row}>
                <span className="text-[12px] text-text-2">Source card or folder</span>
                <Button variant="plain" onClick={() => void pickFolder("Choose a card or folder to offload").then((p) => p && setSource(p))}>
                  <FolderOpen size={13} /> Choose…
                </Button>
              </div>
              {source ? <PathRow path={source} /> : <div className="text-[11.5px] text-text-3">The whole folder is copied, keeping its structure.</div>}
            </div>
            <div className="flex flex-col gap-1.5">
              <div className={row}>
                <span className="text-[12px] text-text-2">Destinations</span>
                <Button
                  variant="plain"
                  onClick={() =>
                    void pickFolder("Choose a destination drive or folder").then((p) => {
                      if (p && !dests.includes(p)) setDests([...dests, p]);
                    })
                  }
                >
                  <Plus size={13} /> Add…
                </Button>
              </div>
              {dests.map((d) => (
                <PathRow key={d} path={card ? `${d.replace(/[\\/]+$/, "")}${d.includes("\\") ? "\\" : "/"}${card}` : d} onRemove={() => setDests(dests.filter((x) => x !== d))} />
              ))}
              {!dests.length && <div className="text-[11.5px] text-text-3">Add one or more drives; all copies are written at the same time.</div>}
            </div>
            <label className={row}>
              <span className="text-[12px] text-text-2">Checksum</span>
              <select aria-label="Checksum" value={hash} onChange={(e) => setHash(e.target.value)} className="h-6 rounded border border-separator-strong bg-control px-1.5 text-[12px]">
                {HASHES.map((h) => (
                  <option key={h.value} value={h.value}>
                    {h.label}
                  </option>
                ))}
              </select>
            </label>
            <label className={cx(row, !project && "opacity-50")}>
              <span className="text-[12px] text-text-2">Add the first destination’s copy to this project</span>
              <Switch label="Add to project" checked={addToProject && !!project} onChange={setAddToProject} />
            </label>
            <p className="text-[11.5px] text-text-3">
              Each destination gets an <span className="font-mono">ascmhl</span> folder with the checksums. Stopping and starting again resumes: verified files are skipped.
            </p>
            <SheetFooter className="flex justify-end gap-2">
              <Button onClick={() => onOpenChange(false)}>Cancel</Button>
              <Button variant="primary" disabled={!source || !dests.length} onClick={() => void start()}>
                Start offload
              </Button>
            </SheetFooter>
          </>
        )}
      </div>
    </Sheet>
  );
}
