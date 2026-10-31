import { describe, expect, it } from "vitest";
import { analysisProgress, FINISHED } from "./progress";

describe("analysisProgress", () => {
  it("counts a finishing clip fully instead of dropping to 0%", () => {
    // One clip: measured every sample, now in the audio/temporal pass.
    expect(analysisProgress({ 7: 1 }, 0, 1)).toEqual({ fraction: 1, finishing: true });
    expect(analysisProgress({ 7: 0.99 }, 0, 1)).toEqual({ fraction: 0.99, finishing: false });
  });

  it("counts finished jobs through their clips, not twice", () => {
    expect(analysisProgress({ 1: FINISHED, 2: 0.5 }, 1, 2)).toEqual({ fraction: 0.75, finishing: false });
    expect(analysisProgress({ 1: FINISHED, 2: 1 }, 1, 2)).toEqual({ fraction: 1, finishing: true });
    expect(analysisProgress({}, 0, 0)).toEqual({ fraction: 0, finishing: false });
  });
});
