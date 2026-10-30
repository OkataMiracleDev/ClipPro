// Smart collections (SPEC P9.8): saved queries over clips — flags, score range, duration, camera,
// date, rating, label, name, codec — matched all-or-any.

import type { Annotation, Clip, ClipAi, ClipScore, ShotType } from "../api";
import { FLAGS } from "./domain";

export type Rule =
  | { field: "flag"; op: "has" | "lacks"; value: string }
  | { field: "score" | "duration" | "selects" | "rating" | "speakers"; op: ">=" | "<="; value: number }
  | { field: "camera" | "name"; op: "contains"; value: string }
  | { field: "codec" | "label"; op: "is"; value: string }
  | { field: "date"; op: "after" | "before"; value: string }
  | { field: "person"; op: "is" | "not"; value: number; name: string }
  | { field: "shot"; op: "is" | "not"; value: ShotType }
  | { field: "setting"; op: "is"; value: "indoor" | "outdoor" | "day" | "night" };

export const SHOT_TYPES: { value: ShotType; label: string }[] = [
  { value: "wide", label: "Wide" },
  { value: "medium", label: "Medium" },
  { value: "close_up", label: "Close-up" },
  { value: "extreme_close_up", label: "Extreme close-up" },
  { value: "insert", label: "Insert" },
  { value: "aerial", label: "Aerial" },
  { value: "over_the_shoulder", label: "Over the shoulder" },
];

export interface SmartQuery {
  match: "all" | "any";
  rules: Rule[];
}

export interface ClipFacts {
  clip: Clip;
  score?: ClipScore;
  annotations: Annotation[];
  /** AI results, when the AI modules have run on the clip. */
  ai?: ClipAi;
}

export function factsFor(clips: Clip[], scored: { clips: ClipScore[] } | null, annotations: Annotation[], ai?: Map<number, ClipAi>): ClipFacts[] {
  const sc = new Map(scored?.clips.map((c) => [c.clip, c]) ?? []);
  return clips.map((clip) => ({ clip, score: sc.get(clip.clip), annotations: annotations.filter((a) => a.clip === clip.clip), ai: ai?.get(clip.clip) }));
}

function flagBits(sc: ClipScore | undefined): number {
  let b = 0;
  if (sc) for (const f of sc.flags) b |= f;
  return b;
}

export function matchRule(r: Rule, f: ClipFacts): boolean {
  const sc = f.score;
  const cmp = (v: number, op: ">=" | "<=", x: number) => (op === ">=" ? v >= x : v <= x);
  switch (r.field) {
    case "flag": {
      const i = FLAGS.findIndex((x) => x.key === r.value);
      const has = i >= 0 && (flagBits(sc) & (1 << i)) !== 0;
      return r.op === "has" ? has : !has;
    }
    case "score": {
      const best = sc ? sc.segments.reduce((a, s) => Math.max(a, s.mean), -Infinity) : -Infinity;
      return Number.isFinite(best) && cmp(best * 100, r.op, r.value);
    }
    case "duration":
      return sc !== undefined && cmp(sc.durationUs / 1e6, r.op, r.value);
    case "selects":
      return cmp(sc?.segments.length ?? 0, r.op, r.value);
    case "rating": {
      const best = f.annotations.reduce((a, x) => Math.max(a, x.rating), 0);
      return cmp(best, r.op, r.value);
    }
    case "speakers":
      // Only clips with speaker turns (the speakers module ran and heard speech).
      return f.ai !== undefined && f.ai.speakers > 0 && cmp(f.ai.speakers, r.op, r.value);
    case "camera":
      return (f.clip.camera ?? "").toLowerCase().includes(r.value.trim().toLowerCase());
    case "name":
      return f.clip.name.toLowerCase().includes(r.value.trim().toLowerCase());
    case "codec":
      return f.clip.codec.toLowerCase() === r.value.trim().toLowerCase();
    case "label":
      return f.annotations.some((a) => a.label === r.value);
    case "date": {
      if (!f.clip.created) return false;
      const d = f.clip.created.slice(0, 10);
      return r.op === "after" ? d >= r.value : d <= r.value;
    }
    case "person": {
      // Clips the faces module hasn't seen match neither condition.
      if (!f.ai?.facesDone) return false;
      const has = f.ai.people.includes(r.value);
      return r.op === "is" ? has : !has;
    }
    case "setting": {
      const p = r.value === "indoor" || r.value === "outdoor" ? f.ai?.outdoor : f.ai?.night;
      if (p === null || p === undefined) return false;
      return r.value === "outdoor" || r.value === "night" ? p >= 0.5 : p < 0.5;
    }
    case "shot": {
      if (!f.ai?.shot) return false;
      return r.op === "is" ? f.ai.shot === r.value : f.ai.shot !== r.value;
    }
  }
}

export function matches(q: SmartQuery, f: ClipFacts): boolean {
  if (!q.rules.length) return true;
  return q.match === "all" ? q.rules.every((r) => matchRule(r, f)) : q.rules.some((r) => matchRule(r, f));
}

export function describeRule(r: Rule): string {
  switch (r.field) {
    case "flag":
      return `${r.op === "has" ? "Has" : "No"} ${FLAGS.find((f) => f.key === r.value)?.label.toLowerCase() ?? r.value}`;
    case "score":
      return `Best score ${r.op === ">=" ? "≥" : "≤"} ${r.value}`;
    case "duration":
      return `Length ${r.op === ">=" ? "≥" : "≤"} ${r.value}s`;
    case "selects":
      return `Selects ${r.op === ">=" ? "≥" : "≤"} ${r.value}`;
    case "rating":
      return `Rating ${r.op === ">=" ? "≥" : "≤"} ${r.value}★`;
    case "speakers":
      return `Speakers ${r.op === ">=" ? "≥" : "≤"} ${r.value}`;
    case "camera":
      return `Camera contains “${r.value}”`;
    case "name":
      return `Name contains “${r.value}”`;
    case "codec":
      return `Codec is ${r.value}`;
    case "label":
      return `Labelled ${r.value}`;
    case "date":
      return `Recorded ${r.op} ${r.value}`;
    case "person":
      return `${r.name || "Person"} ${r.op === "is" ? "appears" : "doesn't appear"}`;
    case "setting":
      return `${r.value[0]!.toUpperCase()}${r.value.slice(1)}`;
    case "shot":
      return `Shot ${r.op === "is" ? "is" : "isn't"} ${SHOT_TYPES.find((s) => s.value === r.value)?.label.toLowerCase() ?? r.value}`;
  }
}
