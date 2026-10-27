// Luma waveform monitor (SPEC VIE-4, P6.3): for each output column, a histogram of the luma
// levels in the matching picture columns. Computed from the frame the viewer shows.

export interface Waveform {
  cols: number;
  rows: number;
  /** counts[row * cols + col]; row 0 is the top (100 %). */
  counts: Uint32Array;
  max: number;
}

/** Waveform of a luma plane. `limited`: values are 16–235 (video range) rather than 0–255. */
export function waveform(y: Uint8Array | Uint8ClampedArray, w: number, h: number, opts: { cols?: number; rows?: number; limited?: boolean; stride?: number; step?: number } = {}): Waveform {
  const cols = opts.cols ?? 256;
  const rows = opts.rows ?? 128;
  const stride = opts.stride ?? 1; // bytes between luma samples (4 for RGBA)
  const step = Math.max(1, opts.step ?? Math.ceil(h / 270)); // row subsampling
  const lo = opts.limited ? 16 : 0;
  const span = opts.limited ? 219 : 255;
  const counts = new Uint32Array(cols * rows);
  let max = 0;
  for (let yy = 0; yy < h; yy += step) {
    const rowBase = yy * w;
    for (let x = 0; x < w; x++) {
      const v = y[(rowBase + x) * stride]!;
      const level = Math.min(1, Math.max(0, (v - lo) / span));
      const r = rows - 1 - Math.round(level * (rows - 1));
      const c = Math.min(cols - 1, Math.floor((x * cols) / w));
      const i = r * cols + c;
      const n = ++counts[i]!;
      if (n > max) max = n;
    }
  }
  return { cols, rows, counts, max };
}

/** Rec.709 luma of RGBA pixels, written into a single-channel array. */
export function rgbaToLuma(rgba: Uint8ClampedArray): Uint8Array {
  const out = new Uint8Array(rgba.length / 4);
  for (let i = 0, j = 0; j < out.length; i += 4, j++) out[j] = Math.round(0.2126 * rgba[i]! + 0.7152 * rgba[i + 1]! + 0.0722 * rgba[i + 2]!);
  return out;
}

type Listener = (w: Waveform) => void;
const listeners = new Set<Listener>();

/** The viewer publishes each shown frame's waveform here while the scope is open. */
export const scopeBus = {
  emit(w: Waveform) {
    listeners.forEach((l) => l(w));
  },
  listen(l: Listener): () => void {
    listeners.add(l);
    return () => {
      listeners.delete(l);
    };
  },
  get active() {
    return listeners.size > 0;
  },
};
