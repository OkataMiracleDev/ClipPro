import { describe, expect, it } from "vitest";
import { CubeError, parseCube } from "./cube";

function identity3d(n: number) {
  const rows = [`TITLE "id"`, `LUT_3D_SIZE ${n}`];
  for (let b = 0; b < n; b++) for (let g = 0; g < n; g++) for (let r = 0; r < n; r++) rows.push(`${r / (n - 1)} ${g / (n - 1)} ${b / (n - 1)}`);
  return rows.join("\n");
}

describe("parseCube", () => {
  it("reads a 3D identity LUT with red fastest", () => {
    const l = parseCube(identity3d(3));
    expect(l.kind).toBe("3d");
    expect(l.size).toBe(3);
    expect(l.title).toBe("id");
    expect(Array.from(l.data.slice(4, 8))).toEqual([0.5, 0, 0, 1]); // index 1: r advances first
    expect(Array.from(l.data.slice(12, 16))).toEqual([0, 0.5, 0, 1]); // index 3: g
  });

  it("reads a 1D LUT with domain and comments", () => {
    const l = parseCube("# comment\nLUT_1D_SIZE 2\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 2 2 2\n\n0 0 0\n1 1 1\n");
    expect(l.kind).toBe("1d");
    expect(l.domainMax).toEqual([2, 2, 2]);
  });

  it("accepts sizes up to 65 and rejects larger", () => {
    expect(parseCube(identity3d(65)).data.length).toBe(65 ** 3 * 4);
    expect(() => parseCube("LUT_3D_SIZE 66\n")).toThrow(CubeError);
  });

  it("rejects malformed files", () => {
    for (const bad of ["", "0 0 0", "LUT_3D_SIZE 2\n0 0 0", "LUT_1D_SIZE 2\n0 0\n1 1 1", "LUT_1D_SIZE 2\n0 0 x\n1 1 1", "LUT_1D_SIZE 2\nDOMAIN_MAX 0 0 0\n0 0 0\n1 1 1", "LUT_1D_SIZE 2\nLUT_3D_SIZE 2"]) {
      expect(() => parseCube(bad), JSON.stringify(bad)).toThrow(CubeError);
    }
  });

  it("only ever throws CubeError on random input (fuzz)", () => {
    let s = 12345;
    const rnd = () => ((s = (s * 1103515245 + 12345) >>> 0) / 2 ** 32);
    const alphabet = ["LUT_3D_SIZE ", "LUT_1D_SIZE ", "DOMAIN_MIN ", "TITLE ", "2", "3", "0.5", "-1", "1e9", "nan", " ", "\n", "#", "x", "\r\n"];
    for (let i = 0; i < 3000; i++) {
      let t = "";
      const len = 1 + Math.floor(rnd() * 40);
      for (let k = 0; k < len; k++) t += alphabet[Math.floor(rnd() * alphabet.length)];
      try {
        const l = parseCube(t);
        expect(l.data.length % 4).toBe(0);
      } catch (e) {
        expect(e).toBeInstanceOf(CubeError);
      }
    }
  });
});
