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
  { value: "html", label: "HTML report", hint: "Thumbnails, sortable tables, one file", ext: "html", filter: "HTML", timeline: false },
  { value: "pdf", label: "PDF contact sheet", hint: "Best frame of each select, for clients", ext: "pdf", filter: "PDF", timeline: false },
];

type GroupBy = "clip" | "camera" | "date" | "reel" | "rating" | "label";

const GROUPS: { value: GroupBy; label: string }[] = [
  { value: "clip", label: "Clip" },
  { value: "camera", label: "Camera" },
  { value: "date", label: "Date recorded" },
  { value: "reel", label: "Reel" },
  { value: "rating", label: "Rating" },
  { value: "label", label: "Colour label" },
];

/** Reports have no markers, handles or audio. */
const REPORTS = new Set(["html", "pdf"]);

export function ExportSheet({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const { settings, sort, project, scored, toast } = useApp();
  const [format, setFormat] = useState("xmeml");
  // `?export=stringout` (screenshots) starts on the stringout layout.
  const [layout, setLayout] = useState<"selects" | "with_rejects" | "stringout">(() => (new URLSearchParams(location.search).get("export") === "stringout" ? "stringout" : "selects"));
  const [groupBy, setGroupBy] = useState<GroupBy>("clip");
  const [groupOrder, setGroupOrder] = useState<"key" | "best_first">("key");
  const [separator, setSeparator] = useState<"none" | "gap" | "slug">("slug");
  const [sepSeconds, setSepSeconds] = useState("2");
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
        group_by: groupBy,
        group_order: groupOrder,
        separator,
        separator_s: Math.max(0, Number(sepSeconds) || 0),
        handles,
        gap_s: Math.max(0, Number(gap) || 0),
        with_audio: audio,
        markers,
        marker_name: markerName,
        marker_comment: "{flags}",
        generated: new Date().toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }),
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
                { value: "stringout", label: "Stringout", title: "Selects grouped, with gaps or titled slugs between groups" },
              ]}
            />
          </div>
        )}
        {def.timeline && layout === "stringout" && (
          <div className="flex flex-col gap-2.5 rounded-(--radius-control) bg-control-hover/40 p-2.5 ring-1 ring-separator ring-inset">
            <div className={row}>
              <span className="text-[12px] text-text-2">Group by</span>
              <span className="flex items-center gap-2">
                <select aria-label="Group by" value={groupBy} onChange={(e) => setGroupBy(e.target.value as GroupBy)} className="h-6 rounded border border-separator-strong bg-control px-1.5 text-[12px]">
                  {GROUPS.map((g) => (
                    <option key={g.value} value={g.value}>
                      {g.label}
                    </option>
                  ))}
                </select>
                <Segmented
                  size="sm"
                  label="Group order"
                  value={groupOrder}
                  onChange={setGroupOrder}
                  options={[
                    { value: "key", label: groupBy === "rating" || groupBy === "label" ? "Best first" : groupBy === "date" ? "Oldest first" : "A–Z", title: "By the group's name or value" },
                    { value: "best_first", label: "Best select first", title: "The group holding the highest-scoring select plays first" },
                  ]}
                />
              </span>
            </div>
            <div className={row}>
              <span className="text-[12px] text-text-2">Between groups</span>
              <span className="flex items-center gap-2">
                <Segmented
                  size="sm"
                  label="Between groups"
                  value={separator}
                  onChange={setSeparator}
                  options={[
                    { value: "none", label: "Nothing" },
                    { value: "gap", label: "Gap" },
                    { value: "slug", label: "Titled slug", title: "A black slug named after the group, before each group" },
                  ]}
                />
                {separator !== "none" && (
                  <input
                    aria-label="Separator length in seconds"
                    value={sepSeconds}
                    onChange={(e) => setSepSeconds(e.target.value)}
                    inputMode="decimal"
                    className="h-6 w-12 rounded border border-separator-strong bg-control px-1.5 text-right font-mono text-[12px]"
                  />
                )}
                {separator !== "none" && <span className="text-[11px] text-text-3">s</span>}
              </span>
            </div>
          </div>
        )}
        {!REPORTS.has(format) && (
          <label className={row}>
            <span className="text-[12px] text-text-2">Include handles</span>
            <Switch label="Include handles" checked={handles} onChange={setHandles} />
          </label>
        )}
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
        {!REPORTS.has(format) && (
          <label className={row}>
            <span className="text-[12px] text-text-2">Markers</span>
            <Switch label="Markers" checked={markers} onChange={setMarkers} />
          </label>
        )}
        {markers && !REPORTS.has(format) && (
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
