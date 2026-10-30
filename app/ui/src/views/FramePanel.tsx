// Frame inspector (SPEC VIE-5): the sample under the playhead — verdict and reasons, the score
// chain (Overall → tiers → camera filter → F), groups, sub-metrics, flags, histogram and raw
// measurements.

import { Check, ChevronDown, ChevronRight, X } from "lucide-react";
import { useState } from "react";
import type { Inspection } from "../api";
import { scoreColor } from "../components/ui";
import { FLAGS } from "../lib/domain";
import { useInspection } from "../lib/inspect";
import { formatClock } from "../lib/typed";
import { useApp } from "../store";
import { SelectEditor } from "./SelectEditor";

const pct = (v: number) => (Number.isFinite(v) ? `${Math.round(v * 100)}` : "—");
const mult = (v: number) => (Number.isFinite(v) ? `×${v.toFixed(2)}` : "—");
const flagLabel = (k: string) => FLAGS.find((f) => f.key === k)?.label ?? k;

function Bar({ value }: { value: number }) {
  const v = Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : 0;
  return (
    <div className="h-1.5 w-full overflow-hidden rounded-full bg-control">
      <div className="h-full rounded-full" style={{ width: `${v * 100}%`, background: scoreColor(value) }} />
    </div>
  );
}

function Histogram({ hist }: { hist: number[] }) {
  const max = Math.max(1, ...hist);
  return (
    <svg viewBox={`0 0 ${hist.length} 32`} preserveAspectRatio="none" className="h-10 w-full" role="img" aria-label="Luma histogram">
      <rect width={hist.length} height="32" fill="var(--c-control)" />
      {hist.map((h, i) => {
        const y = 32 * (h / max);
        return <rect key={i} x={i} y={32 - y} width="1" height={y} fill="var(--c-text-2)" opacity={0.75} />;
      })}
    </svg>
  );
}

function Verdict({ d }: { d: Inspection }) {
  return (
    <div className={`flex flex-col gap-1 rounded-(--radius-control) px-3 py-2 ring-1 ${d.pass ? "bg-accent-soft ring-accent" : "bg-control ring-separator-strong"}`}>
      <div className="flex items-center gap-1.5 text-[13px] font-medium">
        {d.pass ? <Check size={14} className="text-accent-text" /> : <X size={14} className="text-text-3" />}
        {d.pass ? `Selected${d.select ? ` · select ${d.select}` : ""}` : "Not selected"}
      </div>
      {d.reasons.map((r) => (
        <div key={r} className="text-[12px] text-text-2">
          {r}
        </div>
      ))}
    </div>
  );
}

function Chain({ d }: { d: Inspection }) {
  const rows: [string, string][] = [
    ["Overall", pct(d.overall)],
    ["Flag tiers", mult(d.tierMultiplier)],
    ["Camera filter", mult(d.cameraFactor)],
  ];
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-baseline justify-between">
        <span className="text-[12px] text-text-2">Score</span>
        <span className="font-mono text-[20px] font-medium tabular" style={{ color: scoreColor(d.score) }}>
          {pct(d.score)}
        </span>
      </div>
      {rows.map(([k, v]) => (
        <div key={k} className="flex justify-between text-[12px]">
          <span className="text-text-3">{k}</span>
          <span className="font-mono tabular text-text-2">{v}</span>
        </div>
      ))}
      {(d.forceIn || d.forceOut) && <div className="text-[11px] text-text-3">{d.forceOut ? "Force-excluded" : "Force-included"} by a flag tier</div>}
    </div>
  );
}

function Groups({ d }: { d: Inspection }) {
  const [open, setOpen] = useState<string | null>(null);
  return (
    <div className="flex flex-col gap-1.5">
      {d.groups.map((g) => {
        const subs = d.subs.filter((s) => s.group === g.key);
        const isOpen = open === g.key;
        return (
          <div key={g.key} className="flex flex-col gap-1">
            <button className="flex items-center gap-1.5 text-left text-[12px]" onClick={() => setOpen(isOpen ? null : g.key)} aria-expanded={isOpen}>
              {isOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
              <span className="flex-1">{g.label}</span>
              <span className="font-mono text-[11px] text-text-3 tabular">w {Number.isFinite(g.weight) ? g.weight.toFixed(2) : "—"}</span>
              <span className="w-7 text-right font-mono tabular">{pct(g.score)}</span>
            </button>
            <Bar value={g.score} />
            {isOpen && (
              <div className="ml-4 flex flex-col gap-1 py-1">
                {subs.length === 0 && <div className="text-[11px] text-text-3">No sub-metrics</div>}
                {subs.map((s) => (
                  <div key={s.key} className="grid grid-cols-[1fr_auto_auto] items-center gap-x-2 text-[11px]" title={`Measured ${pct(s.value)} (normalised), weight ${s.weight.toFixed(2)}`}>
                    <span className="truncate text-text-2">{s.label}</span>
                    <span className="font-mono text-text-3 tabular">{pct(s.value)}</span>
                    <span className="w-7 text-right font-mono tabular">{pct(s.score)}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

export function FramePanel() {
  const clip = useApp((s) => s.selectedClip);
  const { data, error } = useInspection(true);
  const [showRaw, setShowRaw] = useState(false);
  if (clip === null) return <div className="px-4 py-6 text-[12px] text-text-3">Select a clip to inspect its frames.</div>;
  if (error) return <div className="px-4 py-6 text-[12px] text-text-3">{error}</div>;
  if (!data) return <div className="px-4 py-6 text-[12px] text-text-3">Loading…</div>;
  return (
    <div className="flex flex-col gap-4 px-4 py-3">
      <div className="flex items-baseline justify-between text-[11px] text-text-3">
        <span>Sample {data.sample + 1}</span>
        <span className="font-mono tabular">{formatClock(data.timeUs)}</span>
      </div>
      <Verdict d={data} />
      <section className="flex flex-col gap-2">
        <h3 className="section-header">Select</h3>
        <SelectEditor />
      </section>
      <Chain d={data} />
      <section className="flex flex-col gap-2">
        <h3 className="section-header">Groups</h3>
        <Groups d={data} />
      </section>
      <section className="flex flex-col gap-2">
        <h3 className="section-header">Flags</h3>
        {data.flags.length === 0 ? (
          <div className="text-[12px] text-text-3">None</div>
        ) : (
          <div className="flex flex-wrap gap-1">
            {data.flags.map((f) => (
              <span key={f} className="rounded bg-control px-1.5 py-0.5 text-[11px] ring-1 ring-separator-strong">
                {flagLabel(f)}
              </span>
            ))}
          </div>
        )}
      </section>
      <section className="flex flex-col gap-2">
        <h3 className="section-header">Luma histogram</h3>
        <Histogram hist={data.hist} />
      </section>
      <section className="flex flex-col gap-2">
        <button className="section-header flex items-center gap-1" onClick={() => setShowRaw(!showRaw)} aria-expanded={showRaw}>
          {showRaw ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          Raw measurements
        </button>
        {showRaw && (
          <div className="grid grid-cols-[1fr_auto] gap-x-3 gap-y-0.5 font-mono text-[11px]">
            {data.raw.map((r) => (
              <div key={r.key} className="contents">
                <span className="truncate text-text-3">{r.key}</span>
                <span className="text-right tabular text-text-2">{Number.isFinite(r.value) ? r.value.toPrecision(4) : "—"}</span>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
