import { describe, expect, it } from "vitest";
import type { Annotation, Clip, ClipScore } from "../api";
import { factsFor, matches, type SmartQuery } from "./smart";

const clip = (id: number, over: Partial<Clip> = {}): Clip => ({
  clip: id,
  media: id,
  name: `C${id}`,
  path: "",
  state: "done",
  progress: 1,
  error: null,
  profile: "rec709",
  bin: null,
  camera: null,
  created: null,
  codec: "h264",
  ...over,
});
const score = (id: number, best: number, dur: number, flags: number[] = []): ClipScore => ({
  clip: id,
  durationUs: dur * 1e6,
  rateHz: 4,
  bar: new Float32Array(1),
  flags: Uint32Array.from(flags),
  segments: best > 0 ? [{ startUs: 0, endUs: 1e6, inUs: 0, outUs: 1e6, mean: best, firstSample: 0, lastSample: 3, kind: "auto" }] : [],
});

describe("smart collections", () => {
  const clips = [clip(1, { camera: "Sony FX6", created: "2026-10-12T09:00:00Z" }), clip(2, { codec: "prores", created: "2026-10-20T10:00:00Z" }), clip(3)];
  const scored = { clips: [score(1, 0.8, 30), score(2, 0.4, 5, [1 << 5]), score(3, 0, 60)] };
  const anns: Annotation[] = [{ clip: 2, startUs: 0, endUs: 1e6, rating: 4, label: "blue", note: "" }];
  const facts = factsFor(clips, scored, anns);
  const pick = (q: SmartQuery) => facts.filter((f) => matches(q, f)).map((f) => f.clip.clip);

  it("matches each field", () => {
    expect(pick({ match: "all", rules: [{ field: "flag", op: "has", value: "shaky" }] })).toEqual([2]);
    expect(pick({ match: "all", rules: [{ field: "flag", op: "lacks", value: "shaky" }] })).toEqual([1, 3]);
    expect(pick({ match: "all", rules: [{ field: "score", op: ">=", value: 70 }] })).toEqual([1]);
    expect(pick({ match: "all", rules: [{ field: "duration", op: "<=", value: 30 }] })).toEqual([1, 2]);
    expect(pick({ match: "all", rules: [{ field: "selects", op: "<=", value: 0 }] })).toEqual([3]);
    expect(pick({ match: "all", rules: [{ field: "camera", op: "contains", value: "fx6" }] })).toEqual([1]);
    expect(pick({ match: "all", rules: [{ field: "codec", op: "is", value: "ProRes" }] })).toEqual([2]);
    expect(pick({ match: "all", rules: [{ field: "rating", op: ">=", value: 3 }] })).toEqual([2]);
    expect(pick({ match: "all", rules: [{ field: "label", op: "is", value: "blue" }] })).toEqual([2]);
    expect(pick({ match: "all", rules: [{ field: "date", op: "after", value: "2026-10-15" }] })).toEqual([2]);
  });

  it("combines with all / any, and an empty query matches everything", () => {
    const rules = [
      { field: "score", op: ">=", value: 70 },
      { field: "codec", op: "is", value: "prores" },
    ] as SmartQuery["rules"];
    expect(pick({ match: "all", rules })).toEqual([]);
    expect(pick({ match: "any", rules })).toEqual([1, 2]);
    expect(pick({ match: "all", rules: [] })).toEqual([1, 2, 3]);
  });
});
