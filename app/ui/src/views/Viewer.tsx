// Viewer (P6, ADR-002): native <video> for codecs WebView2 plays, backend-decoded frames in a
// WebGL view (with shader overlays) for everything else. The store playhead is the single source of truth: playback
// writes it, scrubbing anywhere else seeks to it.

import { ChevronLeft, ChevronRight, Film, Pause, Play, Repeat } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type ColourProfile, type MediaSource } from "../api";
import { scoreColor } from "../components/ui";
import { parseCube } from "../lib/cube";
import { pickLut } from "../lib/dialogs";
import { FLAGS, PROFILES } from "../lib/domain";
import { fetchFrame } from "../lib/frames";
import { GlView, type Overlay, type ProfileColour } from "../lib/glview";
import { useInspection } from "../lib/inspect";
import { parseHeader, HEADER_BYTES } from "../lib/glview";
import { rgbaToLuma, scopeBus, waveform } from "../lib/scope";
import { actionFor, isTyping } from "../lib/keys";
import { shuttle, type ShuttleKey } from "../lib/shuttle";
import { WaveformScope } from "./WaveformScope";
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
  const userLut = useApp((s) => s.userLut);
  const split = useApp((s) => s.split);
  const compare = useApp((s) => s.compare);
  const scope = useApp((s) => s.scope);
  // Log footage and user LUTs need the shader's colour stage, so they use frames too.
  const graded = (clip?.profile ?? "rec709") !== "rec709" || userLut !== null;
  const native = src?.nativeUrl && clip && !fallback[clip.media] && overlay === "none" && !scope && !graded && !(playing && rate < 0) ? src.nativeUrl : null;
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
      const action = actionFor(e);
      if (!action) return;
      const st = useApp.getState();
      const { playheadUs: at, playing: p, rate: r } = st;
      const shuttleBy = (key: ShuttleKey) => {
        if (key === "l" && !p && at >= durationUs - frameUs) seek(0);
        if (key === "j" && !p && at <= 0) return;
        const next = shuttle(key, p, r);
        setRate(next.rate);
        setPlaying(next.playing);
      };
      const step = (us: number) => {
        setPlaying(false);
        seek(at + us);
      };
      switch (action) {
        case "play_pause":
          if (!p && at >= durationUs - frameUs) seek(0);
          setRate(1);
          setPlaying(!p);
          break;
        case "shuttle_back":
          shuttleBy("j");
          break;
        case "shuttle_stop":
          shuttleBy("k");
          break;
        case "shuttle_forward":
          shuttleBy("l");
          break;
        case "prev_frame":
          step(-frameUs);
          break;
        case "next_frame":
          step(frameUs);
          break;
        case "back_second":
          step(-1e6);
          break;
        case "forward_second":
          step(1e6);
          break;
        case "go_start":
          seek(0);
          break;
        case "go_end":
          setPlaying(false);
          seek(durationUs);
          break;
        case "loop":
          toggleLoop();
          break;
        case "inspector":
          setRightTab(st.rightTab === "frame" ? "tuning" : "frame");
          break;
        case "zebras":
          toggleOverlay("zebras");
          break;
        case "false_colour":
          toggleOverlay("false_colour");
          break;
        case "peaking":
          toggleOverlay("peaking");
          break;
        case "focus_map":
          toggleMeasureOverlay("tiles");
          break;
        case "motion":
          toggleMeasureOverlay("motion");
          break;
        case "waveform":
          st.toggleScope();
          break;
        case "split":
          st.setSplit(st.split === null ? 0.5 : null);
          break;
        case "close_compare":
          if (!st.compare) return;
          st.setCompare(null);
          break;
        case "mark_in":
          st.setMark("in");
          break;
        case "mark_out":
          st.setMark("out");
          break;
        case "force":
          void st.toggleForce();
          break;
        case "trim_start":
          void st.trimSelect("start");
          break;
        case "trim_end":
          void st.trimSelect("end");
          break;
        case "manual_select":
          void st.addManualSegment();
          break;
        default:
          return; // global actions (undo, export…) are handled elsewhere
      }
      e.preventDefault();
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
      <div className="flex min-h-0 flex-1">
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
            {split !== null && !native && graded && <SplitHandle split={split} />}
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
              <FrameCanvas key={clip.media} media={clip.media} clip={clip.clip} profile={clip.profile} src={src} width={box.w} frameUs={frameUs} onError={setError} />
            )}
            <MeasureOverlay kind={measureOverlay} data={inspection.data} />
          </div>
        ) : null}
        {clip && src && scope && !native && <WaveformScope key={clip.media} />}
        {clip && (
          <div className="absolute bottom-3 left-3 flex items-center gap-2 rounded-md bg-black/45 px-2 py-1 font-mono text-[11px] text-white/85 backdrop-blur tabular">
            {formatClock(playheadUs)}
            {inSelect && <span className="rounded bg-[#0a84ff] px-1 text-[10px] text-white">SELECT</span>}
          </div>
        )}
        {clip && src && (
          <div className="absolute top-3 right-3 left-3 flex flex-wrap items-center justify-end gap-2">
            <ColourMenu media={clip.media} profile={clip.profile} />
            {/* Half-width compare panes keep only the colour menu; overlays stay on their keys. */}
            {!compare && (
              <>
                <OverlayBar value={overlay} onToggle={toggleOverlay} measure={measureOverlay} onMeasure={toggleMeasureOverlay} scope={scope} />
                <div className="rounded-md bg-black/45 px-2 py-1 font-mono text-[10px] text-white/60 backdrop-blur" title={native ? "Native playback" : "Decoded frames"}>
                  {src.codec.toUpperCase()} · {src.width}×{src.height} · {native ? "native" : "frames"}
                </div>
              </>
            )}
          </div>
        )}
        {error && <div className="absolute bottom-3 rounded-md bg-black/60 px-2.5 py-1 text-[11px] text-white/80">{error}</div>}
      </div>
      {compare && clip && <ComparePane compare={compare} />}
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

/** Profile picker, viewer LUT and before/after split (SPEC VIE-6, P6.5/P6.6). */
function ColourMenu({ media, profile }: { media: number; profile: ColourProfile }) {
  const userLut = useApp((s) => s.userLut);
  const split = useApp((s) => s.split);
  const { setClipProfile, setUserLut, setSplit, toast } = useApp.getState();
  const chip = "rounded px-1.5 py-0.5 text-[10px]";
  async function loadLut() {
    const path = await pickLut();
    if (!path) return;
    try {
      const lut = parseCube(await api.readLut(path));
      setUserLut({ name: lut.title || path.split(/[\\/]/).pop()!.replace(/\.cube$/i, ""), lut });
    } catch (e) {
      toast(`Couldn't load the LUT: ${e instanceof Error ? e.message : e}`, "error");
    }
  }
  return (
    <div className="flex items-center gap-0.5 rounded-md bg-black/45 p-0.5 text-white/80 backdrop-blur">
      <select
        aria-label="Colour profile"
        title="Colour profile of this clip"
        value={profile}
        onChange={(e) => void setClipProfile(media, e.target.value as ColourProfile)}
        className={`${chip} cursor-pointer bg-transparent outline-none hover:text-white`}
      >
        {PROFILES.map((p) => (
          <option key={p.value} value={p.value} className="bg-[#1c1c1e] text-white">
            {p.label}
          </option>
        ))}
      </select>
      <span className="mx-0.5 h-3 w-px bg-white/20" />
      {userLut ? (
        <button className={`${chip} bg-white/85 text-black`} title="Viewer LUT — click to remove" onClick={() => setUserLut(null)}>
          {userLut.name} ✕
        </button>
      ) : (
        <button className={`${chip} hover:text-white`} title="Load a .cube viewer LUT" onClick={() => void loadLut()}>
          LUT…
        </button>
      )}
      <button
        className={`${chip} ${split !== null ? "bg-white/85 text-black" : "hover:text-white"}`}
        aria-pressed={split !== null}
        title="Before/after split (B)"
        onClick={() => setSplit(split === null ? 0.5 : null)}
      >
        A|B
      </button>
    </div>
  );
}

/** Draggable divider for the before/after split. */
function SplitHandle({ split }: { split: number }) {
  const ref = useRef<HTMLDivElement>(null);
  const onDown = (e: React.PointerEvent) => {
    const box = ref.current?.parentElement?.getBoundingClientRect();
    if (!box) return;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => useApp.getState().setSplit(Math.min(0.98, Math.max(0.02, (ev.clientX - box.left) / box.width)));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };
  return (
    <div ref={ref} className="absolute inset-y-0 z-10 w-4 -translate-x-1/2 cursor-ew-resize" style={{ left: `${split * 100}%` }} onPointerDown={onDown} role="separator" aria-label="Before/after split" aria-valuenow={Math.round(split * 100)}>
      <span className="absolute top-2 -left-9 rounded bg-black/55 px-1 text-[9px] text-white/80">Before</span>
      <span className="absolute top-2 left-5 rounded bg-black/55 px-1 text-[9px] text-white/80">After</span>
    </div>
  );
}

const MEASURE_OVERLAYS: { value: MeasureOverlayKind; label: string; key: string }[] = [
  { value: "tiles", label: "Focus map", key: "T" },
  { value: "motion", label: "Motion", key: "V" },
];

function OverlayBar({ value, onToggle, measure, onMeasure, scope }: { value: Overlay; onToggle: (o: Overlay) => void; measure: MeasureOverlayKind; onMeasure: (o: MeasureOverlayKind) => void; scope: boolean }) {
  return (
    <div role="group" aria-label="Overlays" className="flex items-center rounded-md bg-black/45 p-0.5 backdrop-blur">
      <button
        aria-pressed={scope}
        title="Luma waveform (W)"
        onClick={() => useApp.getState().toggleScope()}
        className={`rounded px-1.5 py-0.5 text-[10px] ${scope ? "bg-white/85 text-black" : "text-white/70 hover:text-white"}`}
      >
        Waveform
      </button>
      <span className="mx-0.5 h-3 w-px bg-white/20" />
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
/** A/B compare (P6.6): the second select, following the primary playhead's offset into its
 * own select. Always decoded frames; silent. */
function ComparePane({ compare }: { compare: { clip: number; startUs: number; endUs: number; anchorUs: number } }) {
  const clips = useApp((s) => s.clips);
  const c = clips.find((x) => x.clip === compare.clip);
  const media = useMediaSource(c?.media ?? null);
  const src = media?.src ?? null;
  const { ref, box } = useFitBox(src ? src.width / Math.max(1, src.height) : 16 / 9);
  const [error, setError] = useState<string | null>(null);
  const frameUs = 1e6 / (src?.fps || 25);
  const follow = useCallback((us: number) => Math.min(compare.endUs - frameUs, Math.max(compare.startUs, compare.startUs + (us - compare.anchorUs))), [compare, frameUs]);
  const head = useApp((s) => s.playheadUs);
  return (
    <div ref={ref} className="relative grid min-h-0 flex-1 place-items-center overflow-hidden border-l border-white/10">
      {c && src && (
        <div className="relative" style={{ width: box.w, height: box.h }}>
          <FrameCanvas key={c.media} media={c.media} clip={c.clip} profile={c.profile} src={src} width={box.w} frameUs={frameUs} onError={setError} follow={follow} />
        </div>
      )}
      <div className="absolute top-3 left-3 flex items-center gap-2 rounded-md bg-black/45 px-2 py-1 font-mono text-[11px] text-white/85 backdrop-blur tabular">
        <span className="max-w-48 truncate font-sans">{c?.name ?? "—"}</span>
        {formatClock(follow(head))}
      </div>
      <button className="absolute top-3 right-3 rounded-md bg-black/45 px-2 py-1 text-[11px] text-white/80 backdrop-blur hover:text-white" title="Close compare (Esc)" onClick={() => useApp.getState().setCompare(null)}>
        Close
      </button>
      {error && <div className="absolute bottom-3 rounded-md bg-black/60 px-2.5 py-1 text-[11px] text-white/80">{error}</div>}
    </div>
  );
}

let scopeCanvas: HTMLCanvasElement | null = null;

function publishScope(f: Awaited<ReturnType<typeof fetchFrame>>) {
  if (f.kind === "i420") {
    const hd = parseHeader(f.buf);
    if (!hd) return;
    scopeBus.emit(waveform(new Uint8Array(f.buf, HEADER_BYTES, hd.width * hd.height), hd.width, hd.height, { limited: hd.range === 0 }));
  } else {
    const w = 320;
    const h = Math.max(1, Math.round((w * f.img.naturalHeight) / Math.max(1, f.img.naturalWidth)));
    scopeCanvas ??= document.createElement("canvas");
    scopeCanvas.width = w;
    scopeCanvas.height = h;
    const g = scopeCanvas.getContext("2d", { willReadFrequently: true });
    if (!g) return;
    g.drawImage(f.img, 0, 0, w, h);
    scopeBus.emit(waveform(rgbaToLuma(g.getImageData(0, 0, w, h).data), w, h, { step: 1 }));
  }
}

const profileColours = new Map<ColourProfile, Promise<ProfileColour>>();

function FrameCanvas({
  media,
  clip,
  profile,
  src,
  width,
  frameUs,
  onError,
  follow,
}: {
  media: number;
  clip: number;
  profile: ColourProfile;
  src: MediaSource;
  width: number;
  frameUs: number;
  onError: (e: string | null) => void;
  /** Secondary view: shows `follow(playhead)` and never drives playback or audio. */
  follow?: (us: number) => number;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const view = useRef<GlView | null>(null);
  const ownPlaying = useApp((s) => s.playing);
  const playing = follow ? false : ownPlaying;
  const head = useApp((s) => s.playheadUs);
  const playheadUs = follow ? follow(head) : head;
  const overlay = useApp((s) => s.overlay);
  const rate = useApp((s) => s.rate);
  const loader = useRef({ busy: false, want: -1, shown: -1 });
  const playingRef = useRef(playing);
  playingRef.current = playing;
  const overlayRef = useRef(overlay);
  overlayRef.current = overlay;
  const userLut = useApp((s) => s.userLut);
  const split = useApp((s) => s.split);
  const splitRef = useRef(split);
  splitRef.current = split;
  // Request width: device pixels, capped at the source, in 64 px steps so the backend's scaler
  // is reused while the window resizes.
  const reqW = Math.min(src.width, Math.max(64, Math.ceil((width * (window.devicePixelRatio || 1)) / 64) * 64));

  const draw = useCallback(() => {
    view.current?.draw({ overlay: overlayRef.current, zebra: [0.7, 0.95], peaking: 0.35, time: performance.now() / 1000, split: splitRef.current });
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
  }, [overlay, draw, width, split]);

  // Colour transform: the clip's profile (built-in) or the user's LUT.
  useEffect(() => {
    let live = true;
    if (profile === "rec709") {
      view.current?.setProfile(null);
      draw();
    } else {
      let p = profileColours.get(profile);
      if (!p) {
        p = api.previewColour(profile);
        profileColours.set(profile, p);
        p.catch(() => profileColours.delete(profile));
      }
      p.then((c) => {
        if (!live) return;
        view.current?.setProfile(c);
        draw();
      }, () => onError("Couldn't load this profile's transform"));
    }
    return () => {
      live = false;
    };
  }, [profile, draw, onError]);

  useEffect(() => {
    view.current?.setCube(userLut?.lut ?? null);
    draw();
  }, [userLut, draw]);

  const snap = useCallback((us: number) => Math.max(0, Math.min(src.durationUs - frameUs, Math.round(us / frameUs) * frameUs)), [src.durationUs, frameUs]);

  const load = useCallback(
    (us: number, onShown?: (us: number) => void) => {
      const l = loader.current;
      l.want = us;
      if (l.busy) return;
      l.busy = true;
      fetchFrame(media, us, reqW, playingRef.current ? useApp.getState().rate : null)
        .then((f) => {
          // The waveform reads the luma before the shared buffer is released (`done`).
          if (!follow && scopeBus.active) publishScope(f);
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
    [media, reqW, onError, draw, follow],
  );

  // Paused: show the playhead frame (coalescing rapid scrubs to the latest).
  useEffect(() => {
    if (playing) return;
    const us = snap(playheadUs);
    if (us !== loader.current.shown || loader.current.want !== us) load(us);
  }, [playheadUs, playing, snap, load]);

  // Opening the waveform refetches the shown frame so the scope has data while paused.
  const scopeOn = useApp((s) => s.scope);
  useEffect(() => {
    if (scopeOn && !follow && !playingRef.current && loader.current.shown >= 0) load(loader.current.shown);
  }, [scopeOn, follow, load]);

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
