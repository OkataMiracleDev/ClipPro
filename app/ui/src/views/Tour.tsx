// First-run tour (P14.4): a few callouts over the real window, shown once (Settings keeps
// `tour_done`) and again from Help.

import { useEffect, useLayoutEffect, useState } from "react";
import { create } from "zustand";
import { api } from "../api";
import { Button } from "../components/ui";
import { useApp } from "../store";

interface Step {
  target: string;
  title: string;
  text: string;
}

export const STEPS: Step[] = [
  { target: "add", title: "Add footage", text: "Add files or whole folders here, or drag them onto the window. Analysis starts straight away." },
  { target: "clips", title: "Your clips", text: "Every clip with its progress, number of selects and best score. Right-click for more: retry, proxies, sync, relink." },
  { target: "viewer", title: "The viewer", text: "Plays the selected clip. Press Z, C, P, T or V for exposure, focus and motion overlays; Ctrl+I explains any frame." },
  { target: "inspector", title: "Tuning", text: "Pick a preset, then adjust the score threshold, segment length and weighting. Selects update instantly." },
  { target: "timeline", title: "Timeline and chart", text: "Every clip's score, selects and flags. The chart above plots each select by length and score." },
  { target: "export", title: "Export", text: "Send the selects to Premiere Pro, Resolve, Final Cut Pro or Avid, render them, or make a report. Press F1 any time for help." },
];

interface TourState {
  step: number | null;
  start: () => void;
  stop: () => void;
}

export const useTour = create<TourState>((set) => ({
  step: null,
  start: () => set({ step: 0 }),
  stop: () => set({ step: null }),
}));

function finish() {
  useTour.getState().stop();
  const s = useApp.getState().appSettings;
  if (s && !s.tour_done) void api.setSettings({ ...s, tour_done: true }).catch(() => {});
}

export function Tour() {
  const step = useTour((s) => s.step);
  const appSettings = useApp((s) => s.appSettings);
  const project = useApp((s) => s.project);
  const [rect, setRect] = useState<DOMRect | null>(null);

  // First run: once a project is open and settings say the tour hasn't been seen.
  useEffect(() => {
    if (project && appSettings && !appSettings.tour_done && useTour.getState().step === null) useTour.getState().start();
  }, [project, appSettings]);

  useLayoutEffect(() => {
    if (step === null) return;
    const find = () => {
      const el = document.querySelector(`[data-tour="${STEPS[step]!.target}"]`);
      const next = el ? el.getBoundingClientRect() : null;
      setRect((prev) => (prev && next && prev.x === next.x && prev.y === next.y && prev.width === next.width && prev.height === next.height ? prev : next));
    };
    find();
    // The layout can still move (panels opening, fonts arriving): follow the target.
    const id = window.setInterval(find, 250);
    window.addEventListener("resize", find);
    return () => {
      window.clearInterval(id);
      window.removeEventListener("resize", find);
    };
  }, [step]);

  useEffect(() => {
    if (step === null) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") finish();
      else if (e.key === "ArrowRight" || e.key === "Enter") {
        if (step + 1 < STEPS.length) useTour.setState({ step: step + 1 });
        else finish();
      }
      else if (e.key === "ArrowLeft" && step > 0) useTour.setState({ step: step - 1 });
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [step]);

  if (step === null) return null;
  const s = STEPS[step]!;
  const last = step === STEPS.length - 1;
  // Callout beside the target: below it when there's room, else above; kept on screen.
  const W = 300;
  const r = rect ?? new DOMRect(window.innerWidth / 2 - 1, window.innerHeight / 2 - 1, 2, 2);
  const below = r.bottom + 160 < window.innerHeight;
  const left = Math.min(Math.max(12, r.left + r.width / 2 - W / 2), window.innerWidth - W - 12);
  const top = below ? Math.min(r.bottom + 10, window.innerHeight - 170) : Math.max(12, r.top - 160);
  return (
    <div className="fixed inset-0 z-[60]" role="dialog" aria-label="Tour" aria-modal>
      {/* Dim everything except the target (four panels around it), and outline the target. */}
      {[
        { left: 0, top: 0, width: "100%", height: Math.max(0, r.top - 4) },
        { left: 0, top: r.bottom + 4, width: "100%", bottom: 0 },
        { left: 0, top: r.top - 4, width: Math.max(0, r.left - 4), height: r.height + 8 },
        { left: r.right + 4, top: r.top - 4, right: 0, height: r.height + 8 },
      ].map((st, i) => (
        <div key={i} className="absolute bg-black/45" style={st} />
      ))}
      <div className="pointer-events-none absolute rounded-lg outline-2 outline-accent" style={{ left: r.left - 4, top: r.top - 4, width: r.width + 8, height: r.height + 8 }} />
      <div className="absolute flex flex-col gap-2 rounded-(--radius-sheet) bg-raised p-4 shadow-(--shadow-float) ring-1 ring-separator-strong" style={{ left, top, width: W }}>
        <div className="flex items-baseline justify-between">
          <span className="text-[13.5px] font-semibold">{s.title}</span>
          <span className="text-[11px] text-text-3 tabular">
            {step + 1} of {STEPS.length}
          </span>
        </div>
        <p className="text-[12.5px] leading-snug text-text-2">{s.text}</p>
        <div className="mt-1 flex items-center gap-2">
          <Button variant="plain" onClick={finish}>
            Skip tour
          </Button>
          <span className="flex-1" />
          {step > 0 && <Button onClick={() => useTour.setState({ step: step - 1 })}>Back</Button>}
          <Button variant="primary" onClick={() => (last ? finish() : useTour.setState({ step: step + 1 }))}>
            {last ? "Done" : "Next"}
          </Button>
        </div>
      </div>
    </div>
  );
}
