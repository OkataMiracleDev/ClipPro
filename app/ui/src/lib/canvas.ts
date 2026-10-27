import { useEffect, useRef, useState } from "react";

/** Tracks an element's size (CSS px) with a ResizeObserver. */
export function useSize<T extends HTMLElement>() {
  const ref = useRef<T | null>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => {
      if (e) setSize({ w: Math.floor(e.contentRect.width), h: Math.floor(e.contentRect.height) });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return [ref, size] as const;
}

/** Prepares a HiDPI canvas and returns its 2D context scaled to CSS pixels. */
export function setupCanvas(c: HTMLCanvasElement, w: number, h: number): CanvasRenderingContext2D | null {
  const dpr = window.devicePixelRatio || 1;
  if (c.width !== Math.round(w * dpr) || c.height !== Math.round(h * dpr)) {
    c.width = Math.round(w * dpr);
    c.height = Math.round(h * dpr);
    c.style.width = `${w}px`;
    c.style.height = `${h}px`;
  }
  const ctx = c.getContext("2d");
  if (!ctx) return null;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  return ctx;
}

/** Reads a CSS custom property from :root (canvas can't use var()). */
export function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || "#888";
}

/** Resolves any CSS colour (including oklch) to RGB by painting one pixel. */
function parseColor(c: string): [number, number, number] {
  const cv = document.createElement("canvas");
  cv.width = cv.height = 1;
  const ctx = cv.getContext("2d", { willReadFrequently: true });
  if (!ctx) return [128, 128, 128];
  ctx.fillStyle = "#808080";
  ctx.fillStyle = c;
  ctx.fillRect(0, 0, 1, 1);
  const d = ctx.getImageData(0, 0, 1, 1).data;
  return [d[0] ?? 128, d[1] ?? 128, d[2] ?? 128];
}

let rampCache: { key: string; ramp: string[] } | null = null;

/** Score colour ramp for canvas: precomputed 256-entry lookup from the CSS tokens. */
export function scoreRamp(): string[] {
  const key = cssVar("--c-score-lo") + cssVar("--c-score-mid") + cssVar("--c-score-hi");
  if (rampCache && rampCache.key === key) return rampCache.ramp;
  const lo = parseColor(cssVar("--c-score-lo"));
  const mid = parseColor(cssVar("--c-score-mid"));
  const hi = parseColor(cssVar("--c-score-hi"));
  const out: string[] = [];
  for (let i = 0; i < 256; i++) {
    const t = i / 255;
    const [a, b, k] = t < 0.5 ? [lo, mid, t * 2] : [mid, hi, (t - 0.5) * 2];
    const c = a.map((x, j) => Math.round(x + (b[j]! - x) * k));
    out.push(`rgb(${c[0]},${c[1]},${c[2]})`);
  }
  rampCache = { key, ramp: out };
  return out;
}
