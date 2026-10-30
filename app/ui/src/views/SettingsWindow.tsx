// App Settings window (SPEC OTH-1 v1, PLAN P5.7): General, Performance and Analysis defaults, in a
// separate window with an icon toolbar of tabs (the macOS Preferences pattern, SPEC §14.4).
// Every change saves immediately; analysis and performance values apply to projects opened next.

import { Accessibility, Database, Gauge, Keyboard, ScanLine, SlidersHorizontal, Sparkles } from "lucide-react";
import { KeyboardTab } from "./KeyboardTab";
import { AiTab } from "./Ai";
import { pickSettingsFile } from "../lib/dialogs";
import { useEffect, useState } from "react";
import { api, isTauri, type AppSettings } from "../api";
import { cx, Segmented, Switch } from "../components/ui";
import { CaptionButtons, TrafficLights } from "./TitleBar";

type Tab = "general" | "performance" | "analysis" | "keyboard" | "cache" | "compatibility" | "ai";

const TABS: { value: Tab; label: string; icon: typeof Gauge }[] = [
  { value: "general", label: "General", icon: SlidersHorizontal },
  { value: "performance", label: "Performance", icon: Gauge },
  { value: "analysis", label: "Analysis", icon: ScanLine },
  { value: "keyboard", label: "Keyboard", icon: Keyboard },
  { value: "cache", label: "Cache", icon: Database },
  { value: "compatibility", label: "Compatibility", icon: Accessibility },
  { value: "ai", label: "AI Models", icon: Sparkles },
];

const RATES: { value: string; label: string; hint: string }[] = [
  { value: "draft", label: "Draft · 1 per second", hint: "Fastest; good enough to triage long cards" },
  { value: "fast", label: "Fast · 2 per second", hint: "" },
  { value: "standard", label: "Standard · 4 per second", hint: "The default balance" },
  { value: "detailed", label: "Detailed · 8 per second", hint: "" },
  { value: "thorough", label: "Thorough · 16 per second", hint: "Catches very brief problems" },
  { value: "all", label: "Every frame", hint: "Slowest; frame-exact flags" },
];

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[180px_1fr] items-start gap-4 py-2">
      <div className="pt-1 text-right text-[13px] text-text-2">{label}</div>
      <div className="flex flex-col items-start gap-1">
        {children}
        {hint && <div className="text-[11px] text-text-3">{hint}</div>}
      </div>
    </div>
  );
}

const select = "h-7 w-60 rounded-(--radius-control) border border-separator-strong bg-control px-2 text-[13px]";

export function SettingsWindow() {
  // `?tab=` opens a given page (screenshots, deep links from the main window).
  const [tab, setTab] = useState<Tab>(() => {
    const t = new URLSearchParams(location.search).get("tab") as Tab | null;
    return t && TABS.some((x) => x.value === t) ? t : "general";
  });
  const [s, setS] = useState<AppSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    void api.getSettings().then(setS, (e) => setError(String(e)));
    let off: (() => void) | undefined;
    void api.onSettings(setS).then((u) => (off = u));
    return () => off?.();
  }, []);
  useEffect(() => {
    if (s?.colour_blind_scores) document.documentElement.dataset.scores = "cb";
    else delete document.documentElement.dataset.scores;
    if (!s || new URLSearchParams(location.search).has("theme")) return;
    if (s.theme === "light" || s.theme === "dark") document.documentElement.dataset.theme = s.theme;
    else delete document.documentElement.dataset.theme;
  }, [s?.theme, s?.colour_blind_scores]);

  const update = (patch: Partial<AppSettings>) => {
    if (!s) return;
    const next = { ...s, ...patch };
    setS(next);
    api.setSettings(next).then(setS, (e) => setError(`Couldn't save: ${e}`));
  };
  const windows = s?.window_controls === "windows";

  return (
    <div className="flex h-screen flex-col bg-window text-text">
      <header data-tauri-drag-region className="drag flex shrink-0 flex-col border-b border-separator">
        <div data-tauri-drag-region className="flex h-(--h-titlebar) items-center">
          {!windows && <TrafficLights />}
          <div data-tauri-drag-region className="pointer-events-none flex-1 text-center text-[13px] font-semibold">
            {TABS.find((t) => t.value === tab)!.label}
          </div>
          {windows ? <CaptionButtons maximise={false} /> : <div className="w-[76px]" />}
        </div>
        <nav className="no-drag flex justify-center gap-1 pb-2" role="tablist" aria-label="Settings sections">
          {TABS.map((t) => (
            <button
              key={t.value}
              role="tab"
              aria-selected={tab === t.value}
              onClick={() => setTab(t.value)}
              className={cx("flex w-[78px] flex-col items-center gap-1 rounded-md py-1.5 text-[11px]", tab === t.value ? "bg-accent-soft text-accent-text" : "text-text-2 hover:bg-control-hover")}
            >
              <t.icon size={20} strokeWidth={1.6} />
              {t.label}
            </button>
          ))}
        </nav>
      </header>
      <main className="min-h-0 flex-1 overflow-y-auto px-6 py-4" role="tabpanel">
        {!s ? (
          <div className="text-[12px] text-text-3">{error ?? "Loading…"}</div>
        ) : tab === "general" ? (
          <>
            <Row label="Appearance">
              <Segmented
                label="Appearance"
                value={s.theme as "system" | "light" | "dark"}
                onChange={(theme) => update({ theme })}
                options={[
                  { value: "system", label: "System" },
                  { value: "light", label: "Light" },
                  { value: "dark", label: "Dark" },
                ]}
              />
            </Row>
            <Row label="Window controls" hint="Traffic lights on the left, or Windows minimise / maximise / close on the right.">
              <Segmented
                label="Window controls"
                value={s.window_controls as "mac" | "windows"}
                onChange={(window_controls) => update({ window_controls })}
                options={[
                  { value: "mac", label: "macOS style" },
                  { value: "windows", label: "Windows style" },
                ]}
              />
            </Row>
            <Row label="At start-up" hint={s.last_project ? `Last project: ${s.last_project}` : undefined}>
              <label className="flex items-center gap-2 text-[13px]">
                <Switch label="Reopen the last project" checked={s.reopen_last} onChange={(reopen_last) => update({ reopen_last })} />
                Reopen the last project
              </label>
            </Row>
          </>
        ) : tab === "performance" ? (
          <>
            <Row label="Analysis workers" hint="Clips analysed at the same time. Automatic uses your CPU count.">
              <select aria-label="Analysis workers" className={select} value={s.workers} onChange={(e) => update({ workers: Number(e.target.value) })}>
                <option value={0}>Automatic</option>
                {[1, 2, 3, 4, 6, 8, 12, 16].map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </select>
            </Row>
            <Row label="Hardware decoding" hint="Decode on the GPU (D3D11VA) when the codec allows; falls back to the CPU otherwise.">
              <label className="flex items-center gap-2 text-[13px]">
                <Switch label="Use the GPU to decode video" checked={s.gpu_decode} onChange={(gpu_decode) => update({ gpu_decode })} />
                Use the GPU to decode video
              </label>
            </Row>
            <Row label="Measurement cache" hint="Reuses analysis of the same footage across projects. Stored on this PC.">
              <label className="flex items-center gap-2 text-[13px]">
                <Switch label="Share measurements between projects" checked={s.use_cache} onChange={(use_cache) => update({ use_cache })} />
                Share measurements between projects
              </label>
            </Row>
          </>
        ) : tab === "analysis" ? (
          <>
            <Row label="Sample rate" hint={RATES.find((r) => r.value === s.sample_rate)?.hint || "How many moments per second are measured."}>
              <select aria-label="Sample rate" className={select} value={s.sample_rate} onChange={(e) => update({ sample_rate: e.target.value })}>
                {RATES.map((r) => (
                  <option key={r.value} value={r.value}>
                    {r.label}
                  </option>
                ))}
              </select>
            </Row>
            <Row label="Motion" hint="Measures each moment against the next frame: more accurate shake and blur, a little slower.">
              <label className="flex items-center gap-2 text-[13px]">
                <Switch label="Measure motion from frame pairs" checked={s.pair_sampling} onChange={(pair_sampling) => update({ pair_sampling })} />
                Measure motion from frame pairs
              </label>
            </Row>
          </>
        ) : null}
        {s && tab === "general" && (
          <Row label="Settings file" hint="Move your settings and shortcuts to another PC. Recent projects stay on this one.">
            <div className="flex gap-2">
              <button
                className="h-7 rounded-(--radius-control) px-3 text-[13px] ring-1 ring-separator-strong hover:bg-control-hover"
                onClick={async () => {
                  const p = await pickSettingsFile(true);
                  if (p) await api.settingsExport(p).catch((e) => setError(`Couldn't export: ${e}`));
                }}
              >
                Export…
              </button>
              <button
                className="h-7 rounded-(--radius-control) px-3 text-[13px] ring-1 ring-separator-strong hover:bg-control-hover"
                onClick={async () => {
                  const p = await pickSettingsFile(false);
                  if (p) await api.settingsImport(p).then(setS, (e) => setError(`Couldn't import: ${e}`));
                }}
              >
                Import…
              </button>
            </div>
          </Row>
        )}
        {s && tab === "keyboard" && <KeyboardTab s={s} update={update} />}
        {s && tab === "cache" && <CacheTab />}
        {s && tab === "compatibility" && (
          <Row label="Score colours" hint="Blue → orange instead of red → green, for red–green colour blindness.">
            <label className="flex items-center gap-2 text-[13px]">
              <Switch label="Colour-blind-safe score colours" checked={s.colour_blind_scores} onChange={(colour_blind_scores) => update({ colour_blind_scores })} />
              Colour-blind-safe score colours
            </label>
            <div className="mt-1 h-2 w-60 rounded-full" style={{ background: "linear-gradient(90deg, var(--c-score-lo), var(--c-score-mid), var(--c-score-hi))" }} />
          </Row>
        )}
        {s && tab === "ai" && <AiTab s={s} update={update} Row={Row} />}
        {s && ["performance", "analysis"].includes(tab) && <p className="mt-4 text-center text-[11px] text-text-3">Changes apply to projects you open or create from now on.</p>}
        {error && s && <p className="mt-2 text-center text-[11px] text-danger">{error}</p>}
        {!isTauri && <p className="mt-6 text-center text-[11px] text-text-3">Browser preview — settings aren't saved.</p>}
      </main>
    </div>
  );
}

function formatBytes(b: number) {
  if (b < 1e6) return `${Math.round(b / 1e3)} KB`;
  if (b < 1e9) return `${(b / 1e6).toFixed(0)} MB`;
  return `${(b / 1e9).toFixed(2)} GB`;
}

function CacheTab() {
  const [info, setInfo] = useState<{ path: string; entries: number; bytes: number } | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    api.cacheInfo().then(setInfo, () => setInfo(null));
  }, []);
  return (
    <>
      <Row label="Measurement cache" hint="Analysis results reused when the same footage is imported again, in any project.">
        <div className="text-[13px]">{info ? `${info.entries} clip${info.entries === 1 ? "" : "s"} · ${formatBytes(info.bytes)}` : "…"}</div>
        <div className="max-w-80 truncate text-[11px] text-text-3" title={info?.path}>
          {info?.path}
        </div>
      </Row>
      <Row label="" hint="Projects keep their own measurements; only re-imports into new projects get slower.">
        <button
          disabled={busy || !info || info.entries === 0}
          className="h-7 rounded-(--radius-control) px-3 text-[13px] ring-1 ring-separator-strong hover:bg-control-hover disabled:opacity-40"
          onClick={async () => {
            setBusy(true);
            await api.cacheClear().catch(() => 0);
            setInfo(await api.cacheInfo());
            setBusy(false);
          }}
        >
          {busy ? "Clearing…" : "Clear cache"}
        </button>
      </Row>
    </>
  );
}
