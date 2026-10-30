// Local AI views (SPEC §18, PLAN P12.7): People in the sidebar, the Transcript panel with
// transcript and visual search, AI progress in the status bar, and Settings → AI Models.

import * as CM from "@radix-ui/react-context-menu";
import { Download, FileText, Image as ImageIcon, Search, Trash2, UserRound, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api, type AiModel, type AppSettings, type SearchHit, type Transcript } from "../api";
import { Button, cx, Segmented, Spinner, Switch } from "../components/ui";
import { pickExportPath } from "../lib/dialogs";
import { formatClock } from "../lib/typed";
import { useApp } from "../store";

// ---------- People (sidebar) ----------

function FaceThumb({ clip, us, box, size = 20 }: { clip: number; us: number; box: [number, number, number, number] | null; size?: number }) {
  const media = useApp((s) => s.clips.find((c) => c.clip === clip)?.media);
  if (media === undefined) return <UserRound size={14} />;
  // Crop a square around the face box (frame fractions) out of a small frame; assumes 16:9.
  const [x, y, w, h] = box ?? [0.3, 0.1, 0.4, 0.6];
  const aspect = 16 / 9;
  const sw = Math.min(1, Math.max(w, h / aspect) * 1.4);
  const sh = Math.min(1, sw * aspect);
  const left = Math.min(Math.max(0, x + w / 2 - sw / 2), 1 - sw);
  const top = Math.min(Math.max(0, y + h / 2 - sh / 2), 1 - sh);
  return (
    <span
      aria-hidden
      className="inline-block shrink-0 rounded-full bg-control ring-1 ring-separator"
      style={{
        width: size,
        height: size,
        backgroundImage: `url(${api.frameUrl(media, us, 320)})`,
        backgroundSize: `${100 / sw}% ${100 / sh}%`,
        backgroundPosition: `${sw < 1 ? (left / (1 - sw)) * 100 : 50}% ${sh < 1 ? (top / (1 - sh)) * 100 : 50}%`,
      }}
    />
  );
}

export function PeopleSection({ item }: { item: (active: boolean) => string }) {
  const { people, library, setLibrary, renamePerson, mergePeople } = useApp();
  const [naming, setNaming] = useState<{ id: number; name: string } | null>(null);
  if (!people.length) return null;
  const commit = () => {
    if (naming && naming.name.trim()) void renamePerson(naming.id, naming.name.trim());
    setNaming(null);
  };
  return (
    <div>
      <div className="section-header mb-1 px-2">People</div>
      {people.map((p) => {
        const active = library.kind === "person" && library.id === p.id;
        return (
          <CM.Root key={p.id}>
            <CM.Trigger asChild>
              <div className={item(active)} role="button" tabIndex={0} onClick={() => setLibrary({ kind: "person", id: p.id })} onDoubleClick={() => setNaming({ id: p.id, name: p.name })}>
                {p.cover ? <FaceThumb clip={p.cover.clip} us={p.cover.us} box={p.cover.box} size={18} /> : <UserRound size={14} />}
                {naming?.id === p.id ? (
                  <input
                    autoFocus
                    aria-label="Rename person"
                    value={naming.name}
                    onChange={(e) => setNaming({ ...naming, name: e.target.value })}
                    onBlur={commit}
                    onKeyDown={(e) => (e.key === "Enter" ? commit() : e.key === "Escape" && setNaming(null))}
                    className="h-5 min-w-0 flex-1 rounded border border-accent bg-control px-1 text-[12px]"
                  />
                ) : (
                  <span className="min-w-0 flex-1 truncate">{p.name}</span>
                )}
                <span className="font-mono text-[11px] text-text-3 tabular">{p.clips.length}</span>
              </div>
            </CM.Trigger>
            <CM.Portal>
              <CM.Content className="z-50 min-w-44 rounded-lg bg-raised p-1 shadow-(--shadow-float) ring-1 ring-separator-strong">
                <CM.Item className="flex h-6 items-center rounded px-2 text-[13px] outline-none data-[highlighted]:bg-accent data-[highlighted]:text-accent-fg" onSelect={() => setNaming({ id: p.id, name: p.name })}>
                  Rename…
                </CM.Item>
                {people.length > 1 && (
                  <CM.Sub>
                    <CM.SubTrigger className="flex h-6 items-center rounded px-2 text-[13px] outline-none data-[highlighted]:bg-accent data-[highlighted]:text-accent-fg">Same person as ›</CM.SubTrigger>
                    <CM.Portal>
                      <CM.SubContent className="z-50 min-w-40 rounded-lg bg-raised p-1 shadow-(--shadow-float) ring-1 ring-separator-strong">
                        {people
                          .filter((o) => o.id !== p.id)
                          .map((o) => (
                            <CM.Item key={o.id} className="flex h-6 items-center rounded px-2 text-[13px] outline-none data-[highlighted]:bg-accent data-[highlighted]:text-accent-fg" onSelect={() => void mergePeople(o.id, p.id)}>
                              {o.name}
                            </CM.Item>
                          ))}
                      </CM.SubContent>
                    </CM.Portal>
                  </CM.Sub>
                )}
              </CM.Content>
            </CM.Portal>
          </CM.Root>
        );
      })}
    </div>
  );
}

// ---------- Transcript panel ----------

const SPEAKER_COLOURS = ["text-accent", "text-[oklch(0.65_0.15_150)]", "text-[oklch(0.68_0.15_60)]", "text-[oklch(0.62_0.16_320)]"];

function SearchBox() {
  const { clips, setPlayhead, ai } = useApp();
  const [q, setQ] = useState("");
  const [visual, setVisual] = useState(false);
  const [hits, setHits] = useState<SearchHit[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const names = useMemo(() => new Map(clips.map((c) => [c.clip, c.name])), [clips]);
  const anyTranscript = [...ai.values()].some((a) => a.transcript);
  const anyVisual = [...ai.values()].some((a) => a.clipDone);
  const run = async () => {
    if (!q.trim()) return setHits(null);
    setBusy(true);
    setError(null);
    try {
      setHits(await api.aiSearch(q, visual));
    } catch (e) {
      setError(String(e));
      setHits([]);
    }
    setBusy(false);
  };
  return (
    <div className="flex flex-col gap-2 border-b border-separator px-3 py-3">
      <div className="flex items-center gap-2">
        <label className="flex h-7 min-w-0 flex-1 items-center gap-1.5 rounded-(--radius-control) border border-separator-strong bg-control px-2">
          <Search size={13} className="shrink-0 text-text-3" />
          <input
            aria-label="Search footage"
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void run()}
            placeholder={visual ? "Describe a shot: “boat at sunset”" : "Words someone says"}
            className="min-w-0 flex-1 bg-transparent text-[13px] outline-none"
          />
          {busy && <Spinner size={11} />}
        </label>
      </div>
      <Segmented
        size="sm"
        label="Search by"
        value={visual ? "visual" : "words"}
        onChange={(v) => {
          setVisual(v === "visual");
          setHits(null);
        }}
        options={[
          { value: "words", label: "Spoken words", title: anyTranscript ? undefined : "Needs transcripts (Settings → AI Models)" },
          { value: "visual", label: "What's on screen", title: anyVisual ? undefined : "Needs shot analysis (Settings → AI Models)" },
        ]}
      />
      {error && <div className="text-[11px] text-danger">{error}</div>}
      {hits && (
        <div className="flex max-h-56 flex-col gap-0.5 overflow-y-auto">
          {hits.length === 0 && <div className="text-[12px] text-text-3">No matches.</div>}
          {hits.map((h, i) => (
            <button key={i} className="flex flex-col rounded-md px-2 py-1 text-left hover:bg-control-hover" onClick={() => setPlayhead(h.clip, h.startUs)}>
              <span className="flex items-center gap-1.5 text-[11px] text-text-3">
                {visual ? <ImageIcon size={11} /> : <FileText size={11} />}
                <span className="truncate">{names.get(h.clip) ?? `Clip ${h.clip}`}</span>
                <span className="font-mono tabular">{formatClock(h.startUs)}</span>
              </span>
              {!visual && <span className="line-clamp-2 text-[12px]">{h.text}</span>}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function TranscriptPanel() {
  const { selectedClip, playheadUs, setPlayhead, ai, appSettings, clips } = useApp();
  const [t, setT] = useState<Transcript | null | "loading">(null);
  const info = selectedClip !== null ? ai.get(selectedClip) : undefined;
  useEffect(() => {
    if (selectedClip === null || !info?.transcript) return setT(null);
    let live = true;
    setT("loading");
    api.transcriptGet(selectedClip).then(
      (r) => live && setT(r),
      () => live && setT(null),
    );
    return () => {
      live = false;
    };
  }, [selectedClip, info?.transcript]);
  const name = clips.find((c) => c.clip === selectedClip)?.name ?? "clip";
  const consent = appSettings?.ai_consent;
  return (
    <div className="flex h-full flex-col">
      <SearchBox />
      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
        {!consent ? (
          <div className="flex flex-col items-center gap-2 py-6 text-center text-[12px] text-text-2">
            Transcripts, People and search use local AI models that run on this PC.
            <Button onClick={() => void api.openSettings()}>Set up AI…</Button>
          </div>
        ) : selectedClip === null ? (
          <div className="py-6 text-center text-[12px] text-text-3">Select a clip to read its transcript.</div>
        ) : t === "loading" ? (
          <div className="flex justify-center py-6">
            <Spinner />
          </div>
        ) : !t ? (
          <div className="py-6 text-center text-[12px] text-text-3">{info?.transcript === false && appSettings?.ai_transcripts ? "Not transcribed yet: it will be once analysis finishes." : "No transcript for this clip."}</div>
        ) : (
          <>
            <div className="mb-2 flex items-center justify-between">
              <span className="section-header">Transcript · {t.language.toUpperCase() || "?"}</span>
              <span className="flex gap-1">
                {(["srt", "vtt"] as const).map((ext) => (
                  <button
                    key={ext}
                    className="rounded px-1.5 py-0.5 text-[11px] text-text-2 ring-1 ring-separator-strong hover:bg-control-hover"
                    onClick={async () => {
                      const p = await pickExportPath(name, ext, ext === "srt" ? "SubRip subtitles" : "WebVTT subtitles", "Export transcript");
                      if (p) await api.transcriptExport(selectedClip, p, null, null).then(() => useApp.getState().toast(`Saved ${p.split(/[\\/]/).pop()}`), (e) => useApp.getState().toast(String(e), "error"));
                    }}
                  >
                    {ext.toUpperCase()}
                  </button>
                ))}
              </span>
            </div>
            <div className="flex flex-col gap-2">
              {t.lines.map((l, i) => {
                const now = playheadUs >= l.startUs && playheadUs < l.endUs;
                const flub = t.flubs.some(([a, b]) => a < l.endUs && b > l.startUs);
                const words = t.words.filter((w) => w.startUs >= l.startUs - 1 && w.startUs < l.endUs);
                return (
                  <button key={i} onClick={() => setPlayhead(selectedClip, l.startUs)} className={cx("flex gap-2 rounded-md px-1.5 py-1 text-left", now ? "bg-accent-soft" : "hover:bg-control-hover")}>
                    <span className="w-12 shrink-0 pt-px font-mono text-[10px] text-text-3 tabular">{formatClock(l.startUs)}</span>
                    <span className="min-w-0 flex-1 text-[12px] leading-snug">
                      {l.speaker !== null && (info?.speakers ?? 0) > 1 && <span className={cx("mr-1 font-medium", SPEAKER_COLOURS[l.speaker % SPEAKER_COLOURS.length])}>S{l.speaker + 1}</span>}
                      {words.length
                        ? words.map((w, k) => (
                            <span key={k} className={cx(w.filler && "text-text-3 line-through decoration-text-3/50", w.prob < 0.4 && "underline decoration-dotted decoration-text-3")} title={w.filler ? "Filler" : w.prob < 0.4 ? "Low confidence" : undefined}>
                              {k ? " " : ""}
                              {w.text}
                            </span>
                          ))
                        : l.text}
                      {flub && <span className="ml-1 rounded bg-warning/15 px-1 text-[10px] text-warning" title="A repeated or restarted phrase">flub</span>}
                    </span>
                  </button>
                );
              })}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

// ---------- Status bar ----------

export function AiStatus() {
  const { aiJob, clips } = useApp();
  if (!aiJob) return null;
  const name = clips.find((c) => c.clip === aiJob.clip)?.name;
  const stage = aiJob.stage === "vision" ? "faces & shots" : aiJob.stage === "transcript" ? "transcribing" : aiJob.stage === "speakers" ? "speakers" : "";
  return (
    <span className="flex items-center gap-1.5 tabular" title={name}>
      <Spinner size={10} /> AI · {aiJob.index + 1} of {aiJob.count} · {stage}
      {aiJob.stage === "vision" ? ` ${Math.round(aiJob.fraction * 100)}%` : ""}
      <button aria-label="Stop AI analysis" className="rounded text-text-3 hover:text-text" onClick={() => void api.aiCancel()}>
        <X size={11} />
      </button>
    </span>
  );
}

// ---------- Settings → AI Models ----------

const MODULES: { key: "ai_faces" | "ai_shots" | "ai_transcripts" | "ai_speakers"; module: AiModel["module"]; label: string; hint: string }[] = [
  { key: "ai_faces", module: "faces", label: "Faces and People", hint: "Finds faces, groups them into people, and scores faces in focus." },
  { key: "ai_shots", module: "clip", label: "Shot types and visual search", hint: "Wide, medium, close-up…; a look score; search by describing a shot." },
  { key: "ai_transcripts", module: "transcripts", label: "Transcripts", hint: "Searchable transcripts, flubs, sentence-aware selects, subtitles." },
  { key: "ai_speakers", module: "speakers", label: "Speakers", hint: "Who speaks when, in interviews and conversations." },
];

export const WHISPER: { id: string; label: string }[] = [
  { id: "whisper-tiny", label: "Tiny · fastest" },
  { id: "whisper-base", label: "Base · fast" },
  { id: "whisper-small", label: "Small · balanced" },
  { id: "whisper-medium", label: "Medium · accurate" },
  { id: "whisper-large-v3-turbo", label: "Large v3 Turbo · most accurate" },
];

const LANGUAGES: [string, string][] = [
  ["", "Detect automatically"],
  ["en", "English"],
  ["es", "Spanish"],
  ["fr", "French"],
  ["de", "German"],
  ["it", "Italian"],
  ["pt", "Portuguese"],
  ["nl", "Dutch"],
  ["ja", "Japanese"],
  ["zh", "Chinese"],
  ["ko", "Korean"],
  ["ar", "Arabic"],
  ["hi", "Hindi"],
  ["ru", "Russian"],
  ["yo", "Yoruba"],
  ["sw", "Swahili"],
];

function bytes(b: number) {
  if (b < 1e6) return `${Math.max(1, Math.round(b / 1e3))} KB`;
  if (b < 1e9) return `${Math.round(b / 1e6)} MB`;
  return `${(b / 1e9).toFixed(1)} GB`;
}

/** Models a module needs with these settings. */
export function neededModels(module: AiModel["module"], s: AppSettings, all: AiModel[]): AiModel[] {
  return all.filter((m) => m.module === module && (module !== "transcripts" || m.id === s.whisper_model));
}

export function AiTab({ s, update, Row }: { s: AppSettings; update: (p: Partial<AppSettings>) => void; Row: (p: { label: string; hint?: string; children: React.ReactNode }) => React.ReactElement }) {
  const [info, setInfo] = useState<{ models: AiModel[]; dir: string; bytes: number } | null>(null);
  const [progress, setProgress] = useState<Record<string, number>>({});
  const [error, setError] = useState<string | null>(null);
  const refresh = () => api.aiModels().then(setInfo, (e) => setError(String(e)));
  useEffect(() => {
    void refresh();
    let off: (() => void) | undefined;
    void api
      .onAi((e) => {
        if (!e.model) return;
        if (e.kind === "download") setProgress((p) => ({ ...p, [e.model!]: e.fraction }));
        else {
          setProgress((p) => {
            const { [e.model!]: _, ...rest } = p;
            return rest;
          });
          if (e.kind === "download_failed") setError(`Download failed: ${e.message}`);
          void refresh();
        }
      })
      .then((u) => (off = u));
    return () => off?.();
  }, []);
  const models = info?.models ?? [];
  const missing = MODULES.filter((m) => s[m.key]).flatMap((m) => neededModels(m.module, s, models)).filter((m) => !m.installed && !m.downloading && progress[m.id] === undefined);
  const missingBytes = missing.reduce((a, m) => a + m.size, 0);

  if (!s.ai_consent)
    return (
      <div className="mx-auto flex max-w-md flex-col items-center gap-3 py-6 text-center text-[13px] text-text-2">
        <p>
          ClipPro can find faces and people, recognise shot types, transcribe speech and tell speakers apart. It uses open models (MIT and Apache-2.0 licensed) that run
          entirely on this PC. Your footage never leaves it.
        </p>
        <p className="text-[12px] text-text-3">The models are downloaded once, from their publishers on Hugging Face and GitHub, only for the modules you turn on.</p>
        <Button variant="primary" onClick={() => update({ ai_consent: true })}>
          Turn on local AI
        </Button>
      </div>
    );

  return (
    <>
      {MODULES.map((m) => {
        const need = neededModels(m.module, s, models);
        const installed = need.length > 0 && need.every((x) => x.installed);
        const downloading = need.filter((x) => progress[x.id] !== undefined);
        const f = downloading.length ? downloading.reduce((a, x) => a + (progress[x.id] ?? 0) * x.size, 0) / downloading.reduce((a, x) => a + x.size, 0) : 0;
        return (
          <Row key={m.key} label={m.label} hint={m.hint}>
            <div className="flex items-center gap-3">
              <Switch label={m.label} checked={s[m.key]} onChange={(v) => update({ [m.key]: v } as Partial<AppSettings>)} />
              <span className="text-[11px] text-text-3">
                {downloading.length ? (
                  <span className="flex items-center gap-1.5">
                    <span className="inline-block h-1 w-24 overflow-hidden rounded-full bg-control">
                      <span className="block h-full bg-accent" style={{ width: `${Math.round(f * 100)}%` }} />
                    </span>
                    {Math.round(f * 100)}%
                    <button aria-label="Cancel download" className="hover:text-text" onClick={() => downloading.forEach((x) => void api.aiDownloadCancel(x.id))}>
                      <X size={11} />
                    </button>
                  </span>
                ) : installed ? (
                  `Installed · ${bytes(need.reduce((a, x) => a + x.size, 0))}`
                ) : (
                  `${bytes(need.filter((x) => !x.installed).reduce((a, x) => a + x.size, 0))} to download`
                )}
              </span>
            </div>
            {m.module === "transcripts" && (
              <div className="mt-1 flex gap-2">
                <select aria-label="Transcript model" className="h-7 rounded-(--radius-control) border border-separator-strong bg-control px-2 text-[13px]" value={s.whisper_model} onChange={(e) => update({ whisper_model: e.target.value })}>
                  {WHISPER.map((w) => {
                    const mi = models.find((x) => x.id === w.id);
                    return (
                      <option key={w.id} value={w.id}>
                        {w.label}
                        {mi ? ` (${bytes(mi.size)})` : ""}
                      </option>
                    );
                  })}
                </select>
                <select aria-label="Transcript language" className="h-7 rounded-(--radius-control) border border-separator-strong bg-control px-2 text-[13px]" value={s.ai_language} onChange={(e) => update({ ai_language: e.target.value })}>
                  {LANGUAGES.map(([v, l]) => (
                    <option key={v} value={v}>
                      {l}
                    </option>
                  ))}
                </select>
              </div>
            )}
          </Row>
        );
      })}
      <Row label="" hint={missing.length ? `${missing.length} model file${missing.length === 1 ? "" : "s"}, ${bytes(missingBytes)}.` : undefined}>
        <div className="flex gap-2">
          <Button
            variant="primary"
            disabled={!missing.length}
            onClick={() => {
              setError(null);
              setProgress((p) => Object.fromEntries([...Object.entries(p), ...missing.map((m) => [m.id, 0])]));
              api.aiModelDownload(missing.map((m) => m.id)).catch((e) => setError(String(e)));
            }}
          >
            <Download size={13} /> {missing.length ? "Download models" : "Models ready"}
          </Button>
          <Button onClick={() => void api.aiRun().then((n) => useApp.getState().toast(n ? `AI analysis started on ${n} clip${n === 1 ? "" : "s"}` : "Nothing to analyse with AI right now."), (e) => setError(String(e)))}>
            Analyse now
          </Button>
        </div>
      </Row>
      <Row label="Repeatable results" hint="Runs AI on the CPU only, so the same footage always gives the same results. Slower.">
        <label className="flex items-center gap-2 text-[13px]">
          <Switch label="Deterministic AI" checked={s.ai_deterministic} onChange={(ai_deterministic) => update({ ai_deterministic })} />
          Deterministic AI
        </label>
      </Row>
      <Row label="Installed models" hint={info ? `${info.dir} · ${bytes(info.bytes)}` : undefined}>
        <div className="flex w-full max-w-md flex-col gap-0.5">
          {models
            .filter((m) => m.installed)
            .map((m) => (
              <div key={m.id} className="group flex items-center gap-2 rounded px-1 py-0.5 text-[12px] hover:bg-control-hover">
                <span className="min-w-0 flex-1 truncate">{m.name}</span>
                <span className="text-[11px] text-text-3">
                  {m.licence} · {bytes(m.size)}
                </span>
                <button aria-label={`Delete ${m.name}`} className="opacity-0 group-hover:opacity-70 hover:opacity-100" onClick={() => void api.aiModelDelete(m.id).then(refresh, (e) => setError(String(e)))}>
                  <Trash2 size={12} />
                </button>
              </div>
            ))}
          {info && !models.some((m) => m.installed) && <span className="text-[12px] text-text-3">None yet.</span>}
        </div>
      </Row>
      <Row label="" hint="Turning AI off keeps the models and results; nothing new is analysed.">
        <button className="text-[12px] text-text-2 underline-offset-2 hover:underline" onClick={() => update({ ai_consent: false })}>
          Turn off local AI
        </button>
      </Row>
      {error && <p className="mt-2 text-center text-[11px] text-danger">{error}</p>}
    </>
  );
}
