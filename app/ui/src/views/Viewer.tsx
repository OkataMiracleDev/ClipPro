// Viewer (P6, ADR-002): native <video> for codecs WebView2 plays, decoded JPEG frames from the
// backend for everything else. The store playhead is the single source of truth: playback
// writes it, scrubbing anywhere else seeks to it.

import { ChevronLeft, ChevronRight, Film, Pause, Play } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type MediaSource } from "../api";
import { scoreColor } from "../components/ui";
import { FLAGS } from "../lib/domain";
import { formatClock } from "../lib/typed";
import { useApp } from "../store";

const sources = new Map<number, Promise<MediaSource>>();

function useMediaSource(media: number | null) {
  const [state, setState] = useState<{ media: number; src: MediaSource | null; error: string | null } | null>(null);
  useEffect(() => {
    if (media === null) return;
    let live = true;
    let p = sources.get(media);
    if (!p) {
      p = api.mediaSource(media);
      sources.set(media, p);
      p.catch(() => sources.delete(media));
    }
    p.then(
      (src) => live && setState({ media, src, error: null }),
      (e) => live && setState({ media, src: null, error: String(e instanceof Error ? e.message : e) }),
    );
    return () => {
      live = false;
    };
  }, [media]);
  return state && state.media === media ? state : null;
}

/** Largest box with the source aspect that fits the container. */
function useFitBox(aspect: number) {
  const ref = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState({ w: 0, h: 0 });
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      const W = el.clientWidth;
      const H = el.clientHeight;
      const w = Math.min(W, H * aspect);
      setBox({ w: Math.floor(w), h: Math.floor(w / aspect) });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [aspect]);
  return { ref, box };
}

const isTyping = (t: EventTarget | null) => t instanceof HTMLElement && (t.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName));

export function Viewer() {
  const { clips, scored, selectedClip, playheadUs, playing, setPlaying, setPlayhead } = useApp();
  const clip = clips.find((c) => c.clip === selectedClip);
  const sc = scored?.clips.find((c) => c.clip === selectedClip);
  const media = useMediaSource(clip?.media ?? null);
  const src = media?.src ?? null;
  const [fallback, setFallback] = useState<Record<number, boolean>>({});
  const native = src?.nativeUrl && clip && !fallback[clip.media] ? src.nativeUrl : null;
  const durationUs = src?.durationUs ?? sc?.durationUs ?? 0;
  const frameUs = 1e6 / (src?.fps || 25);
  const { ref, box } = useFitBox(src ? src.width / Math.max(1, src.height) : 16 / 9);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => setError(null), [clip?.clip]);

  const sample = sc ? Math.min(sc.bar.length - 1, Math.max(0, Math.round((playheadUs / 1e6) * sc.rateHz))) : -1;
  const value = sc && sample >= 0 ? sc.bar[sample]! : NaN;
  const flags = useMemo(() => {
    if (!sc || sample < 0) return [];
    const bits = sc.flags[sample] ?? 0;
    return FLAGS.filter((_, i) => bits & (1 << i)).map((f) => f.label);
  }, [sc, sample]);
  const inSelect = sc?.segments.some((s) => playheadUs >= s.startUs && playheadUs < s.endUs) ?? false;

  const seek = useCallback(
    (us: number) => {
      if (!clip) return;
      setPlayhead(clip.clip, Math.max(0, Math.min(durationUs - frameUs, us)));
    },
    [clip, durationUs, frameUs, setPlayhead],
  );

  useEffect(() => {
    if (!clip) return;
    const onKey = (e: KeyboardEvent) => {
      if (isTyping(e.target) || e.ctrlKey || e.metaKey || e.altKey) return;
      const { playheadUs: at, playing: p } = useApp.getState();
      if (e.key === " " || e.key === "k" || e.key === "K") {
        e.preventDefault();
        if (!p && at >= durationUs - frameUs) seek(0);
        setPlaying(!p);
      } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        setPlaying(false);
        const step = e.shiftKey ? 1e6 : frameUs;
        seek(at + (e.key === "ArrowLeft" ? -step : step));
      } else if (e.key === "Home") {
        seek(0);
      } else if (e.key === "End") {
        setPlaying(false);
        seek(durationUs);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [clip, durationUs, frameUs, seek, setPlaying]);

  const togglePlay = () => {
    if (!playing && playheadUs >= durationUs - frameUs) seek(0);
    setPlaying(!playing);
  };

  return (
    <div className="flex h-full flex-col bg-viewer">
      <div ref={ref} className="relative grid min-h-0 flex-1 place-items-center overflow-hidden">
        {!clip ? (
          <div className="text-[12px] text-white/40">Select a clip</div>
        ) : media?.error ? (
          <div className="flex flex-col items-center gap-2 text-white/60">
            <Film size={28} className="opacity-40" />
            <div className="text-[13px]">{clip.name}</div>
            <div className="max-w-80 text-center text-[11px] text-white/40">{media.error}</div>
          </div>
        ) : src ? (
          <div className="relative" style={{ width: box.w, height: box.h }}>
            {native ? (
              <NativeVideo
                key={native}
                url={native}
                clip={clip.clip}
                frameUs={frameUs}
                onFallback={() => setFallback((f) => ({ ...f, [clip.media]: true }))}
              />
            ) : (
              <FrameCanvas key={clip.media} media={clip.media} clip={clip.clip} src={src} width={box.w} frameUs={frameUs} onError={setError} />
            )}
          </div>
        ) : null}
        {clip && (
          <div className="absolute top-3 left-3 flex items-center gap-2 rounded-md bg-black/45 px-2 py-1 font-mono text-[11px] text-white/85 backdrop-blur tabular">
            {formatClock(playheadUs)}
            {inSelect && <span className="rounded bg-[#0a84ff] px-1 text-[10px] text-white">SELECT</span>}
          </div>
        )}
        {clip && src && (
          <div className="absolute top-3 right-3 rounded-md bg-black/45 px-2 py-1 font-mono text-[10px] text-white/60 backdrop-blur" title={native ? "Native playback" : "Decoded frames"}>
            {src.codec.toUpperCase()} · {src.width}×{src.height} · {native ? "native" : "frames"}
          </div>
        )}
        {error && <div className="absolute bottom-3 rounded-md bg-black/60 px-2.5 py-1 text-[11px] text-white/80">{error}</div>}
      </div>
      {clip && (
        <div className="flex h-9 shrink-0 items-center gap-3 border-t border-white/10 px-2 text-[12px] text-white/80">
          <div className="flex items-center">
            <TransportButton label="Previous frame (←)" onClick={() => (setPlaying(false), seek(playheadUs - frameUs))}>
              <ChevronLeft size={15} />
            </TransportButton>
            <TransportButton label={playing ? "Pause (Space)" : "Play (Space)"} onClick={togglePlay} disabled={!src}>
              {playing ? <Pause size={14} fill="currentColor" /> : <Play size={14} fill="currentColor" />}
            </TransportButton>
            <TransportButton label="Next frame (→)" onClick={() => (setPlaying(false), seek(playheadUs + frameUs))}>
              <ChevronRight size={15} />
            </TransportButton>
          </div>
          <span className="font-mono text-[11px] text-white/55 tabular">
            {formatClock(playheadUs)} / {formatClock(durationUs)}
          </span>
          <span className="flex items-center gap-1.5">
            <span className="h-2.5 w-2.5 rounded-full" style={{ background: scoreColor(value) }} />
            <span className="font-mono tabular">{Number.isFinite(value) ? Math.round(value * 100) : "—"}</span>
            <span className="text-white/45">score</span>
          </span>
          <span className="truncate text-white/55">{flags.length ? flags.join(" · ") : "No flags"}</span>
        </div>
      )}
    </div>
  );
}

function TransportButton({ label, onClick, disabled, children }: { label: string; onClick: () => void; disabled?: boolean; children: React.ReactNode }) {
  return (
    <button
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
      className="grid h-7 w-7 place-items-center rounded-md text-white/80 hover:bg-white/10 hover:text-white disabled:opacity-35"
    >
      {children}
    </button>
  );
}

/** `<video>` through the asset protocol. Errors (unsupported codec) switch to frames. */
function NativeVideo({ url, clip, frameUs, onFallback }: { url: string; clip: number; frameUs: number; onFallback: () => void }) {
  const ref = useRef<HTMLVideoElement>(null);
  const playing = useApp((s) => s.playing);
  const playheadUs = useApp((s) => s.playheadUs);

  useEffect(() => {
    const v = ref.current;
    if (!v) return;
    if (playing) {
      v.play().catch(() => useApp.getState().setPlaying(false));
      let handle = 0;
      const tick = (_now: number, meta: VideoFrameCallbackMetadata) => {
        useApp.getState().setPlayhead(clip, meta.mediaTime * 1e6);
        handle = v.requestVideoFrameCallback(tick);
      };
      handle = v.requestVideoFrameCallback(tick);
      return () => {
        v.cancelVideoFrameCallback(handle);
        v.pause();
      };
    }
    v.pause();
  }, [playing, clip]);

  useEffect(() => {
    const v = ref.current;
    if (!v || playing) return;
    if (Math.abs(v.currentTime * 1e6 - playheadUs) > frameUs / 2) v.currentTime = playheadUs / 1e6;
  }, [playheadUs, playing, frameUs]);

  return (
    <video
      ref={ref}
      src={url}
      preload="auto"
      className="absolute inset-0 h-full w-full"
      onError={onFallback}
      onEnded={() => useApp.getState().setPlaying(false)}
      onLoadedMetadata={(e) => {
        e.currentTarget.currentTime = useApp.getState().playheadUs / 1e6;
      }}
    />
  );
}

/** Frames decoded by the backend, one request in flight; playback drops frames rather than lag. */
function FrameCanvas({ media, clip, src, width, frameUs, onError }: { media: number; clip: number; src: MediaSource; width: number; frameUs: number; onError: (e: string | null) => void }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const playing = useApp((s) => s.playing);
  const playheadUs = useApp((s) => s.playheadUs);
  const loader = useRef({ busy: false, want: -1, shown: -1 });
  // Request width: device pixels, capped at the source, in 64 px steps so the backend's scaler
  // is reused while the window resizes.
  const reqW = Math.min(src.width, Math.max(64, Math.ceil((width * (window.devicePixelRatio || 1)) / 64) * 64));

  const snap = useCallback((us: number) => Math.max(0, Math.min(src.durationUs - frameUs, Math.round(us / frameUs) * frameUs)), [src.durationUs, frameUs]);

  const load = useCallback(
    (us: number, onShown?: (us: number) => void) => {
      const l = loader.current;
      l.want = us;
      if (l.busy) return;
      l.busy = true;
      const img = new Image();
      img.crossOrigin = "anonymous"; // overlays read pixels back (ACAO * on clippro://)
      img.src = api.frameUrl(media, us, reqW);
      img
        .decode()
        .then(() => {
          const c = canvas.current;
          if (c) {
            c.width = img.naturalWidth;
            c.height = img.naturalHeight;
            c.getContext("2d")?.drawImage(img, 0, 0);
          }
          l.shown = us;
          onError(null);
          onShown?.(us);
        })
        .catch(() => {
          onError("Couldn't decode this frame");
          useApp.getState().setPlaying(false);
        })
        .finally(() => {
          l.busy = false;
          if (!playingRef.current && l.want !== us) load(l.want);
        });
    },
    [media, reqW, onError],
  );
  const playingRef = useRef(playing);
  playingRef.current = playing;

  // Paused: show the playhead frame (coalescing rapid scrubs to the latest).
  useEffect(() => {
    if (playing) return;
    const us = snap(playheadUs);
    if (us !== loader.current.shown || loader.current.want !== us) load(us);
  }, [playheadUs, playing, snap, load]);

  // Playing: a wall clock drives the target; each shown frame moves the playhead.
  useEffect(() => {
    if (!playing) return;
    const t0 = performance.now();
    const us0 = useApp.getState().playheadUs;
    let raf = 0;
    const tick = () => {
      const target = us0 + (performance.now() - t0) * 1000;
      if (target >= src.durationUs - frameUs) {
        useApp.getState().setPlaying(false);
        return;
      }
      if (!loader.current.busy) load(snap(target), (us) => playingRef.current && useApp.getState().setPlayhead(clip, us));
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [playing, clip, src.durationUs, frameUs, snap, load]);

  return <canvas ref={canvas} className="absolute inset-0 h-full w-full bg-black" />;
}
