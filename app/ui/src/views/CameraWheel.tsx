// Camera-movement filter (SPEC TUN-6, §14.2, ALGORITHMS §6.5): a wheel whose puck sets the
// pan/tilt direction and how strict it is (centre ≈ any direction, edge ≈ exact), zoom and rotate
// preferences, Static-only and Mirror, and a live list of the five clips that match best.

import { useEffect, useRef, useState } from "react";
import { api, n, type Settings } from "../api";
import { cx, Switch } from "../components/ui";
import { snapPref } from "../lib/submetrics";
import { useApp } from "../store";
import { BipolarSlider } from "./Weighting";

const W = 150;
const C = W / 2;
const RR = 58;

type Camera = Settings["camera"];

/** Wheel puck ↔ (radius, direction°). 0° is a pan to the right, 90° a tilt up. */
export function puckToCamera(x: number, y: number): { radius: number; direction: number } {
  const dx = x - C;
  const dy = C - y;
  const r = Math.min(1, Math.hypot(dx, dy) / RR);
  const radius = r < 0.05 ? 0 : Math.round(r * 100) / 100;
  const direction = ((Math.round((Math.atan2(dy, dx) * 180) / Math.PI) % 360) + 360) % 360;
  return { radius, direction };
}

function Wheel({ cam, onChange }: { cam: Camera; onChange: (c: Partial<Camera>) => void }) {
  const svg = useRef<SVGSVGElement>(null);
  const r = n(cam.radius);
  const a = (n(cam.direction) * Math.PI) / 180;
  const px = C + Math.cos(a) * r * RR;
  const py = C - Math.sin(a) * r * RR;
  // Acceptance cone: half-angle lerp(90°, 5°, r) (ALGORITHMS §6.5).
  const half = ((90 - 85 * r) * Math.PI) / 180;
  const cone = (dir: number) => {
    const p1 = [C + Math.cos(dir - half) * RR, C - Math.sin(dir - half) * RR];
    const p2 = [C + Math.cos(dir + half) * RR, C - Math.sin(dir + half) * RR];
    return `M${C},${C} L${p1[0]},${p1[1]} A${RR},${RR} 0 0 0 ${p2[0]},${p2[1]} Z`;
  };
  const at = (e: { clientX: number; clientY: number }) => {
    const b = svg.current!.getBoundingClientRect();
    onChange(puckToCamera(((e.clientX - b.left) / b.width) * W, ((e.clientY - b.top) / b.height) * W));
  };
  const off = cam.static_only;
  return (
    <svg
      ref={svg}
      viewBox={`0 0 ${W} ${W}`}
      className={cx("h-[150px] w-[150px] shrink-0 touch-none select-none", off && "opacity-40")}
      role="slider"
      tabIndex={0}
      aria-label="Camera movement direction"
      aria-valuetext={r === 0 ? "Any direction" : `${Math.round(n(cam.direction))}°, ${Math.round(r * 100)}% strict`}
      onPointerDown={(e) => {
        if (off) return;
        (e.target as Element).setPointerCapture(e.pointerId);
        at(e);
      }}
      onPointerMove={(e) => !off && e.buttons === 1 && at(e)}
      onDoubleClick={() => onChange({ radius: 0 })}
      onKeyDown={(e) => {
        const step = e.shiftKey ? 15 : 5;
        if (e.key === "ArrowLeft") onChange({ direction: (n(cam.direction) + step) % 360 });
        else if (e.key === "ArrowRight") onChange({ direction: (n(cam.direction) - step + 360) % 360 });
        else if (e.key === "ArrowUp") onChange({ radius: Math.min(1, r + 0.05) });
        else if (e.key === "ArrowDown") onChange({ radius: Math.max(0, r - 0.05 < 0.05 ? 0 : r - 0.05) });
        else return;
        e.preventDefault();
      }}
    >
      <circle cx={C} cy={C} r={RR} fill="var(--c-control)" stroke="var(--c-separator-strong)" />
      {r > 0 && <path d={cone(a)} fill="var(--c-accent-soft)" />}
      {r > 0 && cam.mirror && <path d={cone(a + Math.PI)} fill="var(--c-accent-soft)" />}
      <line x1={C - RR} y1={C} x2={C + RR} y2={C} stroke="var(--c-separator)" />
      <line x1={C} y1={C - RR} x2={C} y2={C + RR} stroke="var(--c-separator)" />
      {[
        ["Pan →", C + RR - 2, C - 5, "end"],
        ["Tilt ↑", C + 4, C - RR + 10, "start"],
      ].map(([t, x, y, anchor]) => (
        <text key={t as string} x={x as number} y={y as number} fontSize="8.5" fill="var(--c-text-3)" textAnchor={anchor as "end" | "start"}>
          {t}
        </text>
      ))}
      <circle cx={px} cy={py} r={7} fill="var(--c-raised)" stroke="var(--c-accent)" strokeWidth={2} className="cursor-grab" />
    </svg>
  );
}

export function CameraFilterPanel() {
  const settings = useApp((s) => s.settings);
  const updateSettings = useApp((s) => s.updateSettings);
  const selectClip = useApp((s) => s.selectClip);
  const [matches, setMatches] = useState<{ clip: number; name: string; score: number }[]>([]);
  const cam = settings?.camera;
  const key = cam ? JSON.stringify(cam) : "";
  useEffect(() => {
    if (!settings || !cam?.enabled) {
      setMatches([]);
      return;
    }
    const t = setTimeout(() => {
      api.cameraMatches(settings, 5).then(setMatches, () => setMatches([]));
    }, 150);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  if (!settings || !cam) return null;
  const set = (patch: Partial<Camera>) => updateSettings((s) => ({ ...s, camera: { ...s.camera, ...patch } }));
  const dim = !settings.switches.camera_filter;

  return (
    <div className={cx("flex flex-col gap-3", dim && "opacity-40")}>
      <label className="flex items-center justify-between">
        <span className="text-[12px] text-text-2">Filter by camera movement</span>
        <Switch label="Filter by camera movement" checked={cam.enabled} onChange={(enabled) => set({ enabled })} />
      </label>
      {cam.enabled && (
        <>
          <div className="flex items-center gap-3">
            <Wheel cam={cam} onChange={set} />
            <div className="flex min-w-0 flex-1 flex-col gap-2">
              <label className="flex items-center justify-between gap-2">
                <span className="text-[12px] text-text-2">Static only</span>
                <Switch label="Static only" checked={cam.static_only} onChange={(static_only) => set({ static_only })} />
              </label>
              <label className="flex items-center justify-between gap-2" title="Accept the opposite direction too (left or right, up or down)">
                <span className="text-[12px] text-text-2">Mirror</span>
                <Switch label="Mirror" checked={cam.mirror} onChange={(mirror) => set({ mirror })} />
              </label>
              <div className="text-[10.5px] text-text-3">
                {cam.static_only ? "Only shots without camera movement" : n(cam.radius) === 0 ? "Any pan/tilt direction" : `${Math.round(n(cam.direction))}° · ${Math.round(n(cam.radius) * 100)}% strict`}
              </div>
            </div>
          </div>
          <div className={cx("flex flex-col gap-2", cam.static_only && "pointer-events-none opacity-40")}>
            <span className="text-[12px] text-text-2">Zoom</span>
            <BipolarSlider label="Zoom" value={n(cam.zoom)} ends={["Out", "In"]} onChange={(zoom) => set({ zoom: snapPref(zoom) })} />
            <span className="text-[12px] text-text-2">Rotate</span>
            <BipolarSlider label="Rotate" value={n(cam.rotate)} ends={["Clockwise", "Anticlockwise"]} onChange={(rotate) => set({ rotate: snapPref(rotate) })} />
          </div>
          <div className="flex flex-col gap-1">
            <span className="section-header">Best matches</span>
            {matches.length === 0 ? (
              <span className="text-[11px] text-text-3">No analysed clips yet</span>
            ) : (
              matches.map((m) => (
                <button key={m.clip} onClick={() => selectClip(m.clip)} className="flex items-center gap-2 rounded px-1 py-0.5 text-left text-[12px] hover:bg-control-hover">
                  <span className="min-w-0 flex-1 truncate">{m.name}</span>
                  <span className="h-1.5 w-12 overflow-hidden rounded-full bg-separator-strong">
                    <span className="block h-full rounded-full bg-accent" style={{ width: `${Math.round(m.score * 100)}%` }} />
                  </span>
                  <span className="w-7 text-right font-mono text-[10.5px] text-text-3 tabular">{Math.round(m.score * 100)}</span>
                </button>
              ))
            )}
          </div>
        </>
      )}
    </div>
  );
}
