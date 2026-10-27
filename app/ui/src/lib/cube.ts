// Adobe/Resolve `.cube` LUT parser (P6.5): 1D or 3D, sizes up to 65 (3D) / 65536 (1D).
// Output is RGBA32F (alpha 1) in the file's order: red changes fastest, as WebGL's 3D textures
// expect.

export class CubeError extends Error {}

export interface CubeLut {
  title: string;
  kind: "1d" | "3d";
  size: number;
  domainMin: [number, number, number];
  domainMax: [number, number, number];
  /** RGBA float data: size entries (1D) or size³ entries (3D). */
  data: Float32Array;
}

const MAX_3D = 65;
const MAX_1D = 65536;

function triple(parts: string[], line: number): [number, number, number] {
  if (parts.length !== 3) throw new CubeError(`Line ${line}: expected 3 numbers`);
  const v = parts.map(Number) as [number, number, number];
  if (v.some((x) => !Number.isFinite(x))) throw new CubeError(`Line ${line}: not a number`);
  return v;
}

export function parseCube(text: string): CubeLut {
  let title = "";
  let kind: "1d" | "3d" | null = null;
  let size = 0;
  let domainMin: [number, number, number] = [0, 0, 0];
  let domainMax: [number, number, number] = [1, 1, 1];
  let data: Float32Array | null = null;
  let n = 0;
  const lines = text.split(/\r\n|\r|\n/);
  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i]!.trim();
    if (!raw || raw.startsWith("#")) continue;
    const parts = raw.split(/\s+/);
    const key = parts[0]!.toUpperCase();
    if (key === "TITLE") {
      title = raw.slice(5).trim().replace(/^"|"$/g, "");
    } else if (key === "LUT_1D_SIZE" || key === "LUT_3D_SIZE") {
      if (kind) throw new CubeError(`Line ${i + 1}: LUT size given twice`);
      kind = key === "LUT_1D_SIZE" ? "1d" : "3d";
      size = Number(parts[1]);
      const max = kind === "3d" ? MAX_3D : MAX_1D;
      if (!Number.isInteger(size) || size < 2 || size > max) throw new CubeError(`Line ${i + 1}: size must be 2–${max}`);
      data = new Float32Array((kind === "3d" ? size ** 3 : size) * 4);
    } else if (key === "DOMAIN_MIN") {
      domainMin = triple(parts.slice(1), i + 1);
    } else if (key === "DOMAIN_MAX") {
      domainMax = triple(parts.slice(1), i + 1);
    } else if (key === "LUT_1D_INPUT_RANGE" || key === "LUT_3D_INPUT_RANGE") {
      const [lo, hi] = parts.slice(1).map(Number);
      if (!Number.isFinite(lo) || !Number.isFinite(hi)) throw new CubeError(`Line ${i + 1}: bad input range`);
      domainMin = [lo!, lo!, lo!];
      domainMax = [hi!, hi!, hi!];
    } else if (/^[-+.\d]/.test(key)) {
      if (!data) throw new CubeError(`Line ${i + 1}: data before LUT size`);
      if (n * 4 >= data.length) throw new CubeError(`Line ${i + 1}: more entries than the LUT size`);
      const [r, g, b] = triple(parts, i + 1);
      data[n * 4] = r;
      data[n * 4 + 1] = g;
      data[n * 4 + 2] = b;
      data[n * 4 + 3] = 1;
      n++;
    } else {
      // Unknown keywords (e.g. LUT_IN_VIDEO_RANGE) are ignored, as Resolve does.
    }
  }
  if (!kind || !data) throw new CubeError("No LUT_1D_SIZE or LUT_3D_SIZE");
  if (n * 4 !== data.length) throw new CubeError(`Expected ${data.length / 4} entries, found ${n}`);
  for (let c = 0; c < 3; c++) if (!(domainMax[c]! > domainMin[c]!)) throw new CubeError("DOMAIN_MAX must exceed DOMAIN_MIN");
  return { title, kind, size, domainMin, domainMax, data };
}
