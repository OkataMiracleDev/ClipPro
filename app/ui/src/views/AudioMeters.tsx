// Playback meters and per-channel mute/solo (SPEC P10.2) for frame playback, where ClipPro plays
// the audio itself. Click a channel to mute it; Alt-click to solo it.

import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { cx } from "../components/ui";
import { useApp } from "../store";

const FLOOR = -60;

export function AudioMeters() {
  const playing = useApp((s) => s.playing);
  const [levels, setLevels] = useState<number[]>([]);
  const [sources, setSources] = useState(0);
  const [muted, setMuted] = useState<boolean[]>([]);
  const hold = useRef<number[]>([]);

  useEffect(() => {
    if (!playing) {
      setLevels([]);
      return;
    }
    let live = true;
    const tick = async () => {
      try {
        const r = await api.audioLevels();
        if (!live) return;
        // Ballistics: fast attack, ~20 dB/s release.
        hold.current = r.levels.map((l, i) => Math.max(l, (hold.current[i] ?? FLOOR) - 1.4));
        setLevels([...hold.current]);
        setSources(r.sourceChannels);
      } catch {
        /* not playing audio */
      }
    };
    const id = setInterval(() => void tick(), 70);
    return () => {
      live = false;
      clearInterval(id);
    };
  }, [playing]);

  const apply = (m: boolean[]) => {
    setMuted(m);
    void api.audioChannelGains(m.map((x) => (x ? 0 : 1))).catch(() => {});
  };
  const n = Math.max(sources, 0);
  if (n === 0 && !levels.length) return null;

  return (
    <span className="flex items-center gap-1.5" aria-label="Audio">
      <span className="flex h-3 flex-col justify-between gap-px" aria-hidden>
        {levels.slice(0, 2).map((l, i) => {
          const f = Math.max(0, Math.min(1, (l - FLOOR) / -FLOOR));
          return (
            <span key={i} className="relative block h-[5px] w-16 overflow-hidden rounded-sm bg-white/10">
              <span className={cx("absolute inset-y-0 left-0", l > -1 ? "bg-[#ff453a]" : l > -12 ? "bg-[#ffd60a]" : "bg-[#32d74b]")} style={{ width: `${f * 100}%` }} />
            </span>
          );
        })}
      </span>
      {Array.from({ length: Math.min(n, 8) }, (_, i) => (
        <button
          key={i}
          className={cx("h-4 min-w-4 rounded px-0.5 font-mono text-[9.5px] ring-1", muted[i] ? "text-white/30 ring-white/15 line-through" : "text-white/80 ring-white/30")}
          title={`Channel ${i + 1}: click to ${muted[i] ? "unmute" : "mute"}, Alt-click to solo`}
          aria-pressed={!muted[i]}
          onClick={(e) => {
            const m = Array.from({ length: n }, (_, j) => muted[j] ?? false);
            if (e.altKey) apply(m.map((_, j) => j !== i));
            else apply(m.map((x, j) => (j === i ? !x : x)));
          }}
        >
          {n === 2 ? (i === 0 ? "L" : "R") : i + 1}
        </button>
      ))}
    </span>
  );
}
