// Measurement overlays (SPEC VIE-2, VIE-4), drawn on a 2D canvas above the picture so they
// work over native video too: the tile sharpness heatmap and the motion vectors with the
// global-motion gizmo (pan/tilt arrow, zoom ring, rotation arc; opacity follows confidence).

import { useEffect, useRef } from "react";
import type { Inspection } from "../api";

export type MeasureOverlayKind = "none" | "tiles" | "motion";

/** Blue → green → yellow → red for 0–1. */
function heat(t: number): [number, number, number] {
  const stops: [number, number, number][] = [
    [40, 90, 255],
    [40, 200, 120],
    [250, 220, 40],
    [255, 70, 50],
  ];
  const x = Math.min(0.999, Math.max(0, t)) * (stops.length - 1);
  const i = Math.floor(x);
  const f = x - i;
  const a = stops[i]!;
  const b = stops[i + 1]!;
  return [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f];
}

function raw(ins: Inspection, key: string) {
  return ins.raw.find((r) => r.key === key)?.value ?? NaN;
}

function drawTiles(g: CanvasRenderingContext2D, w: number, h: number, ins: Inspection) {
  const { cols, rows, values } = ins.tiles;
  if (!cols || !rows || values.length < cols * rows) return;
  let lo = Infinity;
  let hi = -Infinity;
  for (const v of values) {
    if (Number.isFinite(v)) {
      lo = Math.min(lo, v);
      hi = Math.max(hi, v);
    }
  }
  const span = Math.max(0.25, hi - lo); // a flat frame shouldn't look like a heatmap of noise
  const cw = w / cols;
  const ch = h / rows;
  g.font = `${Math.max(9, Math.min(12, ch / 4))}px "JetBrains Mono", monospace`;
  g.textAlign = "center";
  g.textBaseline = "middle";
  for (let r = 0; r < rows; r++) {
    for (let c = 0; c < cols; c++) {
      const v = values[r * cols + c]!;
      if (!Number.isFinite(v)) continue;
      const t = (v - lo) / span;
      const [R, G, B] = heat(t);
      g.fillStyle = `rgba(${R | 0},${G | 0},${B | 0},${0.18 + 0.3 * t})`;
      g.fillRect(c * cw + 0.5, r * ch + 0.5, cw - 1, ch - 1);
      if (v === hi) {
        g.strokeStyle = "rgba(255,255,255,0.9)";
        g.lineWidth = 1.5;
        g.strokeRect(c * cw + 1.5, r * ch + 1.5, cw - 3, ch - 3);
      }
    }
  }
}

function arrow(g: CanvasRenderingContext2D, x: number, y: number, dx: number, dy: number, head: number) {
  const len = Math.hypot(dx, dy);
  if (len < 0.5) {
    g.fillRect(x - 1, y - 1, 2, 2);
    return;
  }
  g.beginPath();
  g.moveTo(x, y);
  g.lineTo(x + dx, y + dy);
  const a = Math.atan2(dy, dx);
  const hs = Math.min(head, len * 0.5);
  g.moveTo(x + dx, y + dy);
  g.lineTo(x + dx - hs * Math.cos(a - 0.45), y + dy - hs * Math.sin(a - 0.45));
  g.moveTo(x + dx, y + dy);
  g.lineTo(x + dx - hs * Math.cos(a + 0.45), y + dy - hs * Math.sin(a + 0.45));
  g.stroke();
}

function drawMotion(g: CanvasRenderingContext2D, w: number, h: number, ins: Inspection) {
  const { cols, rows, values } = ins.mvs;
  const sx = w / Math.max(1, ins.working.w);
  const sy = h / Math.max(1, ins.working.h);
  const gain = 3; // vectors are per frame; exaggerate so small moves are visible
  if (cols && rows && values.length >= cols * rows * 2) {
    g.strokeStyle = "rgba(120,220,255,0.85)";
    g.fillStyle = "rgba(120,220,255,0.6)";
    g.lineWidth = 1.25;
    const cw = w / cols;
    const ch = h / rows;
    for (let r = 0; r < rows; r++) {
      for (let c = 0; c < cols; c++) {
        const i = 2 * (r * cols + c);
        arrow(g, (c + 0.5) * cw, (r + 0.5) * ch, values[i]! * sx * gain, values[i + 1]! * sy * gain, 5);
      }
    }
  }
  // Global-motion gizmo.
  const conf = raw(ins, "cam_conf");
  const ux = raw(ins, "cam_u_x");
  const uy = raw(ins, "cam_u_y");
  const zr = raw(ins, "cam_zr");
  const wr = raw(ins, "cam_wr");
  if (!Number.isFinite(ux) || !Number.isFinite(uy)) return;
  const alpha = Number.isFinite(conf) ? 0.35 + 0.65 * Math.min(1, Math.max(0, conf)) : 0.5;
  const cx = w / 2;
  const cy = h / 2;
  const R = Math.min(w, h) * 0.12;
  g.strokeStyle = `rgba(255,214,10,${alpha})`;
  g.fillStyle = `rgba(255,214,10,${alpha})`;
  g.lineWidth = 2.5;
  arrow(g, cx, cy, ux * sx * gain * 2, uy * sy * gain * 2, 10);
  g.lineWidth = 1.5;
  g.beginPath();
  g.arc(cx, cy, R, 0, Math.PI * 2);
  g.stroke();
  if (Number.isFinite(zr) && Math.abs(zr) > 1e-4) {
    g.setLineDash([4, 4]);
    g.beginPath();
    g.arc(cx, cy, Math.max(4, R * (1 + Math.max(-0.8, Math.min(0.8, zr * 40)))), 0, Math.PI * 2);
    g.stroke();
    g.setLineDash([]);
  }
  if (Number.isFinite(wr) && Math.abs(wr) > 1e-4) {
    const sweep = Math.max(-Math.PI, Math.min(Math.PI, wr * 60));
    g.lineWidth = 3;
    g.beginPath();
    g.arc(cx, cy, R + 6, -Math.PI / 2, -Math.PI / 2 + sweep, sweep < 0);
    g.stroke();
  }
}

export function MeasureOverlay({ kind, data }: { kind: MeasureOverlayKind; data: Inspection | null }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const c = ref.current;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    const w = c.clientWidth;
    const h = c.clientHeight;
    c.width = Math.round(w * dpr);
    c.height = Math.round(h * dpr);
    const g = c.getContext("2d");
    if (!g) return;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, h);
    if (!data || kind === "none") return;
    if (kind === "tiles") drawTiles(g, w, h, data);
    else drawMotion(g, w, h, data);
  });
  return <canvas ref={ref} aria-hidden className="pointer-events-none absolute inset-0 h-full w-full" />;
}
