// Viewer (P6, ADR-002): native <video> for codecs WebView2 plays, backend-decoded frames in a
// WebGL view (with shader overlays) for everything else. The store playhead is the single source of truth: playback
// writes it, scrubbing anywhere else seeks to it.

import { ChevronLeft, ChevronRight, Film, Pause, Play, Repeat } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type MediaSource } from "../api";
import { scoreColor } from "../components/ui";
import { FLAGS } from "../lib/domain";
import { fetchFrame } from "../lib/frames";
import { GlView, type Overlay } from "../lib/glview";
import { useInspection } from "../lib/inspect";
import { shuttle, type ShuttleKey } from "../lib/shuttle";
import { MeasureOverlay, type MeasureOverlayKind } from "./MeasureOverlay";
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

/** Playback bounds: the select under `at` when looping (else the whole clip), and whether to wrap. */
function playRange(clip: number, at: number, durationUs: number, frameUs: number): { start: number; end: number; loop: boolean } {
  const { scored, loop } = useApp.getState();
  const whole = { start: 0, end: Math.max(0, durationUs - frameUs), loop };
  if (!loop) return whole;
  const seg = scored?.clips.find((c) => c.clip === clip)?.segments.find((s) => at >= s.startUs - frameUs / 2 && at < s.endUs);
  return seg ? { start: seg.startUs, end: Math.min(seg.endUs, whole.end), loop } : whole;
}

const isTyping = (t: EventTarget | null) => t instanceof HTMLElement && (t.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName));

export function Viewer() {
  const { clips, scored, selectedClip, playheadUs, playing, setPlaying, setPlayhead, overlay, toggleOverlay, measureOverlay, toggleMeasureOverlay, setRightTab, rate, setRate, loop, toggleLoop } = useApp();
  const inspection = useInspection(measureOverlay !== "none");
  const clip = clips.find((c) => c.clip === selectedClip);
  const sc = scored?.clips.find((c) => c.clip === selectedClip);
  const media = useMediaSource(clip?.media ?? null);
  const src = media?.src ?? null;
  const [fallback, setFallback] = useState<Record<number, boolean>>({});
  // Overlays read pixels, which the cross-origin <video> doesn't allow, and <video> can't play
  // backwards: both use frames.
  const native = src?.nativeUrl && clip && !fallback[clip.media] && overlay === "none" && !(playing && rate < 0) ? src.nativeUrl : null;
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
      if (isTyping(e.target)) return;
      if (e.ctrlKey && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "l") {
        e.preventDefault();
        toggleLoop();
        return;
      }
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      const { playheadUs: at, playing: p, rate: r } = useApp.getState();
      if (e.key === " ") {
        e.preventDefault();
        if (!p && at >= durationUs - frameUs) seek(0);
        setRate(1);
        setPlaying(!p);
      } else if (["j", "k", "l"].includes(e.key.toLowerCase())) {
        const key = e.key.toLowerCase() as ShuttleKey;
        if (key === "l" && !p && at >= durationUs - frameUs) seek(0);
        if (key === "j" && !p && at <= 0) return;
        const next = shuttle(key, p, r);
        setRate(next.rate);
        setPlaying(next.playing);
      } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        setPlaying(false);
        const step = e.shiftKey ? 1e6 : frameUs;
        seek(at + (e.key === "ArrowLeft" ? -step : step));
      } else if (e.key === "z" || e.key === "Z") {
        toggleOverlay("zebras");
      } else if (e.key === "c" || e.key === "C") {
        toggleOverlay("false_colour");
      } else if (e.key === "p" || e.key === "P") {
        toggleOverlay("peaking");
      } else if (e.key === "t" || e.key === "T") {
        toggleMeasureOverlay("tiles");
      } else if (e.key === "m" || e.key === "M") {
        toggleMeasureOverlay("motion");
      } else if (e.key === "i" || e.key === "I") {
        setRightTab(useApp.getState().rightTab === "frame" ? "tuning" : "frame");
      } else if (e.key === "Home") {
        seek(0);
      } else if (e.key === "End") {
        setPlaying(false);
        seek(durationUs);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [clip, durationUs, frameUs, seek, setPlaying, setRate, toggleLoop, toggleOverlay, toggleMeasureOverlay, setRightTab]);

  const togglePlay = () => {
    if (!playing && playheadUs >= durationUs - frameUs) seek(0);
    setRate(1);
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
                durationUs={durationUs}
                onFallback={() => setFallback((f) => ({ ...f, [clip.media]: true }))}
              />
            ) : (
              <FrameCanvas key={clip.media} media={clip.media} clip={clip.clip} src={src} width={box.w} frameUs={frameUs} onError={setError} />
            )}
            <MeasureOverlay kind={measureOverlay} data={inspection.data} />
          </div>
        ) : null}
        {clip && (
          <div className="absolute top-3 left-3 flex items-center gap-2 rounded-md bg-black/45 px-2 py-1 font-mono text-[11px] text-white/85 backdrop-blur tabular">
            {formatClock(playheadUs)}
            {inSelect && <span className="rounded bg-[#0a84ff] px-1 text-[10px] text-white">SELECT</span>}
          </div>
        )}
        {clip && src && (
          <div className="absolute top-3 right-3 flex items-center gap-2">
            <OverlayBar value={overlay} onToggle={toggleOverlay} measure={measureOverlay} onMeasure={toggleMeasureOverlay} />
            <div className="rounded-md bg-black/45 px-2 py-1 font-mono text-[10px] text-white/60 backdrop-blur" title={native ? "Native playback" : "Decoded frames"}>
              {src.codec.toUpperCase()} · {src.width}×{src.height} · {native ? "native" : "frames"}
            </div>
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
            <TransportButton label={loop ? "Loop on (Ctrl+L)" : "Loop the select (Ctrl+L)"} onClick={toggleLoop} pressed={loop}>
              <Repeat size={13} />
            </TransportButton>
          </div>
          {playing && rate !== 1 && <span className="rounded bg-white/15 px-1.5 font-mono text-[11px] tabular">{rate < 0 ? `◀ ${-rate}×` : `${rate}×`}</span>}
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

const OVERLAYS: { value: Overlay; label: string; key: string }[] = [
  { value: "zebras", label: "Zebras", key: "Z" },
  { value: "false_colour", label: "False colour", key: "C" },
  { value: "peaking", label: "Focus peaking", key: "P" },
];

const MEASURE_OVERLAYS: { value: MeasureOverlayKind; label: string; key: string }[] = [
  { value: "tiles", label: "Focus map", key: "T" },
  { value: "motion", label: "Motion", key: "M" },
];

function OverlayBar({ value, onToggle, measure, onMeasure }: { value: Overlay; onToggle: (o: Overlay) => void; measure: MeasureOverlayKind; onMeasure: (o: MeasureOverlayKind) => void }) {
  return (
    <div role="group" aria-label="Overlays" className="flex items-center rounded-md bg-black/45 p-0.5 backdrop-blur">
      {MEASURE_OVERLAYS.map((o) => (
        <button
          key={o.value}
          aria-pressed={measure === o.value}
          title={`${o.label} (${o.key})`}
          onClick={() => onMeasure(o.value)}
          className={`rounded px-1.5 py-0.5 text-[10px] ${measure === o.value ? "bg-white/85 text-black" : "text-white/70 hover:text-white"}`}
        >
          {o.label}
        </button>
      ))}
      <span className="mx-0.5 h-3 w-px bg-white/20" />
      {OVERLAYS.map((o) => (
        <button
          key={o.value}
          aria-pressed={value === o.value}
          title={`${o.label} (${o.key})`}
          onClick={() => onToggle(o.value)}
          className={`rounded px-1.5 py-0.5 text-[10px] ${value === o.value ? "bg-white/85 text-black" : "text-white/70 hover:text-white"}`}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

function TransportButton({ label, onClick, disabled, pressed, children }: { label: string; onClick: () => void; disabled?: boolean; pressed?: boolean; children: React.ReactNode }) {
  return (
    <button
      aria-label={label}
      title={label}
      disabled={disabled}
      aria-pressed={pressed}
      onClick={onClick}
      className={`grid h-7 w-7 place-items-center rounded-md hover:bg-white/10 hover:text-white disabled:opacity-35 ${pressed ? "text-[#0a84ff]" : "text-white/80"}`}
    >
      {children}
    </button>
  );
}

/** `<video>` through the asset protocol. Errors (unsupported codec) switch to frames. */
function NativeVideo({ url, clip, frameUs, durationUs, onFallback }: { url: string; clip: number; frameUs: number; durationUs: number; onFallback: () => void }) {
  const ref = useRef<HTMLVideoElement>(null);
  const playing = useApp((s) => s.playing);
  const playheadUs = useApp((s) => s.playheadUs);
  const rate = useApp((s) => s.rate);

  useEffect(() => {
    const v = ref.current;
    if (!v) return;
    if (playing && rate > 0) {
      v.playbackRate = rate;
      const range = playRange(clip, useApp.getState().playheadUs, durationUs, frameUs);
      v.play().catch(() => useApp.getState().setPlaying(false));
      let handle = 0;
      const tick = (_now: number, meta: VideoFrameCallbackMetadata) => {
        const us = meta.mediaTime * 1e6;
        if (useApp.getState().loop && range.loop && us >= range.end) v.currentTime = range.start / 1e6;
        useApp.getState().setPlayhead(clip, us);
        handle = v.requestVideoFrameCallback(tick);
      };
      handle = v.requestVideoFrameCallback(tick);
      return () => {
        v.cancelVideoFrameCallback(handle);
        v.pause();
      };
    }
    v.pause();
  }, [playing, rate, clip, durationUs, frameUs]);

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
      onEnded={(e) => {
        if (useApp.getState().loop) {
          const r = playRange(clip, 0, durationUs, frameUs);
          e.currentTarget.currentTime = r.start / 1e6;
          void e.currentTarget.play();
        } else useApp.getState().setPlaying(false);
      }}
      onLoadedMetadata={(e) => {
        e.currentTarget.currentTime = useApp.getState().playheadUs / 1e6;
      }}
    />
  );
}

/** Frames decoded by the backend into a WebGL view, one request in flight; playback drops
 * frames rather than lag. Overlay changes redraw without refetching. */
function FrameCanvas({ media, clip, src, width, frameUs, onError }: { media: number; clip: number; src: MediaSource; width: number; frameUs: number; onError: (e: string | null) => void }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const view = useRef<GlView | null>(null);
  const playing = useApp((s) => s.playing);
  const playheadUs = useApp((s) => s.playheadUs);
  const overlay = useApp((s) => s.overlay);
  const rate = useApp((s) => s.rate);
  const loader = useRef({ busy: false, want: -1, shown: -1 });
  const playingRef = useRef(playing);
  playingRef.current = playing;
  const overlayRef = useRef(overlay);
  overlayRef.current = overlay;
  // Request width: device pixels, capped at the source, in 64 px steps so the backend's scaler
  // is reused while the window resizes.
  const reqW = Math.min(src.width, Math.max(64, Math.ceil((width * (window.devicePixelRatio || 1)) / 64) * 64));

  const draw = useCallback(() => {
    view.current?.draw({ overlay: overlayRef.current, zebra: [0.7, 0.95], peaking: 0.35, time: performance.now() / 1000 });
  }, []);

  useEffect(() => {
    if (!canvas.current) return;
    try {
      view.current = new GlView(canvas.current);
    } catch (e) {
      onError(`Viewer unavailable: ${e instanceof Error ? e.message : e}`);
    }
    return () => {
      view.current = null;
    };
  }, [onError]);

  // Overlay and size changes redraw the current frame; zebras crawl while shown.
  useEffect(() => {
    draw();
    if (overlay !== "zebras") return;
    let raf = 0;
    const tick = () => {
      draw();
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [overlay, draw, width]);

  const snap = useCallback((us: number) => Math.max(0, Math.min(src.durationUs - frameUs, Math.round(us / frameUs) * frameUs)), [src.durationUs, frameUs]);

  const load = useCallback(
    (us: number, onShown?: (us: number) => void) => {
      const l = loader.current;
      l.want = us;
      if (l.busy) return;
      l.busy = true;
      fetchFrame(media, us, reqW, playingRef.current ? useApp.getState().rate : null)
        .then((f) => {
          const v = view.current;
          if (v) {
            if (f.kind === "i420") v.setI420(f.buf);
            else v.setImage(f.img, f.img.naturalWidth, f.img.naturalHeight);
            draw();
          }
          f.done();
          l.shown = us;
          onError(null);
          onShown?.(us);
        })
        .catch((e) => {
          onError(`Couldn't show this frame: ${e instanceof Error ? e.message : e}`);
          useApp.getState().setPlaying(false);
        })
        .finally(() => {
          l.busy = false;
          if (!playingRef.current && l.want !== us) load(l.want);
        });
    },
    [media, reqW, onError, draw],
  );

  // Paused: show the playhead frame (coalescing rapid scrubs to the latest).
  useEffect(() => {
    if (playing) return;
    const us = snap(playheadUs);
    if (us !== loader.current.shown || loader.current.want !== us) load(us);
  }, [playheadUs, playing, snap, load]);

  // A new request width refetches the shown frame at that size.
  useEffect(() => {
    if (!playingRef.current && loader.current.shown >= 0) load(loader.current.shown);
  }, [reqW, load]);

  // Playing: a wall clock drives the target; each shown frame moves the playhead.
  useEffect(() => {
    if (!playing) return;
    let t0 = performance.now();
    let us0 = useApp.getState().playheadUs;
    const range = playRange(clip, us0, src.durationUs, frameUs);
    let raf = 0;
    // At 1× the audio clock is the master: start audio here and re-anchor the wall clock to it
    // every few frames. Other rates (and clips without audio) run on the wall clock, silent.
    let audio = false;
    let live = true;
    let lastSync = 0;
    const startAudio = (at: number) => {
      if (rate !== 1 || !src.hasAudio) return;
      api.audioPlay(media, at).then(
        (ok) => {
          audio = ok && live;
          if (!live) void api.audioStop();
        },
        () => (audio = false),
      );
    };
    startAudio(us0);
    const tick = () => {
      const now = performance.now();
      if (audio && now - lastSync > 200) {
        lastSync = now;
        const asked = now;
        void api.audioClock().then((pos) => {
          if (pos === null || !live) return;
          // The reply describes the moment of the request plus about half the round trip.
          us0 = pos;
          t0 = asked + (performance.now() - asked) / 2;
        });
      }
      let target = us0 + (now - t0) * 1000 * rate;
      const past = rate > 0 ? target >= range.end : target <= range.start;
      if (past) {
        if (!useApp.getState().loop) {
          useApp.getState().setPlaying(false);
          useApp.getState().setPlayhead(clip, rate > 0 ? range.end : range.start);
          return;
        }
        us0 = rate > 0 ? range.start : range.end;
        t0 = now;
        target = us0;
        if (audio) startAudio(us0);
      }
      if (!loader.current.busy) load(snap(target), (us) => playingRef.current && useApp.getState().setPlayhead(clip, us));
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => {
      live = false;
      cancelAnimationFrame(raf);
      if (rate === 1 && src.hasAudio) void api.audioStop();
    };
  }, [playing, rate, clip, media, src.durationUs, src.hasAudio, frameUs, snap, load]);

  return <canvas ref={canvas} className="absolute inset-0 h-full w-full bg-black" />;
}
