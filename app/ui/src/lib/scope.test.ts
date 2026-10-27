import { describe, expect, it } from "vitest";
import { rgbaToLuma, waveform } from "./scope";

describe("waveform", () => {
  it("puts a left-dark / right-bright frame at the bottom left and top right", () => {
    const w = 64;
    const h = 8;
    const y = new Uint8Array(w * h);
    for (let r = 0; r < h; r++) for (let x = 0; x < w; x++) y[r * w + x] = x < w / 2 ? 16 : 235;
    const wf = waveform(y, w, h, { cols: 8, rows: 11, limited: true, step: 1 });
    const at = (row: number, col: number) => wf.counts[row * wf.cols + col]!;
    expect(at(10, 0)).toBe(8 * 8); // black, bottom row, first column
    expect(at(0, 7)).toBe(8 * 8); // white, top row, last column
    expect(at(0, 0)).toBe(0);
    expect(wf.max).toBe(64);
  });

  it("converts RGBA to Rec.709 luma", () => {
    expect(Array.from(rgbaToLuma(new Uint8ClampedArray([255, 255, 255, 255, 255, 0, 0, 255])))).toEqual([255, 54]);
  });
});
