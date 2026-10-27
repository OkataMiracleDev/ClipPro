import { Check, ChevronDown, ChevronRight, RotateCcw } from "lucide-react";
import { useState } from "react";
import { n } from "../api";
import { linearScale, RangeSlider, SingleSlider, stopScale } from "../components/TypedSlider";
import { cx, Popover, PopoverClose, Segmented, Switch } from "../components/ui";
import { ACTIVE_FLAGS, FLAGS, TIERS } from "../lib/domain";
import { formatDuration, parseCount, parseDuration, parseScore } from "../lib/typed";
import { useApp } from "../store";
import type { Tier } from "../bindings";

const scoreScale = linearScale(0, 1, 100, (v) => `${Math.round(v * 100)}`, parseScore);
const LENGTH_STOPS: (number | null)[] = [0.1, 0.25, 0.5, 1, 1.5, 2, 3, 4, 5, 6, 8, 10, 12, 15, 20, 30, 45, 60, 90, 120, 180, 300, 600, null];
const lengthScale = stopScale(LENGTH_STOPS, formatDuration, parseDuration);
const CAP_STOPS: (number | null)[] = [1, 2, 3, 4, 5, 6, 8, 10, 15, 20, 30, 50, null];
const capScale = stopScale(CAP_STOPS, (v) => (v === null ? "none" : String(v)), parseCount);
const GAP_STOPS: (number | null)[] = [0, 0.1, 0.25, 0.5, 0.75, 1, 1.5, 2, 3, 5];
const gapScale = stopScale(GAP_STOPS, (v) => formatDuration(v ?? 0), parseDuration);
const unitScale = linearScale(0, 1, 100, (v) => `${Math.round(v * 100)}%`, (s) => {
  const r = parseScore(s);
  return r;
});

const scoreTrack = {
  background: "linear-gradient(90deg, var(--c-score-lo), var(--c-score-mid), var(--c-score-hi))",
  opacity: 0.85,
};

function Section({ title, children, defaultOpen = true }: { title: string; children: React.ReactNode; defaultOpen?: boolean }) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <section className="border-b border-separator px-4 py-3">
      <button className="section-header mb-2 flex w-full items-center gap-1" onClick={() => setOpen(!open)} aria-expanded={open}>
        {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        {title}
      </button>
      {open && <div className="flex flex-col gap-3">{children}</div>}
    </section>
  );
}

export function PresetPicker() {
  const { presets, presetName, modified, setPreset } = useApp();
  return (
    <Popover
      trigger={
        <button className="flex h-7 w-full items-center justify-between rounded-(--radius-control) border border-separator-strong bg-control px-2.5 text-[13px] hover:bg-control-hover">
          <span className="flex items-center gap-1.5 truncate">
            {presetName}
            {modified && <span className="h-1.5 w-1.5 rounded-full bg-accent" title="Modified" />}
          </span>
          <ChevronDown size={14} className="text-text-3" />
        </button>
      }
    >
      <div className="w-[280px] py-1">
        {presets.map((p) => (
          <PopoverClose asChild key={p.name}>
            <button
              onClick={() => void setPreset(p.name)}
              className="flex w-full items-start gap-2 rounded-md px-2 py-1.5 text-left hover:bg-accent hover:text-accent-fg"
            >
              <span className="mt-0.5 w-3.5">{p.name === presetName && <Check size={13} />}</span>
              <span>
                <span className="block text-[13px]">{p.name}</span>
                <span className="block text-[11px] opacity-70">{p.description}</span>
              </span>
            </button>
          </PopoverClose>
        ))}
      </div>
    </Popover>
  );
}

export function TuningPanel() {
  const { settings, updateSettings, scored, presetName, modified, setPreset } = useApp();
  if (!settings) return null;
  const runs = new Map(scored?.summary.flagRuns ?? []);
  const setTier = (i: number, t: Tier) => updateSettings((s) => ({ ...s, tiers: s.tiers.map((x, j) => (j === i ? t : x)) }));
  return (
    <div className="flex h-full flex-col overflow-y-auto" aria-label="Tuning">
      <div className="flex items-center gap-2 border-b border-separator px-4 py-3">
        <div className="min-w-0 flex-1">
          <PresetPicker />
        </div>
        {modified && (
          <button
            className="rounded p-1 text-text-3 hover:bg-control-hover hover:text-text"
            title={`Revert to ${presetName}`}
            aria-label={`Revert to ${presetName}`}
            onClick={() => void setPreset(presetName)}
          >
            <RotateCcw size={14} />
          </button>
        )}
      </div>

      <Section title="Selects">
        <RangeSlider
          label="Score threshold"
          scale={scoreScale}
          value={[n(settings.score_lo), n(settings.score_hi, 1)]}
          trackStyle={scoreTrack}
          onChange={([lo, hi]) => updateSettings((s) => ({ ...s, score_lo: lo, score_hi: hi }))}
        />
        <RangeSlider
          label="Segment length"
          scale={lengthScale}
          value={[n(settings.len_min_s), settings.len_max_s ?? null]}
          onChange={([lo, hi]) => updateSettings((s) => ({ ...s, len_min_s: lo ?? 0, len_max_s: hi }))}
        />
        <SingleSlider
          label="Max per clip"
          scale={capScale}
          value={settings.max_per_clip ?? null}
          onChange={(v) => updateSettings((s) => ({ ...s, max_per_clip: v }))}
        />
      </Section>

      <Section title="Filters">
        {(
          [
            ["metrics", "Metrics", "Score quality metrics"],
            ["flags", "Flags", "Apply flag tiers"],
            ["camera_filter", "Camera movement", "Apply the camera-movement filter"],
          ] as const
        ).map(([k, label, hint]) => (
          <label key={k} className="flex items-center justify-between" title={hint}>
            <span className="text-[12px] text-text-2">{label}</span>
            <Switch
              label={label}
              checked={settings.switches[k]}
              onChange={(v) => updateSettings((s) => ({ ...s, switches: { ...s.switches, [k]: v } }))}
            />
          </label>
        ))}
      </Section>

      <Section title="Flags">
        <div className={cx("flex flex-col gap-1.5", !settings.switches.flags && "opacity-40")}>
          {FLAGS.map((f, i) =>
            ACTIVE_FLAGS.has(f.key) ? (
              <div key={f.key} className="flex items-center justify-between gap-2" title={f.hint}>
                <span className="min-w-0 truncate text-[12px] text-text-2">
                  {f.label}
                  {runs.get(f.key) ? <span className="ml-1.5 font-mono text-[10px] text-text-3 tabular">{runs.get(f.key)}</span> : null}
                </span>
                <Segmented size="sm" label={`${f.label} tier`} options={TIERS} value={settings.tiers[i] ?? "neutral"} onChange={(t) => setTier(i, t)} />
              </div>
            ) : null,
          )}
        </div>
      </Section>

      <Section title="Advanced" defaultOpen={false}>
        <SingleSlider
          label="Gap tolerance"
          scale={gapScale}
          value={n(settings.gap_s)}
          onChange={(v) => updateSettings((s) => ({ ...s, gap_s: v ?? 0 }))}
        />
        <SingleSlider
          label="Centre weighting"
          scale={unitScale}
          value={n(settings.centre_weighting)}
          onChange={(v) => updateSettings((s) => ({ ...s, centre_weighting: v }))}
        />
        <SingleSlider
          label="Stability emphasis"
          scale={unitScale}
          value={n(settings.stability_emphasis)}
          onChange={(v) => updateSettings((s) => ({ ...s, stability_emphasis: v }))}
        />
        <SingleSlider
          label="Handles"
          scale={stopScale([0, 1, 2, 3, 4, 6, 8, 12], (v) => `${v ?? 0} samples`, (s) => {
            const r = parseCount(s.replace(/samples?/, "").trim());
            return r.ok && r.value !== null ? { ok: true, value: r.value } : { ok: false, error: "whole number of samples" };
          })}
          value={settings.handles_samples}
          onChange={(v) => updateSettings((s) => ({ ...s, handles_samples: v ?? 0 }))}
        />
      </Section>
    </div>
  );
}
