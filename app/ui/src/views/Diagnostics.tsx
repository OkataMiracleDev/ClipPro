// Diagnostics (SPEC OTH-5, P14.5): the recent log, a report to attach to bug reports (system,
// settings without paths, log and crash reports; never media), and the log folder.

import { FolderOpen, RefreshCw } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { Button, cx, IconButton, Segmented, Sheet } from "../components/ui";
import { pickDiagnosticsPath } from "../lib/dialogs";
import { useApp } from "../store";

type Level = "all" | "warn";

export function levelOf(line: string): "INFO" | "WARN" | "PANIC" | "ERROR" | null {
  const m = /^\S+ \S+ (INFO|WARN|ERROR|PANIC)\b/.exec(line);
  return (m?.[1] as "INFO" | "WARN" | "PANIC" | "ERROR" | undefined) ?? null;
}

export function Diagnostics({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const toast = useApp((s) => s.toast);
  const [lines, setLines] = useState<string[]>([]);
  const [level, setLevel] = useState<Level>("all");
  const load = () => void api.diagnosticsLog(500).then(setLines, () => setLines([]));
  useEffect(() => {
    if (open) load();
  }, [open]);
  const shown = useMemo(() => (level === "all" ? lines : lines.filter((l) => levelOf(l) !== "INFO")), [lines, level]);

  async function save() {
    const p = await pickDiagnosticsPath();
    if (!p) return;
    try {
      await api.diagnosticsSave(p);
      toast(`Saved ${p.split(/[\\/]/).pop()}`);
    } catch (e) {
      toast(`Couldn’t save the report: ${e}`, "error");
    }
  }

  return (
    <Sheet open={open} onOpenChange={onOpenChange} title="Diagnostics">
      <div className="flex flex-col gap-3">
        <div className="flex items-center gap-2">
          <Segmented
            size="sm"
            label="Show"
            value={level}
            onChange={setLevel}
            options={[
              { value: "all", label: "Everything" },
              { value: "warn", label: "Problems" },
            ]}
          />
          <span className="flex-1" />
          <IconButton label="Reload" onClick={load}>
            <RefreshCw size={13} />
          </IconButton>
          <IconButton label="Open the log folder" onClick={() => void api.diagnosticsOpenFolder().catch((e) => toast(`${e}`, "error"))}>
            <FolderOpen size={13} />
          </IconButton>
        </div>
        <div className="h-64 overflow-auto rounded-(--radius-control) bg-control-hover/50 p-2 font-mono text-[10.5px] leading-relaxed ring-1 ring-separator ring-inset" role="log" aria-label="Log">
          {shown.length ? (
            shown.map((l, i) => {
              const lv = levelOf(l);
              return (
                <div key={i} className={cx("whitespace-pre-wrap", lv === "WARN" && "text-(--c-warning)", (lv === "ERROR" || lv === "PANIC") && "text-danger")}>
                  {l}
                </div>
              );
            })
          ) : (
            <div className="p-2 text-text-3">{level === "warn" ? "No problems logged." : "The log is empty."}</div>
          )}
        </div>
        <p className="text-[11.5px] text-text-3">
          The report has your system details, settings, the recent log and any crash reports. File paths are removed and no media is included.
        </p>
        <div className="flex justify-end gap-2 border-t border-separator pt-3">
          <Button onClick={() => onOpenChange(false)}>Close</Button>
          <Button variant="primary" onClick={() => void save()}>
            Save report…
          </Button>
        </div>
      </div>
    </Sheet>
  );
}
