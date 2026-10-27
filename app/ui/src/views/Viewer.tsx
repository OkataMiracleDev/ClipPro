// Viewer area. Playback and overlays arrive in P6; until then it shows the clip, the sample at
// the playhead, its score and flags.

import { Film } from "lucide-react";
import { useMemo } from "react";
import { scoreColor } from "../components/ui";
import { FLAGS } from "../lib/domain";
import { formatClock } from "../lib/typed";
import { useApp } from "../store";

export function Viewer() {
  const { clips, scored, selectedClip, playheadUs } = useApp();
  const clip = clips.find((c) => c.clip === selectedClip);
  const sc = scored?.clips.find((c) => c.clip === selectedClip);
  const sample = sc ? Math.min(sc.bar.length - 1, Math.max(0, Math.round((playheadUs / 1e6) * sc.rateHz))) : -1;
  const value = sc && sample >= 0 ? sc.bar[sample]! : NaN;
  const flags = useMemo(() => {
    if (!sc || sample < 0) return [];
    const bits = sc.flags[sample] ?? 0;
    return FLAGS.filter((_, i) => bits & (1 << i)).map((f) => f.label);
  }, [sc, sample]);
  const inSelect = sc?.segments.some((s) => playheadUs >= s.startUs && playheadUs < s.endUs) ?? false;

  return (
    <div className="flex h-full flex-col bg-viewer">
      <div className="relative min-h-0 flex-1">
        {clip ? (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-2 text-white/70">
            <Film size={28} className="opacity-40" />
            <div className="text-[13px]">{clip.name}</div>
            <div className="text-[11px] text-white/40">Preview playback arrives with the viewer (P6)</div>
          </div>
        ) : (
          <div className="absolute inset-0 grid place-items-center text-[12px] text-white/40">Select a clip</div>
        )}
        {clip && (
          <div className="absolute top-3 left-3 flex items-center gap-2 rounded-md bg-black/45 px-2 py-1 font-mono text-[11px] text-white/85 backdrop-blur tabular">
            {formatClock(playheadUs)}
            {inSelect && <span className="rounded bg-[#0a84ff] px-1 text-[10px] text-white">SELECT</span>}
          </div>
        )}
      </div>
      {clip && (
        <div className="flex h-9 shrink-0 items-center gap-3 border-t border-white/10 px-3 text-[12px] text-white/80">
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
