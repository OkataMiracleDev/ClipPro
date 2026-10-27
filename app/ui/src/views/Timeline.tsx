// Project timeline (SPEC TML-1, TML-3): clips end to end with per-sample score bars, segments
// and flag lanes. Selects-only packs the selects together. Wheel scrolls, Ctrl+wheel zooms.

import { Maximize2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { ClipScore, Select } from "../api";
import { IconButton } from "../components/ui";
import { cssVar, scoreRamp, setupCanvas, useSize } from "../lib/canvas";
import { formatClock } from "../lib/typed";
import { useApp } from "../store";

const RULER = 18;
const NAME = 16;
const BAR = 44;
const FLAGS = 10;
const GAP_PX = 6;

/** Flag bits drawn in the flag lane, with their colours. */
const FLAG_LANE: [number, string][] = [
  [1 << 5, "#ff9f0a"], // shaky
  [1 << 4, "#bf5af2"], // motion blur
  [1 << 0, "#ff453a"], // clipping
  [1 << 1, "#8e8e93"], // duplicate
  [1 << 2, "#636366"], // solid colour
  [1 << 6, "#5e5ce6"], // low light
  [1 << 7, "#64d2ff"], // focus hunting
];

interface Block {
  clip: number;
  name: string;
  /** Position on the timeline (µs, excluding gaps). */
  at: number;
  /** Source range within the clip. */
  src0: number;
  src1: number;
  score: ClipScore | undefined;
  selectIndex: number | null;
}

export function Timeline() {
  const { clips, scored, selectsOnly, selectedClip, selectedSelect, playheadUs, setPlayhead, selectSelect } = useApp();
  const [wrap, size] = useSize<HTMLDivElement>();
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const [pxPerSec, setPxPerSec] = useState<number | null>(null);
  const [scroll, setScroll] = useState(0);

  const blocks = useMemo<Block[]>(() => {
    const byClip = new Map((scored?.clips ?? []).map((c) => [c.clip, c]));
    const names = new Map(clips.map((c) => [c.clip, c.name]));
    const out: Block[] = [];
    let at = 0;
    if (selectsOnly) {
      (scored?.selects ?? []).forEach((s: Select, i) => {
        const d = s.segment.outUs - s.segment.inUs;
        out.push({ clip: s.clip, name: names.get(s.clip) ?? "", at, src0: s.segment.inUs, src1: s.segment.outUs, score: byClip.get(s.clip), selectIndex: i });
        at += d;
      });
    } else {
      for (const c of clips) {
        const sc = byClip.get(c.clip);
        const d = sc?.durationUs ?? 0;
        if (d <= 0) continue;
        out.push({ clip: c.clip, name: c.name, at, src0: 0, src1: d, score: sc, selectIndex: null });
        at += d;
      }
    }
    return out;
  }, [clips, scored, selectsOnly]);

  const totalUs = blocks.reduce((a, b) => a + (b.src1 - b.src0), 0);
  const fit = size.w > 0 && totalUs > 0 ? Math.max(0.5, (size.w - 16 - blocks.length * GAP_PX) / (totalUs / 1e6)) : 10;
  const pps = pxPerSec ?? fit;
  const xOf = (b: Block, i: number, us: number) => 8 + i * GAP_PX + ((b.at + (us - b.src0)) / 1e6) * pps - scroll;

  useEffect(() => {
    setPxPerSec(null);
    setScroll(0);
  }, [selectsOnly]);

  useEffect(() => {
    const c = canvas.current;
    if (!c || size.w < 10) return;
    const ctx = setupCanvas(c, size.w, size.h);
    if (!ctx) return;
    const ramp = scoreRamp();
    const text2 = cssVar("--c-text-2");
    const text3 = cssVar("--c-text-3");
    const sep = cssVar("--c-separator");
    const accent = cssVar("--c-accent");
    const raised = cssVar("--c-raised");
    const barTop = RULER + NAME;
    // Ruler
    ctx.fillStyle = text3;
    ctx.font = "10px 'JetBrains Mono Variable', monospace";
    const tickSec = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 1800, 3600].find((t) => t * pps >= 70) ?? 3600;
    for (let t = 0; t <= totalUs / 1e6; t += tickSec) {
      // Map timeline seconds to x, accounting for block gaps.
      const us = t * 1e6;
      let i = blocks.findIndex((b) => b.at + (b.src1 - b.src0) >= us);
      if (i < 0) i = blocks.length - 1;
      const b = blocks[i];
      if (!b) break;
      const x = xOf(b, i, b.src0 + (us - b.at));
      if (x < -50 || x > size.w + 50) continue;
      ctx.fillRect(Math.round(x), RULER - 5, 1, 5);
      ctx.fillText(formatClock(us), Math.round(x) + 3, RULER - 6);
    }
    ctx.fillStyle = sep;
    ctx.fillRect(0, RULER, size.w, 1);

    blocks.forEach((b, i) => {
      const x0 = xOf(b, i, b.src0);
      const x1 = xOf(b, i, b.src1);
      if (x1 < 0 || x0 > size.w) return;
      const w = Math.max(1, x1 - x0);
      const active = selectsOnly ? b.selectIndex === selectedSelect : b.clip === selectedClip;
      // Block background + name
      ctx.fillStyle = raised;
      ctx.fillRect(x0, barTop, w, BAR + FLAGS);
      ctx.save();
      ctx.beginPath();
      ctx.rect(x0, RULER, w, NAME + BAR + FLAGS);
      ctx.clip();
      ctx.fillStyle = active ? text2 : text3;
      ctx.font = "11px 'Inter Variable', sans-serif";
      ctx.fillText(b.name, x0 + 4, RULER + 12);
      // Score bar: one column per pixel, mean of the samples it covers
      const sc = b.score;
      if (sc && sc.bar.length) {
        const usPerSample = 1e6 / sc.rateHz;
        for (let px = Math.max(0, Math.floor(x0)); px < Math.min(size.w, Math.ceil(x1)); px++) {
          const us0 = b.src0 + ((px - x0) / pps) * 1e6;
          const us1 = us0 + 1e6 / pps;
          const k0 = Math.max(0, Math.floor(us0 / usPerSample));
          const k1 = Math.min(sc.bar.length - 1, Math.max(k0, Math.floor(us1 / usPerSample)));
          let sum = 0;
          let cnt = 0;
          let fl = 0;
          for (let k = k0; k <= k1; k++) {
            const v = sc.bar[k]!;
            if (!Number.isNaN(v)) {
              sum += v;
              cnt++;
            }
            fl |= sc.flags[k] ?? 0;
          }
          if (cnt) {
            const v = sum / cnt;
            const h = Math.max(2, v * (BAR - 4));
            ctx.fillStyle = ramp[Math.round(v * 255)]!;
            ctx.fillRect(px, barTop + BAR - h, 1, h);
          }
          let lane = 0;
          for (const [bit, col] of FLAG_LANE) {
            if (fl & bit) {
              ctx.fillStyle = col;
              ctx.fillRect(px, barTop + BAR + 2 + lane * 2, 1, 2);
              lane++;
              if (lane > 3) break;
            }
          }
        }
        // Segments (normal mode): outline the selects inside the clip
        if (!selectsOnly) {
          ctx.strokeStyle = accent;
          ctx.lineWidth = 1.5;
          for (const s of sc.segments) {
            const sx0 = xOf(b, i, s.startUs);
            const sx1 = xOf(b, i, s.endUs);
            ctx.strokeRect(sx0 + 0.75, barTop + 0.75, Math.max(1, sx1 - sx0 - 1.5), BAR - 1.5);
          }
        }
      }
      ctx.restore();
      if (active) {
        ctx.strokeStyle = selectsOnly ? "#64d2ff" : accent;
        ctx.lineWidth = 2;
        ctx.strokeRect(x0 + 1, barTop + 1, w - 2, BAR + FLAGS - 2);
      }
    });
    // Playhead
    const pi = blocks.findIndex((b) => b.clip === selectedClip && playheadUs >= b.src0 && playheadUs <= b.src1);
    const pb = blocks[pi];
    if (pb) {
      const x = Math.round(xOf(pb, pi, playheadUs)) + 0.5;
      ctx.strokeStyle = "#fff";
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(x, 4);
      ctx.lineTo(x, barTop + BAR + FLAGS);
      ctx.stroke();
    }
  });

  const hit = (px: number) => {
    for (let i = 0; i < blocks.length; i++) {
      const b = blocks[i]!;
      const x0 = xOf(b, i, b.src0);
      const x1 = xOf(b, i, b.src1);
      if (px >= x0 && px <= x1) return { b, us: b.src0 + ((px - x0) / pps) * 1e6 };
    }
    return null;
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between px-3 pt-1.5 text-[12px]">
        <span className="font-medium text-text-2">{selectsOnly ? "Selects only" : "Timeline"}</span>
        <IconButton label="Fit to window" onClick={() => {
          setPxPerSec(null);
          setScroll(0);
        }}>
          <Maximize2 size={13} />
        </IconButton>
      </div>
      <div ref={wrap} className={`relative min-h-0 flex-1 ${selectsOnly ? "ring-2 ring-[#64d2ff]/60 ring-inset" : ""}`}>
        <canvas
          ref={canvas}
          className="absolute inset-0"
          role="img"
          aria-label="Timeline of clips with score bars"
          onWheel={(e) => {
            if (e.ctrlKey) {
              const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
              const mx = e.clientX - r.left;
              const next = Math.min(2000, Math.max(fit, pps * (e.deltaY < 0 ? 1.15 : 1 / 1.15)));
              setScroll((sc) => Math.max(0, ((sc + mx - 8) * next) / pps - mx + 8));
              setPxPerSec(next);
            } else {
              const maxScroll = Math.max(0, (totalUs / 1e6) * pps + blocks.length * GAP_PX + 16 - size.w);
              setScroll((sc) => Math.min(maxScroll, Math.max(0, sc + e.deltaY + e.deltaX)));
            }
          }}
          onPointerDown={(e) => {
            const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
            const h = hit(e.clientX - r.left);
            if (!h) return;
            // Alt-click a select: compare it with the one under the playhead (P6.6).
            if (e.altKey && h.b.selectIndex !== null) {
              const sel = scored?.selects[h.b.selectIndex];
              const st = useApp.getState();
              const own = scored?.clips.find((c) => c.clip === st.selectedClip)?.segments.find((s) => st.playheadUs >= s.startUs && st.playheadUs < s.endUs);
              if (sel && st.selectedClip !== null) st.setCompare({ clip: sel.clip, startUs: sel.segment.startUs, endUs: sel.segment.endUs, anchorUs: own?.startUs ?? st.playheadUs });
              return;
            }
            if (h.b.selectIndex !== null) selectSelect(h.b.selectIndex);
            setPlayhead(h.b.clip, h.us);
          }}
        />
        {!blocks.length && <div className="absolute inset-0 grid place-items-center text-[12px] text-text-3">{selectsOnly ? "No selects" : "Clips appear here once analysed"}</div>}
      </div>
    </div>
  );
}
