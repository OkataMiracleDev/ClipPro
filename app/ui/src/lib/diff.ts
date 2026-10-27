// Preset diff (SPEC P9.4): what the current tuning changes relative to the preset it came from,
// in plain words ("Score threshold 55–100 → 60–100").

import { n, type Settings } from "../api";
import { FLAGS, TIERS } from "./domain";
import { GROUPS, type SubMetricInfo } from "./submetrics";

const pct = (v: number) => `${Math.round(v * 100)}`;
const dur = (v: number | null | undefined) => (v === null || v === undefined ? "none" : `${Math.round(v * 100) / 100}s`);
const tier = (t: string) => TIERS.find((x) => x.value === t)?.label ?? t;
const onOff = (b: boolean) => (b ? "on" : "off");

export interface Change {
  what: string;
  from: string;
  to: string;
}

export function diffSettings(base: Settings, cur: Settings, infos: SubMetricInfo[]): Change[] {
  const out: Change[] = [];
  const push = (what: string, a: string, b: string) => a !== b && out.push({ what, from: a, to: b });
  push("Score threshold", `${pct(n(base.score_lo))}–${pct(n(base.score_hi))}`, `${pct(n(cur.score_lo))}–${pct(n(cur.score_hi))}`);
  push("Segment length", `${dur(base.len_min_s)}–${dur(base.len_max_s)}`, `${dur(cur.len_min_s)}–${dur(cur.len_max_s)}`);
  push("Max per clip", base.max_per_clip == null ? "none" : String(base.max_per_clip), cur.max_per_clip == null ? "none" : String(cur.max_per_clip));
  GROUPS.forEach((g, i) => push(`${g.label} weight`, `${Math.round(n(base.weights[i]))}%`, `${Math.round(n(cur.weights[i]))}%`));
  infos.forEach((m, i) => {
    const a = base.subs[i];
    const b = cur.subs[i];
    if (!a || !b) return;
    const fmt = (s: typeof a) => (!s.enabled ? "off" : m.target ? `target ${Math.round(n(s.target) * 100) / 100} ±${Math.round(n(s.tol) * 100) / 100}` : `${n(s.pref) > 0 ? "+" : ""}${pct(n(s.pref))}`);
    push(m.label, fmt(a), fmt(b));
  });
  FLAGS.forEach((f, i) => push(`${f.label} flag`, tier(base.tiers[i] ?? "neutral"), tier(cur.tiers[i] ?? "neutral")));
  const cam = (c: Settings["camera"]) =>
    !c.enabled ? "off" : c.static_only ? "static only" : `${Math.round(n(c.direction))}° ${pct(n(c.radius))}%${c.mirror ? " mirrored" : ""}, zoom ${pct(n(c.zoom))}, rotate ${pct(n(c.rotate))}`;
  push("Camera filter", cam(base.camera), cam(cur.camera));
  (["metrics", "flags", "camera_filter"] as const).forEach((k) => push(`${k === "camera_filter" ? "Camera" : k[0]!.toUpperCase() + k.slice(1)} switch`, onOff(base.switches[k]), onOff(cur.switches[k])));
  push("Gap tolerance", dur(base.gap_s), dur(cur.gap_s));
  push("Centre weighting", `${pct(n(base.centre_weighting))}%`, `${pct(n(cur.centre_weighting))}%`);
  push("Stability emphasis", `${pct(n(base.stability_emphasis))}%`, `${pct(n(cur.stability_emphasis))}%`);
  push("Handles", `${base.handles_samples}`, `${cur.handles_samples}`);
  return out;
}
