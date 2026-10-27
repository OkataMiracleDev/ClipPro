// Luma waveform monitor panel (P6.3): draws the latest waveform from the viewer with an IRE
// graticule. Source luma, before any colour transform.

import { useEffect, useRef } from "react";
import { scopeBus, type Waveform } from "../lib/scope";

const W = 256;
const H = 128;

function paint(c: HTMLCanvasElement, wf: Waveform) {
  const g = c.getContext("2d");
  if (!g) return;
  const img = g.createImageData(wf.cols, wf.rows);
  const norm = Math.log1p(wf.max);
  for (let i = 0; i < wf.counts.length; i++) {
    const n = wf.counts[i]!;
    if (!n) continue;
    const t = Math.log1p(n) / norm;
    img.data[i * 4] = 120 + 135 * t;
    img.data[i * 4 + 1] = 255;
    img.data[i * 4 + 2] = 140 + 115 * t;
    img.data[i * 4 + 3] = Math.round(70 + 185 * t);
  }
  c.width = wf.cols;
  c.height = wf.rows;
  g.putImageData(img, 0, 0);
}

export function WaveformScope() {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => scopeBus.listen((wf) => ref.current && paint(ref.current, wf)), []);
  return (
    <div className="pointer-events-none absolute right-3 bottom-3 rounded-md bg-black/70 p-1.5 backdrop-blur" aria-label="Luma waveform">
      <div className="relative" style={{ width: W, height: H }}>
        {[100, 75, 50, 25, 0].map((ire) => (
          <div key={ire} className="absolute inset-x-0 border-t border-white/15" style={{ top: `${100 - ire}%` }}>
            <span className="absolute -top-2 left-0.5 font-mono text-[8px] text-white/45">{ire}</span>
          </div>
        ))}
        <canvas ref={ref} width={W} height={H} className="absolute inset-0 h-full w-full" style={{ imageRendering: "pixelated" }} />
      </div>
    </div>
  );
}
