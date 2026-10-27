// Preview transport benchmark (ADR-002), run by `ClipPro --bench-preview <out.json> <clip>...`.
// Times sequential forward playback through each transport inside WebView2, end to end:
// request, transfer, decode and display (2D draw or WebGL upload, forced to complete).

import { api, n } from "./api";
import { commands, type BenchClip } from "./bindings";

const FRAMES = 48;

interface Row {
  clip: string;
  source: string;
  transport: string;
  width: number;
  ms_mean: number;
  ms_p95: number;
  fps: number;
  kb_mean: number;
}

interface SharedBufferEvent extends Event {
  getBuffer(): ArrayBuffer;
  additionalData: { media: number; us: number; tag: number };
}
interface WebViewHost {
  addEventListener(type: "sharedbufferreceived", cb: (e: SharedBufferEvent) => void): void;
  releaseBuffer(b: ArrayBuffer): void;
}
const webview = (): WebViewHost | undefined => (window as unknown as { chrome?: { webview?: WebViewHost } }).chrome?.webview;

const rawUrl = (media: number, us: number, w: number) => api.frameUrl(media, us, w).replace("/frame/", "/raw/");

function stats(times: number[]) {
  const s = [...times].sort((a, b) => a - b);
  const mean = s.reduce((a, b) => a + b, 0) / s.length;
  return { mean, p95: s[Math.min(s.length - 1, Math.floor(s.length * 0.95))]! };
}

async function run(clips: BenchClip[]): Promise<Row[]> {
  const c2d = document.createElement("canvas").getContext("2d", { willReadFrequently: true })!;
  const glCanvas = document.createElement("canvas");
  const gl = glCanvas.getContext("webgl2")!;
  const tex = [0, 1, 2].map(() => {
    const t = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, t);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    return t;
  });
  gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
  const px = new Uint8Array(4);
  const upload = (buf: ArrayBuffer) => {
    const dv = new DataView(buf);
    const fw = dv.getUint32(4, true);
    const fh = dv.getUint32(8, true);
    const c = (fw / 2) * (fh / 2);
    const planes = [new Uint8Array(buf, 24, fw * fh), new Uint8Array(buf, 24 + fw * fh, c), new Uint8Array(buf, 24 + fw * fh + c, c)];
    planes.forEach((plane, i) => {
      gl.bindTexture(gl.TEXTURE_2D, tex[i]!);
      const [pw, ph] = i === 0 ? [fw, fh] : [fw / 2, fh / 2];
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, pw, ph, 0, gl.RED, gl.UNSIGNED_BYTE, plane);
    });
    gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px); // forces the uploads
    return buf.byteLength;
  };
  const waiting = new Map<number, (n: number) => void>();
  let tagSeq = 0;
  webview()?.addEventListener("sharedbufferreceived", (e) => {
    const buf = e.getBuffer();
    const n = upload(buf);
    webview()?.releaseBuffer(buf);
    waiting.get(e.additionalData.tag)?.(n);
    waiting.delete(e.additionalData.tag);
  });

  const transports: Record<string, (media: number, us: number, w: number) => Promise<number>> = {
    "jpeg <img>": async (media, us, w) => {
      const img = new Image();
      img.crossOrigin = "anonymous";
      img.src = api.frameUrl(media, us, w);
      await img.decode();
      c2d.canvas.width = img.naturalWidth;
      c2d.canvas.height = img.naturalHeight;
      c2d.drawImage(img, 0, 0);
      c2d.getImageData(0, 0, 1, 1);
      return 0;
    },
    "jpeg fetch+bitmap": async (media, us, w) => {
      const blob = await (await fetch(api.frameUrl(media, us, w))).blob();
      const bmp = await createImageBitmap(blob);
      c2d.canvas.width = bmp.width;
      c2d.canvas.height = bmp.height;
      c2d.drawImage(bmp, 0, 0);
      c2d.getImageData(0, 0, 1, 1);
      bmp.close();
      return blob.size;
    },
    "raw I420 webgl": async (media, us, w) => upload(await (await fetch(rawUrl(media, us, w))).arrayBuffer()),
    "shared I420 webgl": async (media, us, w) => {
      const tag = ++tagSeq;
      const got = new Promise<number>((r) => waiting.set(tag, r));
      const res = await commands.previewShared(media, us, w, tag);
      if (res.status === "error") throw new Error(res.error);
      return got;
    },
  };

  const rows: Row[] = [];
  for (const raw of clips) {
    const clip = { ...raw, media: n(raw.media), duration_us: n(raw.duration_us), fps: n(raw.fps, 25) };
    const frameUs = 1e6 / (clip.fps || 25);
    const widths = [...new Set([960, 1920, clip.width].filter((w) => w <= clip.width))];
    for (const [name, fn] of Object.entries(transports)) {
      for (const w of widths) {
        await fn(clip.media, 0, w); // warm-up: open, seek, scaler
        const times: number[] = [];
        let bytes = 0;
        for (let k = 1; k <= FRAMES; k++) {
          const us = (k * frameUs) % Math.max(frameUs, clip.duration_us - frameUs);
          const t0 = performance.now();
          bytes += await fn(clip.media, us, w);
          times.push(performance.now() - t0);
        }
        const st = stats(times);
        rows.push({
          clip: clip.name,
          source: `${clip.width}x${clip.height}`,
          transport: name,
          width: w,
          ms_mean: +st.mean.toFixed(1),
          ms_p95: +st.p95.toFixed(1),
          fps: +(1000 / st.mean).toFixed(1),
          kb_mean: Math.round(bytes / FRAMES / 1024),
        });
      }
    }
  }
  return rows;
}

/** Runs the benchmark when the backend asks for one; resolves false in normal runs. */
export async function maybeRunBench(): Promise<boolean> {
  const cfg = await commands.benchConfig();
  if (cfg.status !== "ok" || !cfg.data) return false;
  let report: string;
  try {
    report = JSON.stringify({ userAgent: navigator.userAgent, frames: FRAMES, rows: await run(cfg.data) }, null, 2);
  } catch (e) {
    report = JSON.stringify({ error: String(e) });
  }
  await commands.benchReport(report);
  return true;
}
