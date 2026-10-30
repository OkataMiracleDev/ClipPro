// Source list + clip table (SPEC §14.1, P5.3).

import * as CM from "@radix-ui/react-context-menu";
import { AlertCircle, CircleDashed, Film, ListChecks } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { api, type Clip } from "../api";
import { pickFolder, pickMedia, pickProxy } from "../lib/dialogs";
import { BinsSection, useLibraryClips } from "./Bins";
import { WatchFolders } from "./WatchFolders";
import { SyncGroupsSection, SyncSheet, useSync } from "./SyncGroups";
import { cx, scoreColor, Spinner } from "../components/ui";
import { formatClock } from "../lib/typed";
import { useApp } from "../store";

export function Sidebar() {
  const { clips, scored, selectsOnly, toggleSelectsOnly, library, setLibrary } = useApp();
  const all = library.kind === "all";
  const item = (active: boolean) =>
    cx("flex h-7 w-full items-center gap-2 rounded-md px-2 text-left text-[13px]", active ? "bg-accent-soft text-text" : "text-text-2 hover:bg-control-hover");
  return (
    <nav className="flex shrink-0 flex-col gap-4 px-2 py-3" aria-label="Library">
      <div>
        <div className="section-header mb-1 px-2">Library</div>
        <button
          className={item(all && !selectsOnly)}
          onClick={() => {
            setLibrary({ kind: "all" });
            if (selectsOnly) toggleSelectsOnly();
          }}
          onDragOver={(e) => e.dataTransfer.types.includes("application/x-clippro-clips") && e.preventDefault()}
          onDrop={(e) => {
            const ids = JSON.parse(e.dataTransfer.getData("application/x-clippro-clips") || "[]") as number[];
            if (ids.length) void useApp.getState().moveToBin(ids, null);
          }}
          title="Drop clips here to take them out of their bin"
        >
          <Film size={14} /> <span className="flex-1">All clips</span>
          <span className="font-mono text-[11px] text-text-3 tabular">{clips.length}</span>
        </button>
        <button
          className={item(all && selectsOnly)}
          onClick={() => {
            setLibrary({ kind: "all" });
            if (!selectsOnly) toggleSelectsOnly();
          }}
        >
          <ListChecks size={14} /> <span className="flex-1">Selects</span>
          <span className="font-mono text-[11px] text-text-3 tabular">{scored?.summary.count ?? 0}</span>
        </button>
      </div>
      <BinsSection item={item} />
      <SyncGroupsSection />
      <WatchFolders />
      <SyncSheet />
    </nav>
  );
}

function StateIcon({ c }: { c: Clip }) {
  if (c.state === "failed") return <AlertCircle size={13} className="text-danger" aria-label="Failed" />;
  if (c.state === "running") return <Spinner size={12} />;
  if (c.state === "queued" || c.state === "paused") return <CircleDashed size={13} className="text-text-3" aria-label="Waiting" />;
  return null;
}

const ROW_H = 44;
const OVERSCAN = 8;

/** "Audio stream ›" for clips with more than one audio stream (P10.2). */
function AudioStreamMenu({ c, item }: { c: Clip; item: string }) {
  const [info, setInfo] = useState<Awaited<ReturnType<typeof api.audioStreams>> | null>(null);
  useEffect(() => {
    let live = true;
    api.audioStreams(c.clip).then((r) => live && setInfo(r), () => {});
    return () => {
      live = false;
    };
  }, [c.clip]);
  if (!info || info.streams.length < 2) return null;
  const choose = (s: number | null) =>
    void api.audioStreamSet(c.clip, s).then(
      () => {
        useApp.getState().toast("Re-analysing the audio");
        void useApp.getState().refreshClips();
      },
      (e) => useApp.getState().toast(`${e}`, "error"),
    );
  return (
    <CM.Sub>
      <CM.SubTrigger className={item}>Audio stream ›</CM.SubTrigger>
      <CM.Portal>
        <CM.SubContent className="z-50 min-w-48 rounded-lg bg-raised p-1 shadow-(--shadow-float) ring-1 ring-separator-strong">
          <CM.Item className={item} onSelect={() => choose(null)}>
            {info.selected === null ? "✓ " : ""}Automatic
          </CM.Item>
          {info.streams.map((s, i) => (
            <CM.Item key={s.index} className={item} onSelect={() => choose(s.index)}>
              {info.selected === s.index ? "✓ " : ""}Track {i + 1} · {s.codec} {s.channels}ch{s.language ? ` · ${s.language}` : ""}
            </CM.Item>
          ))}
        </CM.SubContent>
      </CM.Portal>
    </CM.Sub>
  );
}

/** Right-click menu for a clip (SPEC P5.3). */
function ClipMenu({ c, children }: { c: Clip; children: React.ReactNode }) {
  const { retryClip, bumpClip, linkProxy, toast } = useApp.getState();
  const item = "flex h-6 cursor-default items-center rounded px-2 text-[13px] outline-none data-[disabled]:opacity-40 data-[highlighted]:bg-accent data-[highlighted]:text-accent-fg";
  const waiting = c.state === "queued" || c.state === "paused";
  const busy = c.state === "running" || waiting;
  return (
    <CM.Root>
      <CM.Trigger asChild>{children}</CM.Trigger>
      <CM.Portal>
        <CM.Content className="z-50 min-w-48 rounded-lg bg-raised p-1 shadow-(--shadow-float) ring-1 ring-separator-strong" aria-label={`${c.name} actions`}>
          <CM.Item className={item} disabled={!waiting} onSelect={() => void bumpClip(c.clip)}>
            Move to next
          </CM.Item>
          <CM.Separator className="my-1 h-px bg-separator" />
          <CM.Item className={item} disabled={busy} onSelect={() => void retryClip(c.clip, false)}>
            Retry
          </CM.Item>
          <CM.Item className={item} disabled={busy} onSelect={() => void retryClip(c.clip, true)}>
            Retry with CPU decoding
          </CM.Item>
          <CM.Separator className="my-1 h-px bg-separator" />
          <CM.Item className={item} onSelect={() => useSync.getState().show([c.clip])}>
            Sync with other clips…
          </CM.Item>
          <CM.Sub>
            <CM.SubTrigger className={item}>Move to bin ›</CM.SubTrigger>
            <CM.Portal>
              <CM.SubContent className="z-50 min-w-40 rounded-lg bg-raised p-1 shadow-(--shadow-float) ring-1 ring-separator-strong">
                <CM.Item className={item} disabled={c.bin === null} onSelect={() => void useApp.getState().moveToBin([c.clip], null)}>
                  No bin
                </CM.Item>
                {useApp
                  .getState()
                  .bins.filter((b) => b.kind === "manual")
                  .map((b) => (
                    <CM.Item key={b.id} className={item} disabled={c.bin === b.id} onSelect={() => void useApp.getState().moveToBin([c.clip], b.id)}>
                      {b.name}
                    </CM.Item>
                  ))}
              </CM.SubContent>
            </CM.Portal>
          </CM.Sub>
          <AudioStreamMenu c={c} item={item} />
          <CM.Separator className="my-1 h-px bg-separator" />
          <CM.Item className={item} onSelect={() => void api.reveal(c.path).catch((e) => toast(`${e}`, "error"))}>
            Reveal in Explorer
          </CM.Item>
          <CM.Item
            className={item}
            disabled={c.state === "running"}
            onSelect={() =>
              void pickProxy().then((p) => {
                if (p) void linkProxy(c.clip, p);
              })
            }
          >
            Link proxy…
          </CM.Item>
          {c.missing && (
            <>
              <CM.Separator className="my-1 h-px bg-separator" />
              <CM.Item
                className={item}
                onSelect={() =>
                  void pickMedia().then((ps) => {
                    if (ps[0]) void useApp.getState().relinkClip(c.media, ps[0]);
                  })
                }
              >
                Relink this clip…
              </CM.Item>
              <CM.Item
                className={item}
                onSelect={() =>
                  void pickFolder().then((f) => {
                    if (f) void useApp.getState().relinkFolder(f);
                  })
                }
              >
                Find missing files in a folder…
              </CM.Item>
            </>
          )}
        </CM.Content>
      </CM.Portal>
    </CM.Root>
  );
}

export function ClipList() {
  const { scored, selectedClip, selectClip } = useApp();
  const clips = useLibraryClips();
  const stats = useMemo(() => {
    const m = new Map<number, { selects: number; best: number; duration: number }>();
    for (const c of scored?.clips ?? []) {
      const best = c.segments.reduce((a, s) => Math.max(a, s.mean), -Infinity);
      m.set(c.clip, { selects: c.segments.length, best, duration: c.durationUs });
    }
    return m;
  }, [scored]);
  // Virtualised: only the rows in view (plus a margin) are rendered, so 100k clips scroll smoothly.
  const box = useRef<HTMLDivElement>(null);
  const [view, setView] = useState({ top: 0, h: 800 });
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const update = () => setView({ top: el.scrollTop, h: el.clientHeight });
    update();
    const ro = new ResizeObserver(update);
    ro.observe(el);
    el.addEventListener("scroll", update, { passive: true });
    return () => {
      ro.disconnect();
      el.removeEventListener("scroll", update);
    };
  }, [clips.length > 0]);
  // Keep the selected clip in view when it changes from elsewhere (timeline, keys).
  useEffect(() => {
    const el = box.current;
    const i = clips.findIndex((c) => c.clip === selectedClip);
    if (!el || i < 0) return;
    const y = i * ROW_H;
    if (y < el.scrollTop) el.scrollTop = y;
    else if (y + ROW_H > el.scrollTop + el.clientHeight) el.scrollTop = y + ROW_H - el.clientHeight;
  }, [selectedClip, clips]);
  if (!clips.length) return null;
  const first = Math.max(0, Math.floor(view.top / ROW_H) - OVERSCAN);
  const last = Math.min(clips.length, Math.ceil((view.top + view.h) / ROW_H) + OVERSCAN);
  return (
    <div ref={box} className="h-full overflow-y-auto px-2 py-1" role="listbox" aria-label="Clips">
      <div className="relative" style={{ height: clips.length * ROW_H }}>
        {clips.slice(first, last).map((c, k) => {
          const st = stats.get(c.clip);
          const active = c.clip === selectedClip;
          const hasBest = st && Number.isFinite(st.best);
          return (
            <ClipMenu key={c.clip} c={c}>
              <div
                role="option"
                aria-selected={active}
                onClick={() => selectClip(c.clip)}
                onContextMenu={() => selectClip(c.clip)}
                draggable
                onDragStart={(e) => {
                  e.dataTransfer.setData("application/x-clippro-clips", JSON.stringify([c.clip]));
                  e.dataTransfer.effectAllowed = "move";
                }}
                title={c.error ?? c.path}
                style={{ top: (first + k) * ROW_H, height: ROW_H - 2 }}
                className={cx("absolute inset-x-0 flex cursor-default items-center gap-2 rounded-md px-2", active ? "bg-accent text-accent-fg" : "hover:bg-control-hover")}
              >
                <span className="h-7 w-1 shrink-0 rounded-full" style={{ background: hasBest ? scoreColor(st!.best) : "var(--c-separator-strong)" }} aria-hidden />
                <span className="min-w-0 flex-1">
                  <span className="flex items-center gap-1.5">
                    <span className="truncate text-[12px] font-medium">{c.name}</span>
                    <StateIcon c={c} />
                  </span>
                  <span className={cx("block truncate font-mono text-[10.5px] tabular", active ? "opacity-80" : "text-text-3")}>
                    {c.missing ? "Missing — right-click to relink · " : ""}
                    {st ? formatClock(st.duration) : c.state === "failed" ? (c.error ?? "Failed") : "Not analysed yet"}
                    {st ? ` · ${st.selects} select${st.selects === 1 ? "" : "s"}` : ""}
                    {hasBest ? ` · best ${Math.round(st!.best * 100)}` : ""}
                  </span>
                </span>
              </div>
            </ClipMenu>
          );
        })}
      </div>
    </div>
  );
}
