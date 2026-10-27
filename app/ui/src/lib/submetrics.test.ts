import { describe, expect, it } from "vitest";
import { amberGroups, setWeight, snapPref, type SubMetricInfo } from "./submetrics";

describe("weighting", () => {
  it("dragging a vertex to the centre gives 0% and rescales the rest", () => {
    const w = setWeight([35, 30, 25, 10, 0], 0, 0, false);
    expect(w[0]).toBe(0);
    expect(w.reduce((a, b) => a + b, 0)).toBeCloseTo(100, 0);
    expect(w[1]! / w[2]!).toBeCloseTo(30 / 25, 1);
  });

  it("Ctrl+drag locks the other groups", () => {
    expect(setWeight([35, 30, 25, 10, 0], 2, 60, true)).toEqual([35, 30, 60, 10, 0]);
  });

  it("clamps to 0–100", () => {
    expect(setWeight([50, 50, 0, 0, 0], 0, 140, true)[0]).toBe(100);
    expect(setWeight([50, 50, 0, 0, 0], 0, -5, true)[0]).toBe(0);
  });
});

describe("sub-metric sliders", () => {
  it("snaps to the Neutral detent", () => {
    expect(snapPref(0.04)).toBe(0);
    expect(snapPref(-0.05)).toBe(0);
    expect(snapPref(0.5)).toBe(0.5);
    expect(snapPref(3)).toBe(1);
  });

  it("marks a weighted group amber when all its sub-metrics are off", () => {
    const infos: SubMetricInfo[] = [
      { key: "sharpness", label: "Sharpness", group: "focus", target: false },
      { key: "luma", label: "Luma", group: "exposure", target: true },
    ];
    const subs = [
      { enabled: true, pref: 0, strength: 1 },
      { enabled: false, pref: 0, strength: 1 },
    ];
    expect([...amberGroups([50, 50, 0, 0, 0], subs, infos)].sort()).toEqual(["exposure", "focus"]);
    subs[0]!.pref = 0.7;
    expect([...amberGroups([50, 50, 0, 0, 0], subs, infos)]).toEqual(["exposure"]);
    expect([...amberGroups([50, 0, 0, 0, 0], subs, infos)]).toEqual([]);
  });
});
