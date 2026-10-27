// Export sheet (SPEC EXP-1..8, P7.7): format, layout, handles, gaps, markers.

import { useState } from "react";
import { api } from "../api";
import { Button, Segmented, Sheet, Switch } from "../components/ui";
import { pickExportPath } from "../lib/dialogs";
import { useApp } from "../store";

interface FormatDef {
  value: string;
  label: string;
  hint: string;
  ext: string;
  filter: string;
  timeline: boolean;
}

const FORMATS: FormatDef[] = [
  { value: "xmeml", label: "Premiere Pro / Resolve", hint: "FCP7 XML", ext: "xml", filter: "FCP7 XML", timeline: true },
  { value: "fcpxml", label: "Final Cut Pro", hint: "FCPXML 1.10", ext: "fcpxml", filter: "FCPXML", timeline: true },
  { value: "otio", label: "OpenTimelineIO", hint: "Resolve 18.5+, OTIO tools", ext: "otio", filter: "OpenTimelineIO", timeline: true },
  { value: "edl", label: "EDL", hint: "CMX3600 with locators", ext: "edl", filter: "EDL", timeline: true },
  { value: "ale", label: "Avid Media Composer", hint: "ALE subclips", ext: "ale", filter: "Avid Log Exchange", timeline: false },
  { value: "avid-markers", label: "Avid markers", hint: "Marker list for Import Markers", ext: "txt", filter: "Avid markers", timeline: false },
  { value: "csv", label: "CSV report", hint: "One row per select", ext: "csv", filter: "CSV", timeline: false },
  { value: "json", label: "JSON report", hint: "For scripts and pipelines", ext: "json", filter: "JSON", timeline: false },
];

export function ExportSheet({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const { settings, sort, project, scored, toast } = useApp();
  const [format, setFormat] = useState("xmeml");
  const [layout, setLayout] = useState<"selects" | "with_rejects">("selects");
  const [handles, setHandles] = useState(true);
  const [gap, setGap] = useState("0");
  const [audio, setAudio] = useState(true);
  const [markers, setMarkers] = useState(true);
  const [markerName, setMarkerName] = useState("Select {index} · {score}");
  const [busy, setBusy] = useState(false);
  const def = FORMATS.find((f) => f.value === format) ?? FORMATS[0]!;
  const count = scored?.summary.count ?? 0;

  async function run() {
    if (!settings) return;
    const base = `${(project?.name ?? "ClipPro").replace(/[\\/:*?"<>|]/g, "-")} selects`;
    const path = await pickExportPath(base, def.ext, def.filter);
    if (!path) return;
    setBusy(true);
    try {
      const r = await api.exportSelects(settings, sort, {
        format,
        path,
        name: base,
        layout,
        handles,
        gap_s: Math.max(0, Number(gap) || 0),
        with_audio: audio,
        markers,
        marker_name: markerName,
        marker_comment: "{flags}",
      });
      toast(`Exported ${r.selects} select${r.selects === 1 ? "" : "s"} to ${r.path.split(/[\\/]/).pop()}`);
      onOpenChange(false);
    } catch (e) {
      toast(`Export failed: ${e}`, "error");
    } finally {
      setBusy(false);
    }
  }

  const row = "flex items-center justify-between gap-3";
  return (
    <Sheet open={open} onOpenChange={onOpenChange} title="Export selects">
      <div className="flex flex-col gap-4">
        <div className="grid grid-cols-2 gap-1.5" role="radiogroup" aria-label="Format">
          {FORMATS.map((f) => (
            <button
              key={f.value}
              role="radio"
              aria-checked={f.value === format}
              onClick={() => setFormat(f.value)}
              className={`rounded-(--radius-control) px-2.5 py-2 text-left ring-1 ${f.value === format ? "bg-accent-soft ring-accent" : "ring-separator-strong hover:bg-control-hover"}`}
            >
              <span className="block text-[13px] font-medium">{f.label}</span>
              <span className="block text-[11px] text-text-3">{f.hint}</span>
            </button>
          ))}
        </div>
        {def.timeline && (
          <div className={row}>
            <span className="text-[12px] text-text-2">Layout</span>
            <Segmented
              label="Layout"
              value={layout}
              onChange={setLayout}
              options={[
                { value: "selects", label: "Selects only" },
                { value: "with_rejects", label: "With rejects on V2" },
              ]}
            />
          </div>
        )}
        <label className={row}>
          <span className="text-[12px] text-text-2">Include handles</span>
          <Switch label="Include handles" checked={handles} onChange={setHandles} />
        </label>
        {def.timeline && layout === "selects" && (
          <label className={row}>
            <span className="text-[12px] text-text-2">Gap between selects (seconds)</span>
            <input
              id="export-gap"
              value={gap}
              onChange={(e) => setGap(e.target.value)}
              inputMode="decimal"
              className="h-6 w-16 rounded border border-separator-strong bg-control px-1.5 text-right font-mono text-[12px]"
            />
          </label>
        )}
        {def.timeline && (
          <label className={row}>
            <span className="text-[12px] text-text-2">Include audio</span>
            <Switch label="Include audio" checked={audio} onChange={setAudio} />
          </label>
        )}
        <label className={row}>
          <span className="text-[12px] text-text-2">Markers</span>
          <Switch label="Markers" checked={markers} onChange={setMarkers} />
        </label>
        {markers && (
          <label className="flex flex-col gap-1">
            <span className="text-[12px] text-text-2">Marker name</span>
            <input
              id="export-marker-name"
              value={markerName}
              onChange={(e) => setMarkerName(e.target.value)}
              className="h-7 rounded border border-separator-strong bg-control px-2 text-[12px]"
            />
            <span className="text-[11px] text-text-3">Tokens: {"{clip} {index} {score} {flags} {duration} {tc_in} {tc_out}"}. The comment lists flags.</span>
          </label>
        )}
        <div className="flex items-center justify-between border-t border-separator pt-3">
          <span className="text-[12px] text-text-3">{count} select{count === 1 ? "" : "s"} with the current settings</span>
          <div className="flex gap-2">
            <Button onClick={() => onOpenChange(false)}>Cancel</Button>
            <Button variant="primary" disabled={busy || count === 0} onClick={() => void run()}>
              {busy ? "Exporting…" : "Export…"}
            </Button>
          </div>
        </div>
      </div>
    </Sheet>
  );
}
