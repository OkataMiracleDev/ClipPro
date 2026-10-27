// Sliders with typed-value editing (SPEC §14.2): double-click a value to type it exactly
// (`4`, `4.5`, `1:30`, `90s`, `1h`, `none`). Arrow keys nudge; Shift+arrow nudges ×10.

import * as RSlider from "@radix-ui/react-slider";
import { useState } from "react";
import type { Parsed } from "../lib/typed";
import { cx } from "./ui";

/** Maps slider positions (integers) to values and back. */
export interface Scale<T> {
  steps: number;
  toValue(pos: number): T;
  toPos(v: T): number;
  format(v: T): string;
  parse(s: string): Parsed<T>;
}

export function linearScale(min: number, max: number, steps: number, format: (v: number) => string, parse: (s: string) => Parsed<number>): Scale<number> {
  return {
    steps,
    toValue: (p) => min + ((max - min) * p) / steps,
    toPos: (v) => Math.round(((Math.min(max, Math.max(min, v)) - min) / (max - min)) * steps),
    format,
    parse,
  };
}

/** Discrete stops, with `null` (unlimited) allowed as the last stop. */
export function stopScale<T extends number | null>(stops: T[], format: (v: T) => string, parse: (s: string) => Parsed<T>): Scale<T> {
  return {
    steps: stops.length - 1,
    toValue: (p) => stops[Math.max(0, Math.min(stops.length - 1, p))]!,
    toPos: (v) => {
      if (v === null) return stops.length - 1;
      let best = 0;
      stops.forEach((s, i) => {
        if (s !== null && Math.abs((s as number) - (v as number)) < Math.abs(((stops[best] ?? 0) as number) - (v as number))) best = i;
      });
      return best;
    },
    format,
    parse,
  };
}

function EditableValue<T>({ value, scale, onCommit, label }: { value: T; scale: Scale<T>; onCommit: (v: T) => void; label: string }) {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  if (editing) {
    return (
      <input
        autoFocus
        aria-label={`${label} value`}
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          setError(null);
        }}
        onBlur={() => setEditing(false)}
        onKeyDown={(e) => {
          if (e.key === "Escape") setEditing(false);
          if (e.key === "Enter") {
            const r = scale.parse(text);
            if (r.ok) {
              onCommit(r.value);
              setEditing(false);
            } else setError(r.error);
          }
        }}
        title={error ?? undefined}
        className={cx(
          "h-5 w-16 rounded border bg-control px-1 text-right font-mono text-[11px] outline-none",
          error ? "border-danger" : "border-accent",
        )}
      />
    );
  }
  return (
    <button
      onDoubleClick={() => {
        setText(scale.format(value));
        setEditing(true);
      }}
      title="Double-click to type a value"
      className="h-5 min-w-10 rounded px-1 text-right font-mono text-[11px] text-text-2 tabular hover:bg-control-hover"
    >
      {scale.format(value)}
    </button>
  );
}

const thumb =
  "block h-[18px] w-[18px] rounded-full bg-white shadow-[0_1px_3px_rgba(0,0,0,0.4)] ring-1 ring-black/10 outline-none focus-visible:ring-4 focus-visible:ring-(--c-focus)";

export function RangeSlider<T>({
  label,
  scale,
  value,
  onChange,
  trackStyle,
}: {
  label: string;
  scale: Scale<T>;
  value: [T, T];
  onChange: (v: [T, T]) => void;
  /** Optional background for the track (e.g. the score gradient). */
  trackStyle?: React.CSSProperties;
}) {
  const pos: [number, number] = [scale.toPos(value[0]), scale.toPos(value[1])];
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between">
        <span className="text-[12px] text-text-2">{label}</span>
        <span className="flex items-center gap-1">
          <EditableValue label={`${label} minimum`} value={value[0]} scale={scale} onCommit={(v) => onChange([v, value[1]])} />
          <span className="text-text-3">–</span>
          <EditableValue label={`${label} maximum`} value={value[1]} scale={scale} onCommit={(v) => onChange([value[0], v])} />
        </span>
      </div>
      <RSlider.Root
        className="relative flex h-5 touch-none items-center select-none"
        min={0}
        max={scale.steps}
        step={1}
        minStepsBetweenThumbs={0}
        value={pos}
        aria-label={label}
        onValueChange={(p) => onChange([scale.toValue(p[0]!), scale.toValue(p[1]!)])}
      >
        <RSlider.Track className="relative h-1 grow rounded-full bg-control-hover" style={trackStyle}>
          <RSlider.Range className={cx("absolute h-full rounded-full", trackStyle ? "bg-white/25 ring-1 ring-white/40" : "bg-accent")} />
        </RSlider.Track>
        <RSlider.Thumb className={thumb} aria-label={`${label} minimum`} />
        <RSlider.Thumb className={thumb} aria-label={`${label} maximum`} />
      </RSlider.Root>
    </div>
  );
}

export function SingleSlider<T>({ label, scale, value, onChange }: { label: string; scale: Scale<T>; value: T; onChange: (v: T) => void }) {
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between">
        <span className="text-[12px] text-text-2">{label}</span>
        <EditableValue label={label} value={value} scale={scale} onCommit={onChange} />
      </div>
      <RSlider.Root
        className="relative flex h-5 touch-none items-center select-none"
        min={0}
        max={scale.steps}
        step={1}
        value={[scale.toPos(value)]}
        aria-label={label}
        onValueChange={(p) => onChange(scale.toValue(p[0]!))}
      >
        <RSlider.Track className="relative h-1 grow rounded-full bg-control-hover">
          <RSlider.Range className="absolute h-full rounded-full bg-accent" />
        </RSlider.Track>
        <RSlider.Thumb className={thumb} aria-label={label} />
      </RSlider.Root>
    </div>
  );
}
