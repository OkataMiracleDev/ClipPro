import { describe, expect, it } from "vitest";
import type { ClipScore } from "../api";
import { nextRun } from "./flagRuns";

const clip = (id: number, flags: number[]): ClipScore => ({ clip: id, durationUs: flags.length * 250_000, rateHz: 4, bar: new Float32Array(flags.length), flags: Uint32Array.from(flags), segments: [] });

describe("nextRun", () => {
  const S = 1 << 5; // shaky
  const clips = [clip(1, [0, S, S, 0, S, 0]), clip(2, [0, 0, 0, 0]), clip(3, [S, 0, 0, 0])];
  const order = [1, 2, 3];

  it("finds the next run in the same clip, then the next clips, then wraps", () => {
    expect(nextRun(clips, order, 5, 1, 0)).toEqual({ clip: 1, us: 250_000 });
    expect(nextRun(clips, order, 5, 1, 250_000)).toEqual({ clip: 1, us: 1_000_000 });
    expect(nextRun(clips, order, 5, 1, 1_000_000)).toEqual({ clip: 3, us: 0 });
    expect(nextRun(clips, order, 5, 3, 0)).toEqual({ clip: 1, us: 250_000 });
  });

  it("returns null when the flag never occurs", () => {
    expect(nextRun(clips, order, 2, 1, 0)).toBeNull();
    expect(nextRun(clips, order, 5, null, 0)).toEqual({ clip: 1, us: 250_000 });
  });
});
