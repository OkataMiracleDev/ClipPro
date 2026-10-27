import { describe, expect, it } from "vitest";
import { HEADER_BYTES, parseHeader, yuvToRgb } from "./glview";

/** Applies the column-major mat3 the shader uses. */
function apply(matrix: number, full: boolean, y: number, u: number, v: number) {
  const { m, off } = yuvToRgb(matrix, full);
  const d = [y / 255 - off[0]!, u / 255 - off[1]!, v / 255 - off[2]!];
  return [0, 1, 2].map((r) => m[r]! * d[0]! + m[3 + r]! * d[1]! + m[6 + r]! * d[2]!);
}

describe("yuvToRgb", () => {
  it("maps limited-range black and white", () => {
    apply(1, false, 16, 128, 128).forEach((c) => expect(c).toBeCloseTo(0, 3));
    apply(1, false, 235, 128, 128).forEach((c) => expect(c).toBeCloseTo(1, 3));
  });

  it("decodes BT.709 and BT.601 primaries", () => {
    // 100% red, limited range: BT.709 (63, 102, 240), BT.601 (81, 90, 240).
    const r709 = apply(1, false, 63, 102, 240);
    expect(r709[0]).toBeCloseTo(1, 1);
    expect(Math.abs(r709[1]!)).toBeLessThan(0.02);
    expect(Math.abs(r709[2]!)).toBeLessThan(0.02);
    const r601 = apply(0, false, 81, 90, 240);
    expect(r601[0]).toBeCloseTo(1, 1);
    expect(Math.abs(r601[1]!)).toBeLessThan(0.02);
  });

  it("handles full range", () => {
    apply(1, true, 255, 128, 128).forEach((c) => expect(c).toBeCloseTo(1, 3));
    apply(1, true, 0, 128, 128).forEach((c) => expect(c).toBeCloseTo(0, 3));
  });
});

describe("parseHeader", () => {
  it("reads the CPF1 header", () => {
    const buf = new ArrayBuffer(HEADER_BYTES);
    const dv = new DataView(buf);
    [0x43, 0x50, 0x46, 0x31].forEach((b, i) => dv.setUint8(i, b));
    dv.setUint32(4, 640, true);
    dv.setUint32(8, 360, true);
    dv.setUint8(12, 1);
    dv.setUint8(13, 0);
    dv.setBigInt64(16, 1_480_000n, true);
    expect(parseHeader(buf)).toEqual({ width: 640, height: 360, matrix: 1, range: 0, ptsUs: 1_480_000 });
    expect(parseHeader(new ArrayBuffer(8))).toBeNull();
  });
});
