import { describe, expect, it } from "vitest";
import { mockBackend } from "../mock";
import { History } from "./history";

describe("History", () => {
  it("100 random tuning ops then undo all returns the initial state; redo replays them", async () => {
    const initial = await mockBackend.presetSettings("All-Rounder");
    const h = new History<typeof initial>(500, 0);
    let cur = structuredClone(initial);
    let seed = 7;
    const rnd = () => ((seed = (seed * 1103515245 + 12345) >>> 0) / 2 ** 32);
    const states = [structuredClone(cur)];
    for (let i = 0; i < 100; i++) {
      h.record(cur, i * 1000);
      cur = structuredClone(cur);
      const op = Math.floor(rnd() * 4);
      if (op === 0) cur.score_lo = Math.round(rnd() * 100) / 100;
      else if (op === 1) cur.weights[Math.floor(rnd() * 4)] = Math.round(rnd() * 100);
      else if (op === 2) cur.subs[Math.floor(rnd() * 23)]!.pref = Math.round((rnd() * 2 - 1) * 100) / 100;
      else cur.tiers[Math.floor(rnd() * 10)] = (["neutral", "disfavour", "force_exclude"] as const)[Math.floor(rnd() * 3)]!;
      states.push(structuredClone(cur));
    }
    const final = structuredClone(cur);
    let steps = 0;
    for (let u = h.undo(cur); u; u = h.undo(cur)) {
      cur = u;
      steps++;
    }
    expect(steps).toBe(100);
    expect(cur).toEqual(initial);
    for (let r = h.redo(cur); r; r = h.redo(cur)) cur = r;
    expect(cur).toEqual(final);
  });

  it("coalesces a drag into one step and caps the depth", () => {
    const h = new History<number>(3, 400);
    let v = 0;
    for (let t = 0; t < 1000; t += 50) {
      h.record(v, t);
      v++;
    }
    expect(h.undo(v)).toBe(0);
    expect(h.canUndo).toBe(false);
    const c = new History<number>(3, 0);
    for (let i = 0; i < 10; i++) c.record(i, i * 10);
    let n = 0;
    while (c.undo(99) !== null) n++;
    expect(n).toBe(3);
  });

  it("a new edit after undo clears redo", () => {
    const h = new History<number>(10, 0);
    h.record(1, 0);
    expect(h.undo(2)).toBe(1);
    h.record(1, 100);
    expect(h.canRedo).toBe(false);
  });
});
