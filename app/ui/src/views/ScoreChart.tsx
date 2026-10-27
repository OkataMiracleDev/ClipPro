// Score chart (SPEC TML-2): every select as a dot; x = duration (log), y = mean score.
// The dashed lines mirror the score threshold and can be dragged.

import { useEffect, useMemo, useRef, useState } from "react";
import { n } from "../api";
import { cssVar, scoreRamp, setupCanvas, useSize } from "../lib/canvas";
import { formatClock, formatDuration } from "../lib/typed";
import { useApp } from "../store";

const PAD = { l: 34, r: 12, t: 10, b: 22 };

export function ScoreChart() {
  const { scored, clips, settings, updateSettings, selectedSelect, selectSelect } = useApp();
  const [wrap, size] = useSize<HTMLDivElement>();
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const [hover, setHover] = useState<number | null>(null);
  const [drag, setDrag] = useState<"lo" | "hi" | null>(null);
  const names = useMemo(() => new Map(clips.map((c) => [c.clip, c.name])), [clips]);
  const selects = scored?.selects ?? [];
  const lo = n(settings?.score_lo);
  const hi = n(settings?.score_hi, 1);

  const geom = useMemo(() => {
    const durs = selects.map((s) => Math.max(0.1, (s.segment.endUs - s.segment.startUs) / 1e6));
    const maxD = Math.max(10, ...durs);
    const lx0 = Math.log10(0.1);
    const lx1 = Math.log10(maxD * 1.2);
    const W = size.w - PAD.l - PAD.r;
    const H = size.h - PAD.t - PAD.b;
    const x = (d: number) => PAD.l + ((Math.log10(Math.max(0.1, d)) - lx0) / (lx1 - lx0)) * W;
    const y = (v: number) => PAD.t + (1 - v) * H;
    const yInv = (py: number) => Math.min(1, Math.max(0, 1 - (py - PAD.t) / Math.max(1, H)));
    return { durs, x, y, yInv, maxD };
  }, [selects, size]);

  useEffect(() => {
    const c = canvas.current;
    if (!c || size.w < 50 || size.h < 50) return;
    const ctx = setupCanvas(c, size.w, size.h);
    if (!ctx) return;
    const ramp = scoreRamp();
    const sep = cssVar("--c-separator");
    const text3 = cssVar("--c-text-3");
    ctx.font = "10px 'JetBrains Mono Variable', monospace";
    // Grid
    ctx.strokeStyle = sep;
    ctx.lineWidth = 1;
    ctx.fillStyle = text3;
    for (const v of [0, 0.25, 0.5, 0.75, 1]) {
      const yy = Math.round(geom.y(v)) + 0.5;
      ctx.beginPath();
      ctx.moveTo(PAD.l, yy);
      ctx.lineTo(size.w - PAD.r, yy);
      ctx.stroke();
      ctx.fillText(String(Math.round(v * 100)), 6, yy + 3);
    }
    for (const d of [0.5, 1, 2, 5, 10, 30, 60, 120, 300, 600].filter((d) => d <= geom.maxD * 1.2)) {
      const xx = Math.round(geom.x(d)) + 0.5;
      ctx.fillText(formatDuration(d), xx - 8, size.h - 6);
    }
    // Threshold lines
    ctx.setLineDash([4, 4]);
    ctx.strokeStyle = cssVar("--c-text-2");
    for (const v of [lo, hi]) {
      const yy = Math.round(geom.y(v)) + 0.5;
      ctx.beginPath();
      ctx.moveTo(PAD.l, yy);
      ctx.lineTo(size.w - PAD.r, yy);
      ctx.stroke();
    }
    ctx.setLineDash([]);
    // Dots
    const sel = selectedSelect !== null ? selects[selectedSelect] : null;
    selects.forEach((s, i) => {
      const px = geom.x(geom.durs[i]!);
      const py = geom.y(s.segment.mean);
      ctx.beginPath();
      ctx.arc(px, py, 3.5, 0, Math.PI * 2);
      ctx.fillStyle = ramp[Math.round(Math.max(0, Math.min(1, s.segment.mean)) * 255)]!;
      ctx.fill();
      if (sel && s.clip === sel.clip && i !== selectedSelect) {
        ctx.strokeStyle = cssVar("--c-accent");
        ctx.lineWidth = 1.5;
        ctx.stroke();
      }
    });
    if (sel && selectedSelect !== null) {
      ctx.beginPath();
      ctx.arc(geom.x(geom.durs[selectedSelect]!), geom.y(sel.segment.mean), 6, 0, Math.PI * 2);
      ctx.strokeStyle = "#fff";
      ctx.lineWidth = 2;
      ctx.stroke();
    }
  }, [selects, geom, size, lo, hi, selectedSelect]);

  const nearest = (px: number, py: number) => {
    let best = -1;
    let bd = 64;
    selects.forEach((s, i) => {
      const d = (geom.x(geom.durs[i]!) - px) ** 2 + (geom.y(s.segment.mean) - py) ** 2;
      if (d < bd) {
        bd = d;
        best = i;
      }
    });
    return best < 0 ? null : best;
  };

  const local = (e: React.PointerEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top] as const;
  };

  const hoverSel = hover !== null ? selects[hover] : null;
  const s = scored?.summary;
  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between px-3 pt-2 pb-1 text-[12px]">
        <span className="font-medium text-text-2">Score chart</span>
        <span className="font-mono text-[11px] text-text-3 tabular">
          {s ? `${s.count} selects · ${formatClock(s.selectsUs)} · ${s.percent.toFixed(1)}% of footage` : "—"}
        </span>
      </div>
      <div ref={wrap} className="relative min-h-0 flex-1">
        <canvas
          ref={canvas}
          className="absolute inset-0"
          style={{ cursor: drag ? "ns-resize" : "default" }}
          aria-label="Score chart: selects by duration and score"
          role="img"
          onPointerDown={(e) => {
            const [px, py] = local(e);
            const near = (v: number) => Math.abs(geom.y(v) - py) < 6;
            if (near(lo) || near(hi)) {
              setDrag(near(lo) && (!near(hi) || Math.abs(geom.y(lo) - py) <= Math.abs(geom.y(hi) - py)) ? "lo" : "hi");
              (e.target as HTMLElement).setPointerCapture(e.pointerId);
              return;
            }
            const i = nearest(px, py);
            if (i !== null) selectSelect(i);
          }}
          onPointerMove={(e) => {
            const [px, py] = local(e);
            if (drag) {
              const v = Math.round(geom.yInv(py) * 100) / 100;
              updateSettings((st) => (drag === "lo" ? { ...st, score_lo: Math.min(v, n(st.score_hi, 1)) } : { ...st, score_hi: Math.max(v, n(st.score_lo)) }));
              return;
            }
            setHover(nearest(px, py));
          }}
          onPointerUp={() => setDrag(null)}
          onPointerLeave={() => setHover(null)}
        />
        {hoverSel && hover !== null && (
          <div
            className="pointer-events-none absolute z-10 rounded-md bg-raised px-2 py-1 text-[11px] shadow-(--shadow-float) ring-1 ring-separator-strong"
            style={{ left: Math.min(size.w - 180, geom.x(geom.durs[hover]!) + 10), top: Math.max(0, geom.y(hoverSel.segment.mean) - 34) }}
          >
            <div className="font-medium">{names.get(hoverSel.clip) ?? "Clip"}</div>
            <div className="font-mono text-text-2 tabular">
              score {Math.round(hoverSel.segment.mean * 100)} · {formatDuration((hoverSel.segment.endUs - hoverSel.segment.startUs) / 1e6)}
            </div>
          </div>
        )}
        {!selects.length && (
          <div className="absolute inset-0 grid place-items-center text-[12px] text-text-3">No selects with these settings</div>
        )}
      </div>
    </div>
  );
}
