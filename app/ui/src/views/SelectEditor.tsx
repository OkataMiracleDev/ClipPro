// The select under the playhead (SPEC TUN-11, P9.7): rating, colour label and note, plus the
// manual overrides on it (force out, trims) with their shortcuts.

import { Star, X } from "lucide-react";
import { useEffect, useState } from "react";
import { cx } from "../components/ui";
import { formatClock } from "../lib/typed";
import { useApp } from "../store";

export const LABELS: { name: string; colour: string }[] = [
  { name: "red", colour: "#ff453a" },
  { name: "orange", colour: "#ff9f0a" },
  { name: "yellow", colour: "#ffd60a" },
  { name: "green", colour: "#32d74b" },
  { name: "cyan", colour: "#64d2ff" },
  { name: "blue", colour: "#0a84ff" },
  { name: "purple", colour: "#bf5af2" },
  { name: "magenta", colour: "#ff375f" },
  { name: "white", colour: "#f2f2f7" },
];

const overlap = (a0: number, a1: number, b0: number, b1: number) => Math.max(0, Math.min(a1, b1) - Math.max(a0, b0));

export function SelectEditor() {
  const { scored, selectedClip, playheadUs, annotations, overrides, setAnnotation, removeOverride, toggleForce, marks } = useApp();
  const sc = scored?.clips.find((c) => c.clip === selectedClip);
  const idx = sc?.segments.findIndex((g) => playheadUs >= g.startUs && playheadUs < g.endUs) ?? -1;
  const seg = idx >= 0 ? sc!.segments[idx]! : null;
  const ann = seg
    ? annotations
        .filter((a) => a.clip === selectedClip)
        .map((a) => ({ a, o: overlap(a.startUs, a.endUs, seg.startUs, seg.endUs) }))
        .filter(({ a, o }) => o * 2 >= Math.min(a.endUs - a.startUs, seg.endUs - seg.startUs))
        .sort((x, y) => y.o - x.o)[0]?.a
    : undefined;
  const [note, setNote] = useState(ann?.note ?? "");
  useEffect(() => setNote(ann?.note ?? ""), [ann?.note, seg?.startUs, selectedClip]);
  const here = overrides.filter((o) => o.clip === selectedClip && playheadUs >= o.startUs && playheadUs < o.endUs);
  const mk = marks?.clip === selectedClip ? marks : null;

  if (!seg || selectedClip === null) {
    return (
      <div className="flex flex-col gap-1 text-[11px] text-text-3">
        <span>Not in a select.</span>
        <span>F forces a select here · I / O mark in and out · M adds a manual select.</span>
        {mk && (
          <span className="font-mono tabular">
            In {mk.inUs !== null ? formatClock(mk.inUs) : "—"} · Out {mk.outUs !== null ? formatClock(mk.outUs) : "—"}
          </span>
        )}
        {here.map((o) => (
          <OverrideChip key={o.id} kind={o.kind} onRemove={() => void removeOverride(o.id)} />
        ))}
      </div>
    );
  }
  const save = (patch: Partial<{ rating: number; label: string | null; note: string }>) =>
    void setAnnotation({ clip: selectedClip, startUs: seg.startUs, endUs: seg.endUs, rating: ann?.rating ?? 0, label: ann?.label ?? null, note: ann?.note ?? "", ...patch });
  const rating = ann?.rating ?? 0;

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-baseline justify-between text-[11px] text-text-3">
        <span>
          Select {idx + 1} · {formatClock(seg.startUs)}–{formatClock(seg.endUs)}
        </span>
        {seg.kind !== "auto" && <span className="rounded bg-control px-1 text-[10px]">{seg.kind === "manual" ? "edited" : "forced"}</span>}
      </div>
      <div className="flex items-center gap-0.5" role="radiogroup" aria-label="Rating">
        {[1, 2, 3, 4, 5].map((n) => (
          <button key={n} role="radio" aria-checked={rating === n} aria-label={`${n} star${n > 1 ? "s" : ""}`} onClick={() => save({ rating: rating === n ? 0 : n })} className="p-0.5">
            <Star size={15} className={n <= rating ? "fill-warning text-warning" : "text-text-3"} />
          </button>
        ))}
      </div>
      <div className="flex flex-wrap items-center gap-1" role="radiogroup" aria-label="Colour label">
        {LABELS.map((l) => (
          <button
            key={l.name}
            role="radio"
            aria-checked={ann?.label === l.name}
            aria-label={`${l.name} label`}
            title={l.name}
            onClick={() => save({ label: ann?.label === l.name ? null : l.name })}
            className={cx("h-4 w-4 rounded-full ring-1 ring-black/20", ann?.label === l.name && "ring-2 ring-text ring-offset-1 ring-offset-sidebar")}
            style={{ background: l.colour }}
          />
        ))}
      </div>
      <textarea
        value={note}
        placeholder="Note…"
        rows={2}
        onChange={(e) => setNote(e.target.value)}
        onBlur={() => note !== (ann?.note ?? "") && save({ note })}
        className="resize-none rounded border border-separator-strong bg-control px-2 py-1 text-[12px]"
      />
      <div className="flex flex-wrap items-center gap-1 text-[11px]">
        <button className="rounded px-1.5 py-0.5 ring-1 ring-separator-strong hover:bg-control-hover" onClick={() => void toggleForce()} title="Force this select out (F)">
          Force out
        </button>
        <span className="text-text-3">[ ] trim to the playhead</span>
        {here.map((o) => (
          <OverrideChip key={o.id} kind={o.kind} onRemove={() => void removeOverride(o.id)} />
        ))}
      </div>
    </div>
  );
}

function OverrideChip({ kind, onRemove }: { kind: string; onRemove: () => void }) {
  const label = { force_in: "Forced in", force_out: "Forced out", trim: "Trimmed", manual: "Manual select" }[kind] ?? kind;
  return (
    <span className="flex items-center gap-0.5 rounded bg-accent-soft px-1.5 py-0.5 text-[10.5px] text-accent">
      {label}
      <button aria-label={`Remove: ${label}`} onClick={onRemove} className="hover:text-text">
        <X size={10} />
      </button>
    </span>
  );
}
