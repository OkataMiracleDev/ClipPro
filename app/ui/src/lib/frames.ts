// Frame fetching for the viewer (ADR-002): WebView2 SharedBuffer first, raw I420 over
// clippro:// when SharedBuffer isn't available, and plain images in the browser preview.

import { api, isTauri } from "../api";
import { commands } from "../bindings";

export type FrameData = { kind: "i420"; buf: ArrayBuffer; done(): void } | { kind: "image"; img: HTMLImageElement; done(): void };

interface SharedBufferEvent extends Event {
  getBuffer(): ArrayBuffer;
  additionalData: { media: number; us: number; tag: number };
}
interface WebViewHost {
  addEventListener(type: "sharedbufferreceived", cb: (e: SharedBufferEvent) => void): void;
  releaseBuffer(b: ArrayBuffer): void;
}
const webview = (): WebViewHost | undefined => (window as unknown as { chrome?: { webview?: WebViewHost } }).chrome?.webview;

let sharedOk = true;
let listening = false;
let tagSeq = 0;
const waiting = new Map<number, (buf: ArrayBuffer) => void>();

function listen(wv: WebViewHost) {
  if (listening) return;
  listening = true;
  wv.addEventListener("sharedbufferreceived", (e) => {
    const buf = e.getBuffer();
    const cb = waiting.get(e.additionalData?.tag);
    if (cb) {
      waiting.delete(e.additionalData.tag);
      cb(buf);
    } else {
      wv.releaseBuffer(buf); // a request that timed out
    }
  });
}

async function viaShared(wv: WebViewHost, media: number, us: number, width: number, rate: number | null): Promise<FrameData> {
  listen(wv);
  const tag = ++tagSeq;
  const got = new Promise<ArrayBuffer>((resolve, reject) => {
    waiting.set(tag, resolve);
    setTimeout(() => {
      if (waiting.delete(tag)) reject(new Error("Frame timed out"));
    }, 5000);
  });
  const r = await commands.previewShared(media, us, Math.round(width), tag, rate);
  if (r.status === "error") {
    waiting.delete(tag);
    throw new Error(r.error);
  }
  const buf = await got;
  return { kind: "i420", buf, done: () => wv.releaseBuffer(buf) };
}

async function viaProtocol(media: number, us: number, width: number, rate: number | null): Promise<FrameData> {
  const url = api.frameUrl(media, us, width).replace("/frame/", "/raw/");
  const r = await fetch(rate ? `${url}&play=${rate}` : url);
  if (!r.ok) throw new Error(await r.text());
  return { kind: "i420", buf: await r.arrayBuffer(), done: () => {} };
}

async function viaImage(media: number, us: number, width: number): Promise<FrameData> {
  const img = new Image();
  img.crossOrigin = "anonymous";
  img.src = api.frameUrl(media, us, width);
  await img.decode();
  return { kind: "image", img, done: () => {} };
}

/** The frame shown at `us`, at most `width` pixels wide. `rate` marks playback requests so the
 * backend decodes the next frame ahead. */
export async function fetchFrame(media: number, us: number, width: number, rate: number | null = null): Promise<FrameData> {
  if (!isTauri) return viaImage(media, us, width);
  const wv = webview();
  if (wv && sharedOk) {
    try {
      return await viaShared(wv, media, us, width, rate);
    } catch (e) {
      if (!String(e instanceof Error ? e.message : e).startsWith("SharedBuffer")) throw e;
      sharedOk = false; // older runtime: fall back for the rest of the session
    }
  }
  return viaProtocol(media, us, width, rate);
}
