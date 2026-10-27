// Inspector data for the sample under the playhead, shared by the viewer overlays and the
// Frame panel. Requests are cached per scoring result, so a settings change refetches and
// returning to a sample doesn't.

import { useEffect, useState } from "react";
import { api, type ClipScore, type Inspection, type Scored } from "../api";
import { useApp } from "../store";

const cache = new WeakMap<Scored, Map<string, Promise<Inspection>>>();
const MAX_ENTRIES = 256;

export function sampleAt(sc: ClipScore, us: number): number {
  return Math.min(sc.bar.length - 1, Math.max(0, Math.round((us / 1e6) * sc.rateHz)));
}

function get(scored: Scored, clip: number, sample: number): Promise<Inspection> {
  const { settings } = useApp.getState();
  let m = cache.get(scored);
  if (!m) {
    m = new Map();
    cache.set(scored, m);
  }
  const key = `${clip}:${sample}`;
  let p = m.get(key);
  if (!p && settings) {
    p = api.inspect(settings, clip, sample);
    p.catch(() => m.delete(key));
    m.set(key, p);
    if (m.size > MAX_ENTRIES) m.delete(m.keys().next().value!);
  }
  return p ?? Promise.reject(new Error("No settings"));
}

/** Inspection of the selected clip's sample at the playhead (null while loading or idle). */
export function useInspection(enabled = true): { data: Inspection | null; error: string | null } {
  const scored = useApp((s) => s.scored);
  const clip = useApp((s) => s.selectedClip);
  const sc = scored?.clips.find((c) => c.clip === clip);
  const sample = useApp((s) => (sc ? sampleAt(sc, s.playheadUs) : -1));
  const [state, setState] = useState<{ data: Inspection | null; error: string | null }>({ data: null, error: null });

  useEffect(() => {
    if (!enabled || !scored || clip === null || sample < 0 || !sc?.bar.length) {
      setState({ data: null, error: null });
      return;
    }
    let live = true;
    get(scored, clip, sample).then(
      (data) => live && setState({ data, error: null }),
      (e) => live && setState({ data: null, error: String(e instanceof Error ? e.message : e) }),
    );
    return () => {
      live = false;
    };
  }, [enabled, scored, clip, sample, sc]);

  // Keep showing the previous sample's data while the next one loads (no flicker in playback).
  return state.data && (state.data.clip !== clip || !enabled) ? { data: null, error: null } : state;
}
