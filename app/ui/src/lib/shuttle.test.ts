import { describe, expect, it } from "vitest";
import { shuttle } from "./shuttle";

describe("shuttle", () => {
  it("L plays forward and doubles up to 8×", () => {
    let s = { playing: false, rate: 1 };
    const rates = [];
    for (let i = 0; i < 5; i++) {
      s = shuttle("l", s.playing, s.rate);
      rates.push(s.rate);
    }
    expect(rates).toEqual([1, 2, 4, 8, 8]);
    expect(s.playing).toBe(true);
  });

  it("J reverses and doubles down to −8×", () => {
    let s = { playing: true, rate: 4 };
    const rates = [];
    for (let i = 0; i < 5; i++) {
      s = shuttle("j", s.playing, s.rate);
      rates.push(s.rate);
    }
    expect(rates).toEqual([-1, -2, -4, -8, -8]);
  });

  it("K stops and resets to 1×; L from reverse goes to 1×", () => {
    expect(shuttle("k", true, -4)).toEqual({ playing: false, rate: 1 });
    expect(shuttle("l", true, -2)).toEqual({ playing: true, rate: 1 });
  });
});
