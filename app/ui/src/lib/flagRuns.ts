// "Jump to next run" for flags (SPEC P9.3): the start of the next run of a flag after the playhead,
// in clip order, wrapping around to the first run.

import type { ClipScore } from "../api";

export interface RunPos {
  clip: number;
  us: number;
}

function runStarts(c: ClipScore, bit: number): number[] {
  const out: number[] = [];
  let on = false;
  for (let k = 0; k < c.flags.length; k++) {
    const now = (c.flags[k]! & bit) !== 0;
    if (now && !on) out.push(k);
    on = now;
  }
  return out;
}

/** Next run start strictly after (clip, us); `order` is the clip display order. */
export function nextRun(clips: ClipScore[], order: number[], flagIndex: number, clip: number | null, us: number): RunPos | null {
  const bit = 1 << flagIndex;
  const byId = new Map(clips.map((c) => [c.clip, c]));
  const seq = order.filter((id) => byId.has(id));
  if (!seq.length) return null;
  const start = clip === null ? -1 : seq.indexOf(clip);
  const at = (c: ClipScore, k: number) => (k / c.rateHz) * 1e6;
  for (let step = 0; step <= seq.length; step++) {
    const i = start < 0 ? step : (start + step) % seq.length;
    if (start < 0 && step === seq.length) break;
    const c = byId.get(seq[i]!)!;
    const starts = runStarts(c, bit);
    const first = step === 0 && start >= 0 ? starts.find((k) => at(c, k) > us + 1) : starts[0];
    if (first !== undefined) return { clip: c.clip, us: at(c, first) };
  }
  return null;
}
