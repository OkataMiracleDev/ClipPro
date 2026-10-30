// Imported-timeline review (SPEC EXP-10, P11.3/P11.4): an editor's cut, relinked to the project
// and scored item by item. Weak items are flagged; better takes can be swapped in, and the edit
// goes back out in its own format with score markers.

import * as RDialog from "@radix-ui/react-dialog";
import { AlertTriangle, Check, FileInput, Link2Off, Play, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { api, type Review, type ReviewItem } from "../api";
import { Button, cx, IconButton, scoreColor, Segmented, Spinner, Switch } from "../components/ui";
import { pickExportPath, pickTimeline } from "../lib/dialogs";
import { formatClock } from "../lib/typed";
import { useApp } from "../store";

const FORMATS = [
  { value: "xmeml", label: "FCP7 XML", ext: "xml" },
  { value: "fcpxml", label: "FCPXML", ext: "fcpxml" },
  { value: "otio", label: "OTIO", ext: "otio" },
  { value: "edl", label: "EDL", ext: "edl" },
  { value: "aaf", label: "AAF", ext: "aaf" },
] as const;
type Fmt = (typeof FORMATS)[number]["value"];

/** Record timecode (`HH:MM:SS:FF`, `;` before frames in drop-frame). */
export function recordTc(frames: number, fps: number, drop: boolean): string {
  const base = Math.max(1, Math.round(fps));
  let f = Math.max(0, Math.round(frames));
  const d = drop && base % 30 === 0 ? base / 15 : 0;
  if (d > 0) {
    const per10 = base * 600 - d * 9;
    const perMin = base * 60 - d;
    const ten = Math.floor(f / per10);
    const rem = f % per10;
    f += d * 9 * ten + (rem > d ? d * Math.floor((rem - d) / perMin) : 0);
  }
  const p = (v: number) => String(v).padStart(2, "0");
  const ff = f % base;
  const s = Math.floor(f / base);
  return `${p(Math.floor(s / 3600))}:${p(Math.floor(s / 60) % 60)}:${p(s % 60)}${d ? ";" : ":"}${p(ff)}`;
}

function Bars({ bars, weak }: { bars: number[]; weak: boolean }) {
  if (!bars.length) return <span className="block h-4 w-32 rounded-sm bg-separator/60" aria-hidden />;
  return (
    <span className={cx("flex h-4 w-32 items-end gap-px overflow-hidden rounded-sm bg-control-hover/50 px-px", weak && "ring-1 ring-[var(--c-score-lo)]")} aria-hidden>
      {bars.map((v, i) => (
        <span key={i} className="min-w-px flex-1 rounded-[1px]" style={{ height: `${Math.max(8, v * 100)}%`, background: scoreColor(v) }} />
      ))}
    </span>
  );
}

function Lanes({ review, selected, swapped, onSelect }: { review: Review; selected: number | null; swapped: Map<number, number>; onSelect: (i: number) => void }) {
  const dur = Math.max(1, review.duration);
  const tracks = Array.from({ length: Math.max(1, review.tracks) }, (_, k) => review.tracks - 1 - k); // V1 at the bottom, as in an NLE
  return (
    <div className="flex flex-col gap-0.5 rounded-(--radius-control) bg-control-hover/40 p-1 ring-1 ring-separator ring-inset" role="group" aria-label="Timeline">
      {tracks.map((t) => (
        <div key={t} className="relative h-6">
          <span className="absolute top-1/2 -left-0.5 -translate-x-full -translate-y-1/2 pr-1 font-mono text-[9.5px] text-text-3">V{t + 1}</span>
          {review.items.map((it, i) =>
            it.track !== t ? null : (
              <button
                key={i}
                aria-label={`${it.name}${it.mean === null ? ", offline" : `, score ${Math.round(it.mean * 100)}`}${it.weak ? ", weak" : ""}`}
                aria-pressed={selected === i}
                onClick={() => onSelect(i)}
                className={cx(
                  "absolute inset-y-0 overflow-hidden rounded-[3px] border-r border-black/25 text-left",
                  selected === i ? "ring-2 ring-accent ring-offset-1 ring-offset-raised z-10" : it.weak && !swapped.has(i) ? "ring-1 ring-[var(--c-score-lo)] ring-inset" : "",
                )}
                style={{
                  left: `${(it.recordIn / dur) * 100}%`,
                  width: `max(3px, ${((it.recordOut - it.recordIn) / dur) * 100}%)`,
                  background:
                    it.mean === null
                      ? "repeating-linear-gradient(135deg, var(--c-separator-strong) 0 4px, transparent 4px 8px)"
                      : scoreColor(swapped.has(i) ? (it.alternatives[swapped.get(i)!]?.mean ?? it.mean) : it.mean),
                }}
              >
                <span className="block truncate px-1 text-[10px] leading-6 font-medium text-black/70">{it.name}</span>
              </button>
            ),
          )}
        </div>
      ))}
    </div>
  );
}

function ItemRow({
  review,
  item,
  index,
  open,
  swap,
  onToggle,
  onSwap,
}: {
  review: Review;
  item: ReviewItem;
  index: number;
  open: boolean;
  swap: number | undefined;
  onToggle: () => void;
  onSwap: (alt: number | undefined) => void;
}) {
  const { selectClip, setPlayhead } = useApp();
  const show = (clip: number, us: number) => {
    selectClip(clip);
    setPlayhead(clip, us);
  };
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (open) ref.current?.scrollIntoView({ block: "nearest" });
  }, [open]);
  const offline = item.mean === null;
  return (
    <div ref={ref} className={cx("border-b border-separator last:border-b-0", open && "bg-accent-soft/40")}>
      <button onClick={onToggle} aria-expanded={open} className="grid w-full grid-cols-[2rem_6.5rem_1fr_8.5rem_2.5rem_9rem] items-center gap-3 px-3 py-1.5 text-left hover:bg-control-hover/60">
        <span className="font-mono text-[11px] text-text-3 tabular">{index + 1}</span>
        <span className="font-mono text-[11px] text-text-2 tabular">{recordTc(review.startTcFrames + item.recordIn, review.fps, review.dropFrame)}</span>
        <span className="flex min-w-0 items-center gap-1.5">
          {offline && <Link2Off size={12} className="shrink-0 text-text-3" aria-label="Offline" />}
          {item.weak && swap === undefined && <AlertTriangle size={12} className="shrink-0 text-[var(--c-score-lo)]" aria-label="Weak" />}
          {swap !== undefined && <Check size={12} className="shrink-0 text-accent-text" aria-label="Swapped" />}
          <span className="truncate text-[12.5px]">{item.name}</span>
          {item.flags.map((f) => (
            <span key={f} className="shrink-0 rounded px-1 text-[10px] text-text-2 ring-1 ring-separator-strong">
              {f}
            </span>
          ))}
        </span>
        <Bars bars={item.bars} weak={item.weak && swap === undefined} />
        <span className="text-right font-mono text-[12px] tabular" style={{ color: offline ? undefined : scoreColor(item.mean!) }}>
          {offline ? "—" : Math.round(item.mean! * 100)}
        </span>
        <span className="truncate text-right text-[11px] text-text-3">
          {offline ? "Not in this project" : item.alternatives.length ? `${item.alternatives.length} better take${item.alternatives.length === 1 ? "" : "s"}` : item.weak ? "No better take" : ""}
        </span>
      </button>
      {open && !offline && (
        <div className="flex flex-col gap-0.5 px-3 pb-2 pl-[2.75rem]" role="radiogroup" aria-label={`Take for item ${index + 1}`}>
          <AltRow checked={swap === undefined} onPick={() => onSwap(undefined)} title="Keep the editor’s choice" detail={`${formatClock(item.srcInUs)} – ${formatClock(item.srcOutUs)}`} score={item.mean!} onShow={() => item.clip !== null && show(item.clip, item.srcInUs)} />
          {item.alternatives.map((a, k) => (
            <AltRow
              key={k}
              checked={swap === k}
              onPick={() => onSwap(k)}
              title={a.clipName}
              tag={a.kind === "same_clip" ? "Same clip" : `Same take · ${Math.round(a.similarity * 100)}% alike`}
              detail={`${formatClock(a.startUs)} – ${formatClock(a.endUs)}`}
              score={a.mean}
              onShow={() => show(a.clip, a.startUs)}
            />
          ))}
          {!item.alternatives.length && <span className="py-1 text-[11.5px] text-text-3">No better segment in this clip or another take of it.</span>}
        </div>
      )}
    </div>
  );
}

function AltRow({ checked, onPick, title, tag, detail, score, onShow }: { checked: boolean; onPick: () => void; title: string; tag?: string; detail: string; score: number; onShow: () => void }) {
  return (
    <div className={cx("flex items-center gap-2 rounded-md px-1.5 py-1", checked ? "bg-control ring-1 ring-separator-strong" : "hover:bg-control-hover/60")}>
      <button role="radio" aria-checked={checked} onClick={onPick} className="flex min-w-0 flex-1 items-center gap-2 text-left">
        <span className={cx("grid h-3.5 w-3.5 shrink-0 place-items-center rounded-full ring-1", checked ? "bg-accent ring-accent" : "ring-separator-strong")}>{checked && <span className="h-1.5 w-1.5 rounded-full bg-accent-fg" />}</span>
        <span className="truncate text-[12px]">{title}</span>
        {tag && <span className="shrink-0 text-[10.5px] text-text-3">{tag}</span>}
        <span className="ml-auto shrink-0 font-mono text-[11px] text-text-3 tabular">{detail}</span>
        <span className="w-8 shrink-0 text-right font-mono text-[12px] tabular" style={{ color: scoreColor(score) }}>
          {Math.round(score * 100)}
        </span>
      </button>
      <IconButton label="Show in viewer" onClick={onShow}>
        <Play size={12} />
      </IconButton>
    </div>
  );
}

export function TimelineReview({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const { settings, toast } = useApp();
  const [path, setPath] = useState<string | null>(null);
  const [review, setReview] = useState<Review | null>(null);
  const [busy, setBusy] = useState(false);
  const [weakBelow, setWeakBelow] = useState(55);
  const [filter, setFilter] = useState<"all" | "weak" | "offline">("all");
  const [openItem, setOpenItem] = useState<number | null>(null);
  const [swaps, setSwaps] = useState<Map<number, number>>(new Map());
  const [format, setFormat] = useState<Fmt>("xmeml");
  const [markers, setMarkers] = useState(true);

  async function load(p: string, threshold = weakBelow) {
    if (!settings) return;
    setBusy(true);
    try {
      const r = await api.timelineReview(settings, p, threshold / 100);
      setPath(p);
      setReview(r);
      setSwaps(new Map());
      setOpenItem(null);
      if (FORMATS.some((f) => f.value === r.format)) setFormat(r.format as Fmt);
      // `?review=1` (screenshots) opens the first weak item that has a better take.
      if (new URLSearchParams(location.search).get("review") === "1") setOpenItem(Math.max(0, r.items.findIndex((i) => i.weak && i.alternatives.length > 0)));
    } catch (e) {
      toast(`Couldn’t review the timeline: ${e}`, "error");
    } finally {
      setBusy(false);
    }
  }

  async function choose() {
    const p = await pickTimeline();
    if (p) await load(p);
  }

  // Ask for a file once each time the window opens (settings arrive after the first render).
  const prompted = useRef(false);
  useEffect(() => {
    if (!open) {
      prompted.current = false;
      return;
    }
    if (prompted.current || !settings || review) return;
    prompted.current = true;
    void choose();
  }, [open, settings]);

  async function exportBack() {
    if (!review) return;
    const f = FORMATS.find((x) => x.value === format)!;
    const out = await pickExportPath(`${review.name.replace(/[\\/:*?"<>|]/g, "-")} reviewed`, f.ext, f.label, "Export the reviewed timeline");
    if (!out) return;
    try {
      const r = await api.timelineExportBack([...swaps].map(([item, alternative]) => ({ item, alternative })), markers, format, out);
      toast(`Exported ${r.selects} clip${r.selects === 1 ? "" : "s"} to ${r.path.split(/[\\/]/).pop()}`);
    } catch (e) {
      toast(`Export failed: ${e}`, "error");
    }
  }

  const shown = useMemo(() => {
    if (!review) return [];
    return review.items
      .map((it, i) => [it, i] as const)
      .filter(([it, i]) => (filter === "weak" ? it.weak && !swaps.has(i) : filter === "offline" ? it.mean === null : true));
  }, [review, filter, swaps]);
  const weak = review?.items.filter((it, i) => it.weak && !swaps.has(i)).length ?? 0;

  return (
    <RDialog.Root open={open} onOpenChange={onOpenChange}>
      <RDialog.Portal>
        <RDialog.Overlay className="fixed inset-0 z-40 bg-black/25" />
        <RDialog.Content
          aria-describedby={undefined}
          onOpenAutoFocus={(e) => e.preventDefault()}
          className="fixed top-(--h-titlebar) left-1/2 z-50 flex max-h-[86vh] w-[960px] max-w-[96vw] -translate-x-1/2 flex-col rounded-b-(--radius-sheet) bg-raised shadow-(--shadow-float) ring-1 ring-separator-strong data-[state=open]:animate-[sheet_220ms_var(--ease-spring)]"
        >
          <div className="flex items-start gap-3 px-5 pt-4 pb-3">
            <div className="min-w-0 flex-1">
              <RDialog.Title className="text-[15px] font-semibold">Review timeline</RDialog.Title>
              <p className="truncate text-[12px] text-text-3">
                {review
                  ? `${review.name} · ${FORMATS.find((f) => f.value === review.format)?.label ?? review.format} · ${review.items.length} clip${review.items.length === 1 ? "" : "s"} · ${Number(review.fps.toFixed(3))} fps`
                  : "Score an editor’s cut against this project and swap in better takes."}
              </p>
            </div>
            <label className="flex items-center gap-1.5 text-[12px] text-text-2">
              Weak below
              <input
                value={weakBelow}
                onChange={(e) => setWeakBelow(Math.max(0, Math.min(100, Number(e.target.value) || 0)))}
                onBlur={() => path && void load(path)}
                onKeyDown={(e) => e.key === "Enter" && path && void load(path)}
                inputMode="numeric"
                aria-label="Weak below score"
                className="h-6 w-11 rounded border border-separator-strong bg-control px-1.5 text-right font-mono text-[12px]"
              />
            </label>
            <Button onClick={() => void choose()} disabled={busy}>
              <FileInput size={13} /> {review ? "Open another…" : "Choose a timeline…"}
            </Button>
            <RDialog.Close asChild>
              <IconButton label="Close">
                <X size={15} />
              </IconButton>
            </RDialog.Close>
          </div>
          {busy && !review && (
            <div className="flex items-center gap-2 px-5 pb-5 text-[12px] text-text-2">
              <Spinner /> Relinking and scoring…
            </div>
          )}
          {review && (
            <>
              <div className="flex flex-col gap-2.5 px-5 pl-9">
                <Lanes review={review} selected={openItem} swapped={swaps} onSelect={(i) => setOpenItem(i)} />
                <div className="flex items-center gap-3 text-[12px]">
                  <Segmented
                    size="sm"
                    label="Show"
                    value={filter}
                    onChange={setFilter}
                    options={[
                      { value: "all", label: `All ${review.items.length}` },
                      { value: "weak", label: `Weak ${weak}` },
                      { value: "offline", label: `Offline ${review.items.filter((i) => i.mean === null).length}` },
                    ]}
                  />
                  {review.missing.length > 0 && (
                    <span className="truncate text-text-3" title={review.missing.join("\n")}>
                      Not in this project: {review.missing.slice(0, 3).join(", ")}
                      {review.missing.length > 3 ? ` and ${review.missing.length - 3} more` : ""}
                    </span>
                  )}
                  {busy && <Spinner size={12} />}
                </div>
              </div>
              <div className="mx-5 mt-2 min-h-0 flex-1 overflow-y-auto rounded-(--radius-control) ring-1 ring-separator">
                {shown.map(([it, i]) => (
                  <ItemRow
                    key={i}
                    review={review}
                    item={it}
                    index={i}
                    open={openItem === i}
                    swap={swaps.get(i)}
                    onToggle={() => setOpenItem(openItem === i ? null : i)}
                    onSwap={(alt) =>
                      setSwaps((m) => {
                        const n = new Map(m);
                        if (alt === undefined) n.delete(i);
                        else n.set(i, alt);
                        return n;
                      })
                    }
                  />
                ))}
                {!shown.length && <div className="p-6 text-center text-[12px] text-text-3">{filter === "weak" ? "Nothing weak left in this cut." : "Every clip is in this project."}</div>}
              </div>
              <div className="flex items-center gap-3 px-5 py-3">
                <Segmented label="Format" value={format} onChange={setFormat} options={FORMATS.map((f) => ({ value: f.value, label: f.label }))} />
                <label className="flex items-center gap-2 text-[12px] text-text-2">
                  <Switch label="Score markers" checked={markers} onChange={setMarkers} /> Score markers
                </label>
                <span className="ml-auto text-[12px] text-text-3">{swaps.size ? `${swaps.size} swap${swaps.size === 1 ? "" : "s"}` : "No swaps"}</span>
                <Button variant="primary" onClick={() => void exportBack()}>
                  Export back…
                </Button>
              </div>
            </>
          )}
        </RDialog.Content>
      </RDialog.Portal>
    </RDialog.Root>
  );
}
