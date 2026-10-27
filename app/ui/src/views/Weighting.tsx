// Weighting (SPEC TUN-4/5, §14.2): a radar of group weights (drag a vertex along its axis; the
// centre is 0%, Ctrl+drag locks the others) and, per group, the sub-metric sliders — bipolar with a
// Neutral detent for most, target + tolerance for Luma, Contrast and Loudness.

import { ChevronDown, ChevronRight } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api, n, type Settings } from "../api";
import { cx, Switch } from "../components/ui";
import { amberGroups, ENDS, GROUPS, setWeight, snapPref, TARGETS, type SubMetricInfo } from "../lib/submetrics";
import { useApp } from "../store";

let infoCache: Promise<SubMetricInfo[]> | null = null;

export function useSubMetrics(): SubMetricInfo[] {
  const [infos, setInfos] = useState<SubMetricInfo[]>([]);
  useEffect(() => {
    infoCache ??= api.submetrics();
    infoCache.then(setInfos, () => (infoCache = null));
  }, []);
  return infos;
}

const SIZE = 250;
const R = 70;
const C = SIZE / 2;

function Radar({ weights, axes, amber, onChange }: { weights: number[]; axes: number[]; amber: Set<string>; onChange: (w: number[]) => void }) {
  const svg = useRef<SVGSVGElement>(null);
  const max = Math.max(1, ...axes.map((i) => weights[i] ?? 0));
  const scale = Math.max(50, Math.ceil(max / 10) * 10); // radius = max weight, at least 50%
  const angle = (k: number) => -Math.PI / 2 + (2 * Math.PI * k) / axes.length;
  const pt = (k: number, v: number) => [C + Math.cos(angle(k)) * (R * v) / scale, C + Math.sin(angle(k)) * (R * v) / scale] as const;
  const poly = axes.map((gi, k) => pt(k, weights[gi] ?? 0).join(",")).join(" ");

  const drag = (k: number, gi: number) => (e: React.PointerEvent) => {
    e.preventDefault();
    const el = svg.current;
    if (!el) return;
    (e.target as Element).setPointerCapture(e.pointerId);
    const s0 = scale;
    const move = (ev: PointerEvent) => {
      const r = el.getBoundingClientRect();
      const x = ((ev.clientX - r.left) / r.width) * SIZE - C;
      const y = ((ev.clientY - r.top) / r.height) * SIZE - C;
      // Project the pointer on the axis.
      const along = x * Math.cos(angle(k)) + y * Math.sin(angle(k));
      const v = Math.max(0, (along / R) * s0);
      onChange(setWeight(useApp.getState().settings!.weights.map((w) => n(w)), gi, v < 2 ? 0 : v, ev.ctrlKey));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  return (
    <svg ref={svg} viewBox={`0 0 ${SIZE} ${SIZE}`} className="mx-auto block aspect-square w-full max-w-[250px] touch-none select-none" role="group" aria-label="Metric weighting">
      {[0.25, 0.5, 0.75, 1].map((f) => (
        <polygon key={f} points={axes.map((_, k) => pt(k, f * scale).join(",")).join(" ")} fill="none" stroke="var(--c-separator-strong)" strokeWidth={f === 1 ? 1 : 0.6} />
      ))}
      {axes.map((_, k) => (
        <line key={k} x1={C} y1={C} x2={pt(k, scale)[0]} y2={pt(k, scale)[1]} stroke="var(--c-separator)" />
      ))}
      <polygon points={poly} fill="var(--c-accent-soft)" stroke="var(--c-accent)" strokeWidth={1.5} />
      {axes.map((gi, k) => {
        const g = GROUPS[gi]!;
        const w = weights[gi] ?? 0;
        const [x, y] = pt(k, w);
        const [lx, ly] = pt(k, scale * 1.3);
        return (
          <g key={g.key}>
            <text x={lx} y={ly} textAnchor="middle" dominantBaseline="middle" fontSize="10.5" fill={amber.has(g.key) ? "var(--c-warning)" : "var(--c-text-2)"}>
              {g.label}
            </text>
            <text x={lx} y={ly + 11} textAnchor="middle" dominantBaseline="middle" fontSize="9" fill="var(--c-text-3)" fontFamily="JetBrains Mono, monospace">
              {Math.round(w)}%
            </text>
            <circle
              cx={x}
              cy={y}
              r={6}
              fill="var(--c-raised)"
              stroke="var(--c-accent)"
              strokeWidth={2}
              className="cursor-grab"
              role="slider"
              tabIndex={0}
              aria-label={`${g.label} weight`}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={Math.round(w)}
              onPointerDown={drag(k, gi)}
              onKeyDown={(e) => {
                const step = e.shiftKey ? 10 : 1;
                const d = e.key === "ArrowUp" || e.key === "ArrowRight" ? step : e.key === "ArrowDown" || e.key === "ArrowLeft" ? -step : 0;
                if (!d) return;
                e.preventDefault();
                onChange(setWeight(weights, gi, w + d, e.ctrlKey));
              }}
            />
          </g>
        );
      })}
    </svg>
  );
}

/** −1…1 with a Neutral detent; the fill grows from the centre. */
export function BipolarSlider({ label, value, ends, disabled, onChange }: { label: string; value: number; ends: [string, string]; disabled?: boolean; onChange: (v: number) => void }) {
  const track = useRef<HTMLDivElement>(null);
  const set = (clientX: number) => {
    const r = track.current?.getBoundingClientRect();
    if (!r) return;
    onChange(snapPref(((clientX - r.left) / r.width) * 2 - 1));
  };
  const pos = (value + 1) / 2;
  return (
    <div className={cx("flex flex-col gap-0.5", disabled && "opacity-40")}>
      <div
        ref={track}
        role="slider"
        tabIndex={disabled ? -1 : 0}
        aria-label={label}
        aria-valuemin={-1}
        aria-valuemax={1}
        aria-valuenow={value}
        aria-valuetext={value === 0 ? "Neutral" : `${Math.round(Math.abs(value) * 100)}% toward ${value > 0 ? ends[1] : ends[0]}`}
        aria-disabled={disabled}
        className="relative h-4 cursor-pointer touch-none"
        onPointerDown={(e) => {
          if (disabled) return;
          (e.target as Element).setPointerCapture(e.pointerId);
          set(e.clientX);
        }}
        onPointerMove={(e) => !disabled && e.buttons === 1 && set(e.clientX)}
        onDoubleClick={() => !disabled && onChange(0)}
        onKeyDown={(e) => {
          if (disabled) return;
          const step = e.shiftKey ? 0.1 : 0.05;
          if (e.key === "ArrowRight" || e.key === "ArrowUp") onChange(snapPref(value + step));
          else if (e.key === "ArrowLeft" || e.key === "ArrowDown") onChange(snapPref(value - step));
          else if (e.key === "Home" || e.key === "0") onChange(0);
          else return;
          e.preventDefault();
        }}
      >
        <div className="absolute inset-x-0 top-1/2 h-[3px] -translate-y-1/2 rounded-full bg-control-hover" />
        <div className="absolute top-1/2 h-[3px] -translate-y-1/2 rounded-full bg-accent" style={{ left: `${Math.min(pos, 0.5) * 100}%`, width: `${Math.abs(pos - 0.5) * 100}%` }} />
        <div className="absolute top-1/2 left-1/2 h-2 w-px -translate-y-1/2 bg-text-3" aria-hidden />
        <div className="absolute top-1/2 h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-white shadow ring-1 ring-black/20" style={{ left: `${pos * 100}%` }} />
      </div>
      <div className="flex justify-between text-[10px] text-text-3">
        <span>{ends[0]}</span>
        <span>{value === 0 ? "Neutral" : `${Math.round(Math.abs(value) * 100)}%`}</span>
        <span>{ends[1]}</span>
      </div>
    </div>
  );
}

function TargetSlider({ k, target, tol, disabled, onChange }: { k: string; target: number; tol: number; disabled?: boolean; onChange: (target: number, tol: number) => void }) {
  const t = TARGETS[k] ?? { min: 0, max: 1, fmt: (v: number) => v.toFixed(2), tolMax: 0.5 };
  const frac = (v: number) => (v - t.min) / (t.max - t.min);
  const step = (t.max - t.min) / 100;
  return (
    <div className={cx("flex flex-col gap-1", disabled && "opacity-40")}>
      <div className="relative h-4">
        <div className="absolute inset-x-0 top-1/2 h-[3px] -translate-y-1/2 rounded-full bg-control-hover" />
        <div
          className="absolute top-1/2 h-[7px] -translate-y-1/2 rounded-full bg-accent-soft ring-1 ring-accent/40"
          style={{ left: `${Math.max(0, frac(target - tol)) * 100}%`, width: `${(Math.min(1, frac(target + tol)) - Math.max(0, frac(target - tol))) * 100}%` }}
        />
        <input
          type="range"
          aria-label={`${k} target`}
          min={t.min}
          max={t.max}
          step={step}
          value={target}
          disabled={disabled}
          onChange={(e) => onChange(Number(e.target.value), tol)}
          className="range-thumb absolute inset-0 w-full cursor-pointer appearance-none bg-transparent"
        />
      </div>
      <div className="flex items-center justify-between gap-2 text-[10px] text-text-3">
        <span>Target {t.fmt(target)}</span>
        <label className="flex items-center gap-1">
          ±
          <input
            type="range"
            aria-label={`${k} tolerance`}
            min={step}
            max={t.tolMax}
            step={step}
            value={tol}
            disabled={disabled}
            onChange={(e) => onChange(target, Number(e.target.value))}
            className="h-1 w-16 cursor-pointer accent-(--c-accent)"
          />
          {t.fmt(tol).replace("%", "")}
          {t.fmt(tol).endsWith("%") ? "%" : ""}
        </label>
      </div>
    </div>
  );
}

export function Weighting() {
  const settings = useApp((s) => s.settings);
  const updateSettings = useApp((s) => s.updateSettings);
  const infos = useSubMetrics();
  const [open, setOpen] = useState<string | null>(null);
  if (!settings) return null;
  const weights = settings.weights.map((w) => n(w));
  const subs = settings.subs.map((s) => ({ ...s, pref: n(s.pref), target: n(s.target), tol: n(s.tol), strength: n(s.strength) }));
  const amber = amberGroups(weights, subs, infos);
  // Content is an AI group: shown once AI modules exist (P12) or when a preset weights it.
  const axes = GROUPS.map((_, i) => i).filter((i) => !GROUPS[i]!.ai || weights[i]! > 0);
  const setSub = (i: number, patch: Partial<Settings["subs"][number]>) => updateSettings((s) => ({ ...s, subs: s.subs.map((x, j) => (j === i ? { ...x, ...patch } : x)) }));

  return (
    <div className={cx("flex flex-col gap-2", !settings.switches.metrics && "opacity-40")}>
      <Radar weights={weights} axes={axes} amber={amber} onChange={(w) => updateSettings((s) => ({ ...s, weights: w as Settings["weights"] }))} />
      <p className="text-center text-[10.5px] text-text-3">Drag a corner; the centre turns a group off. Ctrl+drag keeps the others.</p>
      {axes.map((gi) => {
        const g = GROUPS[gi]!;
        const members = infos.map((m, i) => ({ m, i })).filter((x) => x.m.group === g.key);
        const isOpen = open === g.key;
        return (
          <div key={g.key} className="flex flex-col gap-2">
            <button className="flex items-center gap-1.5 text-left text-[12px]" onClick={() => setOpen(isOpen ? null : g.key)} aria-expanded={isOpen}>
              {isOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
              <span className={cx("flex-1", amber.has(g.key) ? "text-warning" : "text-text-2")} title={amber.has(g.key) ? "Weighted, but every sub-metric is off: the group has no effect" : undefined}>
                {g.label}
                {amber.has(g.key) && " · no sub-metric on"}
              </span>
              <span className="font-mono text-[11px] text-text-3 tabular">{Math.round(weights[gi]!)}%</span>
            </button>
            {isOpen && (
              <div className={cx("ml-4 flex flex-col gap-3", weights[gi] === 0 && "opacity-50")}>
                {members.map(({ m, i }) => {
                  const s = subs[i]!;
                  return (
                    <div key={m.key} className="flex flex-col gap-1">
                      <label className="flex items-center justify-between gap-2">
                        <span className="text-[12px] text-text-2">{m.label}</span>
                        <Switch label={`${m.label} on`} checked={s.enabled} onChange={(enabled) => setSub(i, { enabled, ...(enabled && !m.target && s.pref === 0 ? { pref: 0.5 } : {}) })} />
                      </label>
                      {m.target ? (
                        <TargetSlider k={m.key} target={s.target} tol={s.tol} disabled={!s.enabled} onChange={(target, tol) => setSub(i, { target, tol })} />
                      ) : (
                        <BipolarSlider label={m.label} value={s.pref} ends={ENDS[m.key] ?? ["Low", "High"]} disabled={!s.enabled} onChange={(pref) => setSub(i, { pref })} />
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
