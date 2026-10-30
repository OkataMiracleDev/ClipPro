# ClipPro: Product & Technical Specification

| | |
|---|---|
| Version | 0.1 (initial spec) |
| Date | 2026-10-11 |
| Status | Approved for build |
| Platform | Windows 10 (22H2+) / Windows 11, x64 only |
| Deliverable | `ClipPro.exe` (GUI), `clippro-cli.exe` (headless), NSIS installer `ClipPro_<ver>_x64-setup.exe` |
| Companion docs | [ALGORITHMS.md](ALGORITHMS.md) (all measurement and scoring maths), [decisions/](decisions/) (architecture decision records) |

---

## 1. Product summary

ClipPro is a Windows desktop app that turns hours of raw footage into ranked selects. It measures every clip deterministically for **focus, exposure, motion and audio**. It finds the stretches that are technically usable and lets the user tune the rules live, without re-analysing. It then sends the result to Premiere Pro, DaVinci Resolve, Avid Media Composer or Final Cut Pro as a timeline with markers, or renders the selects out as media files.

ClipPro matches the full feature set of Cutdown (cutdown.app, v1.0.1, October 2026) with no feature gating, and adds:

- **Audio analysis**: loudness, clipping, silence, wind, hum, dead channels and speech presence, as a fourth scoring group.
- **Timeline round-trip**: import FCP7 XML, FCPXML, OTIO, EDL and AAF. Re-score an existing edit, swap weak shots for better takes, sync multicam by timecode or audio waveform, and build stringouts.
- **Optional local AI modules**: face detection and recognition (People), Whisper transcripts with word-level search, shot-type classification, semantic visual search, an aesthetic score, best-thumbnail picking, and take grouping with best-take ranking.
- **Workflow extras**: verified card offload (checksums and an MHL report), proxy generation, rendering selects to files, a headless CLI, manual overrides, ratings, smart collections and undo/redo for tuning.

Everything runs offline on the user's PC. Nothing is uploaded. There are no accounts and no tiers.

## 2. Goals and non-goals

### Goals
1. **Feature parity with Cutdown, and more**: every capability in §5 tagged `CORE` ships in 1.0.
2. **Deterministic core**: the same footage, settings and engine version always produce the same selects. Every score traces back to inspectable measurements.
3. **Fast**: analysis at least 20× realtime on a mid-range GPU and at least 5× realtime CPU-only (§22). Re-scoring a 100-hour project takes under 150 ms.
4. **Non-destructive**: source media is never modified. Measurements are taken once; tuning is free and reversible.
5. **Works on any Windows PC**: uses any Direct3D 11 hardware GPU when present, otherwise falls back fully to the CPU, with feature parity and only a speed difference.
6. **Universal NLE handoff**: exports that open correctly in Premiere Pro, Resolve, Avid and FCP, with markers.

### Non-goals (1.0)
- macOS, Linux and Windows on ARM builds. The architecture stays portable (§6.4), but nothing ships for them.
- Being an NLE. There is no trimming UI beyond selecting segments, no effects and no multi-track editing.
- Cloud features, accounts, telemetry, collaboration servers or licence keys.
- AI-driven core scores. AI modules are opt-in and live in a separate "Content" group (§18).
- Decoding camera RAW natively in 1.0. As in Cutdown, RAW formats need a linked proxy. Native BRAW and R3D are on the post-1.0 list (§28).

## 3. Product principles
1. **Measure once, decide many times.** Analysis writes immutable measurements; everything after that is a pure function of measurements plus settings.
2. **Explain every number.** Any score shown in the UI can be expanded down to its sub-metrics and raw measurements.
3. **The machine proposes, the editor disposes.** Manual overrides (force a segment in or out, adjust its in/out points, rate it) always win over the algorithm.
4. **Offline and private.** The network is used only for the optional update check and optional AI model downloads, both user-initiated or user-approved.
5. **Degrade gracefully.** If the GPU, a hardware decoder or a model is missing, ClipPro falls back to something slower that still works, and says so.

## 4. Users and core workflows

**Users:** editors, assistant editors, DITs, producers, colourists, shooters, event and wedding videographers, documentary and reality teams with high shooting ratios, content creators, and archivists with footage that cannot be uploaded.

### W1: Selects pass (the primary flow)
1. Create a project and add folders, cards, files or a watch folder.
2. Analysis starts automatically. Clips fill in as they finish, and the user can work on finished clips straight away.
3. Pick a preset, then tune the score threshold, segment length, max per clip, metric weights, flags and camera-movement filter. The selects update live.
4. Review in the viewer with overlays, jump between segments, and manually force segments in or out.
5. Export to an NLE (markers included), export a report, or render the selects to files.
6. Save the project. Reopening restores everything without re-analysing.

### W2: Re-score an existing edit
Import an FCPXML, XML, OTIO, EDL or AAF timeline. ClipPro relinks the media, analyses the used ranges plus handles, flags weak shots on the timeline, suggests better alternatives from the same take group, and exports the timeline back with markers.

### W3: Multicam / multi-recorder sync
Select clips, then sync by timecode or audio waveform. ClipPro creates sync groups, exports multicam or stacked sequences, and scores per angle so the best-scoring angle can be picked at each moment.

### W4: Interview / talking-head pull
Enable the Transcripts and Faces modules. Search for what was said, filter to segments where a named person's face is in focus, snap segment edges to sentence boundaries, and export with transcript text in the markers.

### W5: DIT offload
Offload a card to one or more destinations with checksum verification and an MHL report. Analysis starts on the destination copy while the offload runs.

### W6: Headless / batch
`clippro-cli analyze <paths> --preset "Talking Head" --export fcpxml --out selects.fcpxml`, for automation and render-farm-style use.

## 5. Feature catalogue

Tags: `CORE` means Cutdown parity and must ship in 1.0. `PLUS` means beyond Cutdown and is in 1.0 unless marked `LATER`. Each feature lists the plan phase that builds it.

### 5.1 Ingest
| ID | Feature | Tag | Phase |
|---|---|---|---|
| ING-1 | Add files, folders (recursive), drives and network shares; drag and drop | CORE | P3 |
| ING-2 | Native decode of common camera and delivery codecs (§9.2) with no transcoding | CORE | P1 |
| ING-3 | Proxy linking (manual, plus auto-match by name, duration and timecode); analyse the proxy, export pointing at the original | CORE | P3 |
| ING-4 | Watch folders with continuous ingest of growing transfers (waits for the file size to settle) | CORE | P9 |
| ING-5 | Metadata extraction: timecode, reel/tape, camera make/model, lens, ISO/shutter/WB where present, creation date, GPS, audio channels | PLUS | P1 |
| ING-6 | Verified offload: copy cards to N destinations with xxHash64, MD5 or SHA-1, plus an ASC MHL v2 report | PLUS | P13 |
| ING-7 | Proxy generation (ProRes Proxy, DNxHR LB, H.264 via hardware encoders), auto-linked | PLUS | P13 |
| ING-8 | Global measurement cache: a file analysed in any project is reused in a new one (matched by fingerprint) | PLUS | P3 |
| ING-9 | Duplicate-file detection across the project (same fingerprint or perceptual hash) | PLUS | P3 |

### 5.2 Analysis
| ID | Feature | Tag | Phase |
|---|---|---|---|
| ANA-1 | Single decode pass per clip; sample rates Draft 1, Fast 2, Standard 4 (default), Detailed 8, Thorough 16 per second, or All frames; last frame always sampled | CORE | P3 |
| ANA-2 | Luma-only working image, max 960 px on the long edge, active-area (letterbox) detection | CORE | P2 |
| ANA-3 | Focus group: Sharpness, Detail, Focus Stability | CORE | P2 |
| ANA-4 | Exposure group: Grading Flexibility, Flicker, Luma, Contrast (+ Noise, White-balance cast as PLUS) | CORE/PLUS | P2 |
| ANA-5 | Motion group: Camera Motion, Subject Motion, Motion Stability, plus global motion model (pan/tilt direction, zoom, rotation) with confidence | CORE | P2 |
| ANA-6 | Pair sampling: each sample also measures the next frame to get true instantaneous motion and blur | PLUS | P2 |
| ANA-7 | Compute profiles: turn the Focus, Exposure, Motion or Audio groups on or off (at least one must stay on); already-analysed clips keep their data | CORE | P3 |
| ANA-8 | Hardware decode (D3D11VA) with CPU fallback; GPU measurement (D3D11 compute) with CPU fallback | CORE | P1/P8 |
| ANA-9 | Worker pool (Auto = about half the logical cores, user-settable 1..N); limit on concurrent large clips (6K and up); long clips split across workers and stitched | CORE | P3 |
| ANA-10 | Pause, resume and stop, with progress kept per clip (checkpointed); queue reordering ("Move to next"); opening a clip's overlay moves it to the front of the queue | CORE | P3 |
| ANA-11 | Audio group: Loudness, Peak/Clipping, Noise floor, Wind, Hum, Speech presence, Channel health | PLUS | P10 |
| ANA-12 | Reproducible mode: forces the CPU path for bit-identical measurements across machines | PLUS | P8 |

### 5.3 Flags
| ID | Feature | Tag | Phase |
|---|---|---|---|
| FLG-1 | Clipping (≥1.5% of active pixels at hard black or white) | CORE | P2 |
| FLG-2 | Duplicate (near-identical to the previous sample) | CORE | P2 |
| FLG-3 | Solid colour (leaders, bars, fades, lens cap) | CORE | P2 |
| FLG-4 | Hard cut (abrupt scene change between samples) | CORE | P2 |
| FLG-5 | Motion blur | CORE | P2 |
| FLG-6 | Shaky | CORE | P2 |
| FLG-7 | Low light | CORE | P2 |
| FLG-8 | Focus hunting, Colour bars/test pattern, Horizon tilt, Audio clipping, Silence, Wind, Hum (PLUS flags) | PLUS | P2/P10 |
| FLG-9 | Five-position tier per flag: Force exclude / Disfavour ×0.6 / Neutral / Favour ×1.2 / Force include | CORE | P4 |
| FLG-10 | Per-flag run counts across the whole project | CORE | P5 |

### 5.4 Tuning and scoring
| ID | Feature | Tag | Phase |
|---|---|---|---|
| TUN-1 | Two-ended Score Threshold slider (segment mean must fall between the low and high handles) | CORE | P4/P5 |
| TUN-2 | Two-ended Segment Length slider (min 100 ms; max unlimited); typed values such as `4`, `4.5`, `1:30`, `90s`, `1h`, `none` | CORE | P4/P5 |
| TUN-3 | Max per Clip (keeps the highest-scoring segments; `none` for unlimited) | CORE | P4/P5 |
| TUN-4 | Metric weighting: drag a radar polygon (Focus, Exposure, Motion, Audio, and Content when AI is on); 0% excludes a group | CORE | P9 |
| TUN-5 | Sub-metric preference sliders, bipolar around Neutral (direction sets what is rewarded, distance sets strength), with on/off toggles; Luma and Contrast are target-type sliders and off by default | CORE | P9 |
| TUN-6 | Camera-movement filter: pan/tilt direction wheel with acceptance spread, zoom arc, rotate arc, Static-only, Mirror | CORE | P9 |
| TUN-7 | Filter master switches (Metrics, Flags, Camera movement) to isolate each filter's effect | CORE | P5 |
| TUN-8 | Hidden preset parameters: gap tolerance, centre-frame weighting, stability emphasis, handles (exposed in an "Advanced" drawer) | CORE/PLUS | P4 |
| TUN-9 | Live re-scoring from in-memory measurements (no disk reads), non-blocking, with a "Re-scoring…" indicator for large projects | CORE | P4 |
| TUN-10 | Undo/redo history for every tuning change; named snapshots ("Compare A/B settings") | PLUS | P9 |
| TUN-11 | Manual overrides: force a segment in or out, nudge or set segment in/out, add a manual segment, star rating 0–5, colour label, note | PLUS | P9 |
| TUN-12 | Transcript-aware edges: snap segment boundaries to word or sentence boundaries (needs Transcripts) | PLUS | P12 |
| TUN-13 | Best-take-only filter (needs take grouping) | PLUS | P12 |

### 5.5 Presets
| ID | Feature | Tag | Phase |
|---|---|---|---|
| PRE-1 | Built-ins: All-Rounder (default), Talking Head, Social Media, Cleanup, Action/Sport, Long Takes, Drone/Aerial, Whip Pans, Locked-Off, Twist, Tilt, Pan | CORE | P4/P9 |
| PRE-2 | PLUS built-ins: Interview (faces + transcript), Wedding/Event, B-roll, Documentary Verité, Product/Tabletop, Gimbal Walk, Audio-First (podcast/vlog) | PLUS | P9/P12 |
| PRE-3 | Custom presets: save, rename, duplicate, delete, import and export as human-readable TOML (§13) | CORE | P9 |

### 5.6 Viewer, overlays and inspection
| ID | Feature | Tag | Phase |
|---|---|---|---|
| VIE-1 | Frame-accurate playback with audio; JKL shuttle; step by frame or by sample; loop segment | CORE | P6 |
| VIE-2 | Focus overlay (tile sharpness heatmap and focus peaking) | CORE | P6 |
| VIE-3 | Exposure overlays: false colour, zebras (two thresholds), histogram, waveform monitor | CORE/PLUS | P6 |
| VIE-4 | Motion overlay: motion vectors, global-motion gizmo (pan/tilt arrow, zoom ring, rotation arc) with confidence | CORE | P6 |
| VIE-5 | Frame inspector: Overall plus per-group, sub-metric and raw values for the current sample; pass/fail with reasons ("failed: Shaky (force exclude)") | CORE | P6 |
| VIE-6 | Log preview: built-in profiles for S-Log3, LogC3, LogC4, V-Log, C-Log2, C-Log3, N-Log, F-Log2, D-Log, Apple Log, RED Log3G10, BMD Film Gen5, plus user `.cube` LUTs | CORE | P6 |
| VIE-7 | Before/after split, and an A/B compare of two segments side by side | PLUS | P6 |
| VIE-8 | Audio meters (peak/RMS/LUFS), per-channel solo/mute, waveform under the video | PLUS | P10 |

### 5.7 Timeline, score chart and selects view
| ID | Feature | Tag | Phase |
|---|---|---|---|
| TML-1 | Project timeline of all clips, with a per-sample score bar (red-to-green), segment blocks, flag lanes and an audio waveform lane | CORE | P5 |
| TML-2 | Score chart: scatter of segments (x = duration, y = mean score), draggable dashed threshold lines, hover details, click to jump, rings for the current segment and for other segments of the same clip; totals (count, total duration, % of footage) | CORE | P5 |
| TML-3 | Selects-only view: packs selects together (ripple style), shown with a cyan outline, non-destructive | CORE | P5 |
| TML-4 | Sorts: timeline order, score high→low or low→high, duration long→short or short→long | CORE | P5 |
| TML-5 | Group by: clip, camera, date, reel, person, shot type, take group | PLUS | P12 |

### 5.8 Export and interchange
| ID | Feature | Tag | Phase |
|---|---|---|---|
| EXP-1 | FCP7 XML (xmeml v5), used by Premiere Pro and Resolve | CORE | P7 |
| EXP-2 | FCPXML (1.10 baseline, version selectable), used by Final Cut Pro and Resolve | CORE | P7 |
| EXP-3 | AAF (Avid Media Composer, Premiere Pro) | CORE | P7b |
| EXP-4 | OpenTimelineIO `.otio` | CORE | P7 |
| EXP-5 | CMX3600 EDL; Avid ALE; Avid marker list `.txt`; SRT/VTT (transcripts) | PLUS | P7/P12 |
| EXP-6 | Markers on every select with templated name, comment and colour (tokens such as `{score}`, `{flags}`, `{shot_type}`, `{people}`, `{transcript}`) | CORE | P7/P9 |
| EXP-7 | Track layout: selects only (one track) or selected plus unselected (two tracks); handles; sequence vs subclips/bins | CORE | P7 |
| EXP-8 | Reports: CSV, JSON, a self-contained HTML report with thumbnails, PDF contact sheet | CORE/PLUS | P7 |
| EXP-9 | Render selects to media files (stream copy when cut points allow, otherwise re-encode to a chosen codec) | PLUS | P13 |
| EXP-10 | Import timelines (FCP7 XML, FCPXML, OTIO, EDL, AAF), relink media, re-score the edit, swap in alternative takes, export back | PLUS | P11 |
| EXP-11 | Multicam sync (timecode, audio waveform) exported as FCPXML mc-clip, or as stacked tracks in XML/OTIO | PLUS | P11 |
| EXP-12 | Stringout builder: grouped selects with optional slugs or gaps between groups | PLUS | P11 |

### 5.9 Projects
| ID | Feature | Tag | Phase |
|---|---|---|---|
| PRJ-1 | Single-file project `.clippro` (SQLite), save, save-as, reopen with no re-analysis | CORE | P3 |
| PRJ-2 | Autosave every 30 s plus on every structural change; crash recovery on next launch | CORE | P3 |
| PRJ-3 | Relink missing media (by path, name, fingerprint or timecode) | CORE | P3 |
| PRJ-4 | Bins/collections and smart collections (saved filter queries) | PLUS | P9 |
| PRJ-5 | Recent projects list; project templates (a preset plus export settings) | PLUS | P9 |

### 5.10 AI modules (optional, offline, opt-in)
| ID | Feature | Tag | Phase |
|---|---|---|---|
| AI-1 | Model manager: list, download (hash-verified), delete, show disk use; nothing downloads without consent | PLUS | P12 |
| AI-2 | Faces: detection, landmarks, embeddings and clustering into People; naming; face-in-focus, eyes-open and face-size measures | PLUS | P12 |
| AI-3 | Transcripts: Whisper speech-to-text with word timestamps, language detection, speaker turns; full-text search; filler and flub detection | PLUS | P12 |
| AI-4 | Shot type: wide / medium / close-up / extreme close-up / insert / aerial / over-the-shoulder; indoor/outdoor; day/night | PLUS | P12 |
| AI-5 | Semantic visual search ("sunset over water") via CLIP embeddings | PLUS | P12 |
| AI-6 | Aesthetic score as an optional sub-metric in the Content group | PLUS | P12 |
| AI-7 | Best thumbnail per segment | PLUS | P12 |
| AI-8 | Take grouping (near-duplicate segments across clips) and best-take ranking | PLUS | P12 |

### 5.11 Other
| ID | Feature | Tag | Phase |
|---|---|---|---|
| OTH-1 | Settings: General, Performance, Compatibility, Cache, AI Models, Keyboard (rebindable) | CORE | P5/P9 |
| OTH-2 | In-app help with a searchable manual; first-run tour | CORE | P14 |
| OTH-3 | Headless CLI `clippro-cli.exe` | PLUS | P13 |
| OTH-4 | Auto-update (signed, user-approved), changelog viewer | PLUS | P14 |
| OTH-5 | Local diagnostics bundle (logs, system info, no media) for bug reports | PLUS | P14 |

## 6. System architecture

### 6.1 Overview

```
┌──────────────────────────── ClipPro.exe (Tauri 2 host process) ─────────────────────────────┐
│                                                                                             │
│  WebView2 UI (React + TS)                       Rust backend (tokio + rayon + std threads)  │
│  ┌──────────────────────────────┐   IPC cmds    ┌─────────────────────────────────────────┐ │
│  │ Clip list · Viewer (WebGL2)  │◄─────────────►│ app (Tauri commands, events, state)     │ │
│  │ Timeline/Chart (Canvas/WebGL)│   events      │  ├─ project   (SQLite .clippro, cache)  │ │
│  │ Tuning · Export · Settings   │               │  ├─ analysis  (scheduler, workers)      │ │
│  └──────────────▲───────────────┘               │  │    ├─ media    (FFmpeg, D3D11VA)      │ │
│                 │ SharedBuffer (zero-copy       │  │    ├─ measure  (CPU SIMD | D3D11 CS)  │ │
│                 │ preview frames + overlays)    │  │    ├─ audio    (R128, FFT, VAD)       │ │
│                 └───────────────────────────────┤  │    └─ ai       (ONNX Runtime, whisper)│ │
│                                                 │  ├─ score     (pure re-scoring engine)  │ │
│                                                 │  ├─ playback  (decode→GPU→SharedBuffer, │ │
│                                                 │  │             cpal audio clock)        │ │
│                                                 │  ├─ interchange (XML/FCPXML/AAF/OTIO/…) │ │
│                                                 │  └─ ingest    (watch, offload, proxies) │ │
│                                                 └─────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────────────────────┘
clippro-cli.exe links the same crates without Tauri or the UI.
```

### 6.2 Cargo workspace layout

```
ClipPro/
  Cargo.toml                 # workspace
  crates/
    clippro-types/           # shared data types, IDs, timecode/rational time, serde; no IO
    clippro-media/           # FFmpeg wrapper: probe, demux, decode (CPU + D3D11VA), audio decode, metadata
    clippro-gpu/             # D3D11 device mgmt, adapter enumeration, compute-shader dispatch, texture interop
    clippro-measure/         # per-sample measurement kernels: CPU (rayon + SIMD) and GPU (via clippro-gpu)
    clippro-audio/           # audio measurements (EBU R128, peaks, FFT features, VAD, sync xcorr)
    clippro-analysis/        # job scheduler, worker pool, clip splitting, checkpointing, temporal pass
    clippro-score/           # pure scoring/segmentation engine (SoA, incremental, rayon)
    clippro-project/         # .clippro SQLite schema, migrations, autosave, recovery, global cache
    clippro-interchange/     # exporters + importers: xmeml, fcpxml, otio, edl, ale, aaf, csv/json/html/pdf
    clippro-ingest/          # watch folders, offload + checksums + MHL, proxy matching/generation, render
    clippro-ai/              # ONNX Runtime sessions, model manager, faces, shot type, CLIP, aesthetic, takes
    clippro-transcribe/      # whisper.cpp binding, word timestamps, diarization, search index
    clippro-playback/        # preview decode pipeline, frame cache, SharedBuffer bridge, audio clock (cpal)
    clippro-cli/             # clippro-cli.exe
    clippro-testgen/         # synthetic test-footage generator with ground truth (dev tool)
  app/                       # Tauri 2 app (src-tauri) → ClipPro.exe
    src-tauri/
    ui/                      # React + TypeScript + Vite frontend
  shaders/                   # HLSL compute shaders → compiled at build time to DXBC (fxc, cs_5_0)
  presets/                   # built-in presets (.toml), embedded at compile time
  calibration/               # metric normalisation constants (calibration.toml), versioned
  testdata/                  # generated synthetic clips (gitignored) + ground-truth JSON (committed)
  docs/
```

### 6.3 Concurrency model
- **UI thread (WebView2)**: rendering only. All heavy work happens in Rust.
- **Tokio runtime**: Tauri commands, IPC, file watching, project I/O. Never blocks on CPU work.
- **Analysis scheduler** (one thread) that owns the job queue and hands work to:
  - **Decode workers**: OS threads, one FFmpeg decoder context each. Count = `workers` setting.
  - **Measurement**: the GPU path runs on a dedicated GPU submission thread per D3D11 device (immediate context, guarded); the CPU path runs on the rayon pool.
  - **Temporal/flag pass**: per clip, after all of its samples are measured (CPU, cheap).
- **Scoring engine**: runs on the rayon pool when settings change. Requests are debounced (16 ms) and cancellable: a newer request cancels an in-flight one at the next stage boundary.
- **Playback**: dedicated threads for decode-ahead, GPU convert, and the cpal audio callback.
- **AI**: separate low-priority queue with its own worker count (default 1). It never starves core analysis.

### 6.4 Portability seams
Windows-only code lives behind traits so a macOS port later only touches these crates:
- `clippro-gpu::ComputeDevice` (D3D11 now; Metal later)
- `clippro-media::HwDecoder` (D3D11VA now; VideoToolbox later)
- `clippro-playback::FrameSink` (WebView2 SharedBuffer now; another transport later)

## 7. Tech stack

Pin exact versions in `Cargo.lock` / `package-lock.json` at P0. Use the latest stable release at build time.

| Concern | Choice | Notes |
|---|---|---|
| Language (backend) | Rust stable, edition 2021, MSVC toolchain `x86_64-pc-windows-msvc` | |
| App shell | Tauri 2 | WebView2 runtime; NSIS bundler; plugins: dialog, fs, shell (open in Explorer), updater, window-state, single-instance |
| UI | React 18+, TypeScript (strict), Vite | |
| UI state | Zustand (UI state) + TanStack Query (backend reads) | |
| UI components | Radix UI primitives + Tailwind CSS | Custom minimal macOS-inspired theme (§14.4); Lucide icons; Inter + JetBrains Mono |
| Window materials | `window-vibrancy` crate | Mica (Win 11) / Acrylic (Win 10) on sidebar and inspector; frameless window with custom title bar |
| Canvas / graphics UI | Canvas2D for chart and timeline (WebGL2 where >50k marks), WebGL2 for viewer and overlays | |
| Rust↔TS types | `specta` + `tauri-specta` | Generates TS bindings for commands and events; CI fails on drift |
| FFmpeg | FFmpeg 7.x **LGPL shared** build (BtbN `win64-lgpl-shared`), bound via `rsmpeg` (or `ffmpeg-sys-next` + thin safe wrapper) | Dynamic linking for LGPL compliance; no GPL components |
| D3D11 / DXGI | `windows` crate (Direct3D11, Dxgi, D3dCompiler) | |
| Shaders | HLSL cs_5_0, compiled at build time with `fxc.exe` (Windows SDK) into embedded DXBC | |
| CPU parallelism | `rayon`; SIMD via `wide` / auto-vectorisation with `-C target-cpu=x86-64-v2` baseline; runtime AVX2 dispatch via `multiversion` | |
| Database | `rusqlite` (bundled SQLite, WAL mode) | |
| Compression | `zstd` | Measurement blobs |
| Hashing | `xxhash-rust` (xxh3/xxh64), `md-5`, `sha1` | |
| Audio DSP | `ebur128`, `rustfft`, `realfft` | |
| Audio output | `cpal` (WASAPI) | |
| File watching | `notify` | |
| Serialisation | `serde`, `serde_json`, `toml`, `quick-xml` | |
| AAF | `cfb` (Compound File Binary) + in-house AAF object model writer/reader | High risk; see §27 |
| ML runtime | `ort` (ONNX Runtime) with execution providers: DirectML (or Windows ML EP where available) → CPU | Optional install component |
| Speech-to-text | `whisper-rs` (whisper.cpp), CPU with optional Vulkan backend | |
| Logging | `tracing`, `tracing-subscriber`, `tracing-appender` (rolling files) | |
| Errors | `thiserror` (libs), `anyhow` (bins) | |
| Testing | `cargo test`, `proptest`, `insta` (snapshots), `criterion` (benches), Vitest + Testing Library (UI), WebdriverIO + `tauri-driver` (E2E) | |
| CI | GitHub Actions `windows-latest` | |

## 8. Hardware and platform strategy

### 8.1 Capability detection (at startup and on "Re-detect" in Settings)
1. Enumerate DXGI adapters. Skip the "Microsoft Basic Render Driver" (WARP) and adapters with `DXGI_ADAPTER_FLAG_SOFTWARE`.
2. On each hardware adapter, try `D3D11CreateDevice` with feature levels `[11_1, 11_0]` and `D3D11_CREATE_DEVICE_VIDEO_SUPPORT`.
3. Choose the adapter: the user's choice in Settings, or else the adapter with the most dedicated VRAM. If a discrete and an integrated GPU are both present, prefer the discrete one for compute. Optionally use the integrated GPU's decoder as well (Settings → Performance → "Use all video decoders").
4. For each candidate codec profile, query decoder support via `ID3D11VideoDevice::GetVideoDecoderProfileCount/GetVideoDecoderProfile` and `CheckVideoDecoderFormat` (H.264, HEVC Main/Main10/4:2:2 where exposed, VP9, AV1, MPEG-2, VC-1). Cache the support matrix.
5. If DirectML or Windows ML is available, list it as an AI execution provider.

### 8.2 Fallback matrix
| Situation | Decode | Measurement | Preview | AI |
|---|---|---|---|---|
| D3D11 GPU and the codec has a HW profile | D3D11VA (zero-copy) | D3D11 compute on decoder textures | D3D11 convert → SharedBuffer | DirectML/WinML → CPU |
| D3D11 GPU, codec not HW-decodable (ProRes, DNx, CineForm…) | FFmpeg CPU (frame threads) | Upload luma plane → D3D11 compute | CPU decode → GPU convert | DirectML/WinML → CPU |
| No usable GPU | FFmpeg CPU | CPU SIMD kernels (rayon) | CPU decode + CPU scale (swscale) → SharedBuffer | CPU |
| GPU device lost mid-run (TDR) | Recreate the device once; on a second failure, switch that run to CPU | same | same | same |

The status bar always shows the active path (e.g. "GPU: RTX 3060 · HW decode 74% of clips"). Clips decoded on the CPU show a small "CPU" chip in the clip list.

### 8.3 Measurement parity
The GPU and CPU kernels implement the same maths ([ALGORITHMS.md](ALGORITHMS.md) §1). Measurements are quantised (§12.3) before storage. Parity tests require GPU and CPU results to match within the quantisation step for ≥99.9% of values, with no difference in any pass/fail decision on the synthetic corpus. **Reproducible mode** forces CPU kernels, a single thread per clip, and fixed-order reductions, giving bit-identical output across machines.

## 9. Media layer (`clippro-media`)

### 9.1 Responsibilities
- `probe(path) -> MediaInfo`: container, streams, codec, profile, bit depth, chroma subsampling, colour primaries/transfer/matrix/range, resolution, SAR/DAR, frame rate (rational) with VFR detection, duration, start timecode (tmcd track / `timecode` tag / MXF material package), reel name, camera metadata, audio streams (channels, layout, sample rate).
- `open_video(path, opts) -> VideoDecoder`: seeks, decodes, and yields `DecodedFrame { pts, luma: LumaRef, chroma_thumb }`, where `LumaRef` is either a GPU texture plane (D3D11 NV12/P010 plane 0) or a CPU plane (u8 or u16).
- `open_audio(path, stream) -> AudioDecoder`: decodes to planar f32 at native rate, resampled to 48 kHz via swresample when needed.
- `fingerprint(path)`: `xxh3_128(size ‖ mtime ‖ first 1 MiB ‖ last 1 MiB ‖ 4 × 64 KiB at 20/40/60/80%)`. Used for caching and relinking.

### 9.2 Format support (1.0)
Containers: `.mp4 .mov .m4v .mkv .mxf .m2ts .mts .avi .webm .dv .mpg .ts .3gp`.
Video codecs: H.264 (8/10-bit, 4:2:0/4:2:2), HEVC (8/10-bit, 4:2:0/4:2:2), Apple ProRes (all flavours incl. 4444/XQ), Avid DNxHD/DNxHR, AVC-Intra, XAVC (Intra/Long GOP/S), XDCAM HD422 (MPEG-2), Panasonic AVC-LongG, GoPro CineForm, VP9, AV1, APV (when present in the bundled FFmpeg), Motion JPEG, DV/DVCPRO, MPEG-2.
RAW (proxy required in 1.0): `.braw .r3d .ari .arx .mxf (X-OCN/ARRIRAW) .crm .nev .dng` sequences, ProRes RAW. These are detected, flagged "Proxy required", and listed with an offer to link or generate a proxy (generation is impossible without a decoder, so only linking is offered).
Image sequences (DPX/EXR/TIFF/PNG/JPEG, detected by numbered pattern) are `LATER`.

### 9.3 Decode strategy
- **Hardware path**: create a `D3D11` device (shared with `clippro-gpu`), wrap it in `AVHWDeviceContext` (`AV_HWDEVICE_TYPE_D3D11VA`), enable multithread protection (`ID3D10Multithread::SetMultithreadProtected(TRUE)`), and request `BindFlags = D3D11_BIND_DECODER | D3D11_BIND_SHADER_RESOURCE` on the frames context. If the driver rejects SRV-bindable decoder arrays, fall back to `CopySubresourceRegion` into an SRV-capable NV12/P010 texture, which is still GPU-only.
- **Sampling with long-GOP codecs**: decode sequentially. For frames not selected for sampling (or as the pair-partner of a sample), set `skip_frame = AVDISCARD_NONREF`, then restore it for the frames needed. Seek only when the gap to the next sample is longer than 2 × GOP duration (the GOP is estimated from the keyframe index).
- **Intra-only codecs** (ProRes, DNx, AVC-Intra, MJPEG, CineForm, DV): seek directly to each sample; no sequential decode needed.
- **Clip splitting**: clips longer than `split_threshold` (default 10 min) are split at keyframes into ⌈duration / 5 min⌉ sections, analysed by separate workers and stitched (§ALGORITHMS 5.4 for seam handling).
- **Large clips**: at most `max_concurrent_large` (default 2) clips of ≥6K width decode at once.
- **Colour handling**: only luma is used for analysis. Limited-range Y is normalised to `[0,1]` as `(Y − 16·2^(b−8)) / (219·2^(b−8))`, clamped. Full range is `Y / (2^b − 1)`. The tiny chroma thumbnail (32×18, Cb/Cr averages) is kept for Solid-colour, bars and WB-cast detection only.
- **VFR**: samples are placed on presentation timestamps. The nearest decoded frame at or after the target time is used.
- **Rotation metadata**: honoured for display. Analysis geometry is computed on the stored orientation, and motion vectors are rotated for display.

## 10. Analysis pipeline (`clippro-analysis`)

The full maths is in [ALGORITHMS.md](ALGORITHMS.md). Pipeline per clip:

```
probe → plan samples (rate, pair-sampling) → [split into sections] →
  decode sample (+ pair frame) → downscale luma to ≤960 long edge (area filter) →
  active-area detect (once per clip + re-check on hard cuts) →
  per-sample kernels: focus tiles, histogram, noise, gradients, motion (pair + prev), chroma thumb →
  write SampleRaw rows (chunked, checkpointed) →
  [stitch sections] → temporal pass (stability, flicker, hunting, shake, cuts, duplicates) →
  flags → normalise to sub-metric scores → store MeasurementSet (quantised, zstd) →
  audio pass (parallel stream) → AI passes (optional, queued)
```

- **Checkpointing**: every 256 samples (or 10 s of wall time), a section's raw rows are flushed to the project DB in one transaction. On resume, decoding restarts from the last checkpoint's next sample (seeking to the preceding keyframe).
- **Queue order**: user order by default. "Move to next" bumps a clip. Opening an overlay on an unanalysed clip bumps it to the front, and its first 30 s are analysed first so the overlay appears quickly.
- **Changing the sample rate**: affects clips not yet started. With analysis paused, a change restarts clips that are mid-run. Finished clips keep their data, and "Re-analyse at new rate" is offered per clip or in bulk.
- **Changing compute profiles**: groups turned off are skipped for new work. Turning a group on later analyses only that group for finished clips, which needs another decode pass (the UI shows this).
- **Engine versioning**: each MeasurementSet records `engine_version`, `calibration_version`, `sample_rate`, `pair_sampling`, `groups`, `decode_path`. If a newer ClipPro changes measurement maths, old sets stay valid and readable, and the user is offered "Re-analyse with engine vX".

## 11. Scoring and segmentation (`clippro-score`)

A pure function `score(measurements, settings, overrides) -> Selects`. It is staged and incremental:

| Stage | Inputs | Output | Re-runs when |
|---|---|---|---|
| S1 sub-metric scores | normalised measures, sub-metric prefs | per-sample sub-scores | sub-metric prefs change |
| S2 group scores | S1, group enable | per-sample group scores | S1 changes |
| S3 overall | S2, group weights, centre weighting | per-sample overall | weights change |
| S4 modifiers | S3, flag tiers, camera filter, AI content | per-sample final score + force flags | tiers or camera filter change |
| S5 candidate runs | S4, low threshold, gap tolerance, hard cuts | runs | threshold low, gap tolerance |
| S6 segments | S5, length min/max, threshold high, handles | segments with mean scores | length, high threshold, handles |
| S7 per-clip cap + overrides + sort | S6, max per clip, manual overrides, sort | final selects | cap, overrides, sort |

Each stage caches its output per clip with a hash of its inputs. Changing the low threshold, for example, re-runs S5–S7 only. Exact formulas are in [ALGORITHMS.md](ALGORITHMS.md) §6–7.

**Invariants** (property-tested):
1. Lowering the low threshold never removes a frame from candidate runs.
2. Raising the high threshold never removes a segment that fits the length rules.
3. Force-include segments survive the score range, the per-clip cap and the camera filter, but are still clamped by segment length (max splits them, min drops them unless extended by handles).
4. Precedence is: manual override > flag Force include > flag Force exclude > score rules ([ALGORITHMS.md](ALGORITHMS.md) §6.4).
5. `score` is deterministic: the same inputs always produce identical output, independent of thread count.
6. Handles never overlap an adjacent segment and never cross the clip's start or end.

## 12. Data model and storage (`clippro-project`)

### 12.1 Project file
`.clippro` is a SQLite 3 database in WAL mode with `application_id = 0x434C5052` ("CLPR") and `user_version` = schema version. Migrations live in code (`migrations/NNN_*.sql`), are forward-only, and the file is backed up before migrating. Autosave commits a transaction every 30 s and after structural changes. On launch, a project with a stale `-wal` and the `clean_shutdown = 0` marker triggers the recovery dialog.

Sidecar cache: `%LOCALAPPDATA%\ClipPro\cache\` holds thumbnails (WebP), waveform peaks, filmstrips and the global measurement cache (`measure-cache.sqlite`, keyed by fingerprint + engine settings). The cache location is configurable and has a size cap (default 20 GB, LRU eviction).

### 12.2 Schema (core tables)
```sql
project(id, name, created_at, modified_at, app_version, settings_json)        -- 1 row
media(id, path, fingerprint, size, mtime, probe_json, status, proxy_media_id, is_proxy, reel, camera, start_tc, fps_num, fps_den, duration_us, width, height, added_at)
clip(id, media_id, bin_id, name, sort_index, rating, color_label, note)       -- a media item placed in the project
bin(id, parent_id, name, kind /*manual|smart*/, query_json)
analysis_run(id, media_id, engine_version, calibration_version, sample_rate, pair_sampling, groups_mask, decode_path, started_at, finished_at, state /*queued|running|paused|done|failed*/, progress, error)
sample_chunk(run_id, section, chunk_index, first_sample, count, data BLOB /*zstd, columnar*/)
measurement_set(run_id, data BLOB /*zstd, columnar, post-temporal, quantised*/, flags BLOB, summary_json)
audio_set(run_id, stream_index, data BLOB, summary_json)
ai_result(media_id, module, model_id, model_hash, data BLOB, created_at)
person(id, name, cover_face_ref, embedding BLOB)
face_track(id, media_id, person_id, data BLOB)
transcript(media_id, language, data BLOB /*words, segments, speakers*/)
transcript_fts USING fts5(media_id UNINDEXED, start_us UNINDEXED, text)
override(id, clip_id, kind /*force_in|force_out|manual_segment|trim*/, start_us, end_us, data_json)
preset(id, name, builtin, toml)
tuning_state(id, json, created_at)            -- undo history (ring buffer, 500) + named snapshots
timeline_import(id, source_path, format, data_json, created_at)
sync_group(id, name, method, data_json)
export_history(id, format, path, settings_json, created_at)
```

### 12.3 Measurement encoding
Columnar per run: `time_us: i64[]`, then one column per measure. Values are quantised to u16 over the measure's documented range (ALGORITHMS §1.0, about 1.5e-5 relative step). Booleans are bitsets. Motion vectors (optional, for the overlay) are stored as i8 pairs at 16×9 grid resolution. Columns are compressed with zstd level 3. Measured size (ADR-001): 2–3.3 MB per footage hour including tiles, motion vectors and histograms; budget ≤ 4 MB per hour (about 400 MB for 100 h). On load, columns are decoded into SoA `Vec<f32>` in memory (≈230 MB for 100 h, about 2.3 MB per footage hour).

## 13. Presets

Human-readable TOML. Unknown keys are preserved on round-trip. Example:

```toml
[preset]
name = "Talking Head"
version = 1
description = "Interviews and dialogue: steady, sharp, well exposed; ignores small subject movement."

[thresholds]
score = [0.62, 1.00]          # low, high
segment_length = ["3s", "none"]
max_per_clip = "none"

[weights]                       # relative; normalised internally; 0 = excluded
focus = 40
exposure = 30
motion = 20
audio = 10
content = 0

[focus]                         # bipolar prefs in [-1, 1]; 0/absent = neutral; false = off
sharpness = 0.8
detail = 0.2
focus_stability = 0.6

[exposure]
grading_flexibility = 0.6
flicker = -0.7                  # negative = reward LOW flicker
luma = { enabled = false, target = 0.45, tolerance = 0.15 }
contrast = { enabled = false, target = 0.55, tolerance = 0.2 }
noise = -0.3

[motion]
motion_stability = 0.7
camera_motion = -0.6            # reward low camera motion
subject_motion = 0.0

[audio]
loudness = { enabled = true, target_lufs = -23, tolerance_lu = 8 }
clean = 1.0                     # reward no clipping
speech_presence = 0.5

[flags]                         # force_exclude | disfavour | neutral | favour | force_include
clipping = "disfavour"
duplicate = "force_exclude"
solid_colour = "force_exclude"
hard_cut = "neutral"
motion_blur = "disfavour"
shaky = "disfavour"
low_light = "disfavour"

[camera_filter]
enabled = false

[advanced]
gap_tolerance = "0.75s"
centre_weighting = 0.3
stability_emphasis = 0.5
handles_samples = 3
snap_to_sentences = false
```

All presets must parse against a JSON Schema (generated from the Rust types via `schemars`) and pass validation in CI. The values for each built-in preset are defined in [ALGORITHMS.md](ALGORITHMS.md) §9.

## 14. UI specification

### 14.1 Layout (default workspace)
```
┌ Title bar: project name · status (analysis progress, GPU path, re-scoring…) · window controls ┐
├ Toolbar: Add media · Analyse ▶/⏸ · Sample rate · Preset ▾ · Selects-only ◻ · Sort ▾ · Export ┤
├──────────────┬──────────────────────────────────────────────┬─────────────────────────────────┤
│ Media/Bins   │ Viewer (16:9, overlays toolbar, timecode,    │ Tuning panel (tabs):            │
│ Clip list    │ JKL, inspector strip)                        │  Basics · Weighting · Flags ·   │
│ (table:      │                                              │  Camera · Audio · Content ·     │
│ thumb, name, ├──────────────────────────────────────────────┤  Advanced                       │
│ status, dur, │ Score chart (scatter)        ↕ resizable     │                                 │
│ selects, best│                                              │ Inspector (current sample)      │
│ score, flags)├──────────────────────────────────────────────┴─────────────────────────────────┤
│              │ Timeline: ruler · clip blocks · score bar · segments · flag lanes · waveform    │
└──────────────┴──────────────────────────────────────────────────────────────────────────────────┘
```
Panels can be resized and collapsed. Layout persists per user. Double-clicking a divider resets it. Optional extra panels: People, Transcript, Search, Sync groups, Imported timeline.

### 14.2 Key interactions
- **Sliders**: double-click a value to type it exactly. Accepted forms: `4`, `4.5`, `1:30`, `90s`, `1h`, `none`. Arrow keys nudge; Shift+arrow nudges ×10.
- **Weighting radar**: drag vertices. Ctrl+drag locks the others. A group's weight reaches 0% when dragged to the centre. The group label turns amber if the group has weight but every sub-metric is off.
- **Sub-metric slider**: bipolar with a Neutral detent at the centre. Labels at each end show what is rewarded ("Soft ← → Sharp").
- **Flag tiers**: a 5-position segmented control per flag, showing the run count across the project.
- **Camera wheel**: a puck sets direction and spread (centre ≈ 180° acceptance, edge = near-exact direction). The left arc sets zoom (up = in), the right arc sets rotation (up = CCW). Static-only and Mirror toggles. A live preview lists the 5 clips that match best.
- **Score chart**: drag the dashed threshold lines; hover for a tooltip; click to jump; scroll to zoom; summary shows "N selects · HH:MM:SS · X% of footage".
- **Timeline**: scroll to zoom and pan. Clicking a segment selects it. `F` toggles force-in or force-out, `[`/`]` trim, and `M` adds a manual segment between in and out points.
- **Undo/redo**: Ctrl+Z / Ctrl+Shift+Z for all tuning and overrides. History is listed in Edit → History.

### 14.3 Default shortcuts (rebindable)
| Key | Action | Key | Action |
|---|---|---|---|
| Space | Play/pause | J / K / L | Shuttle reverse / stop / forward |
| ← / → | Previous/next frame | Shift+← / → | Previous/next sample |
| ↑ / ↓ | Previous/next segment | Ctrl+↑ / ↓ | Previous/next clip |
| I / O | Mark in/out | M | Manual segment from in/out |
| F | Toggle force in/out on segment | 0–5 | Rate segment |
| S | Toggle selects-only | 1 / 2 / 3 / 4 (with Alt) | Overlay: focus / exposure / motion / off |
| Ctrl+E | Export | Ctrl+F | Search |
| Ctrl+S | Save | Ctrl+Z / Ctrl+Shift+Z | Undo / Redo |

### 14.4 Visual design: minimal, macOS-inspired
The owner's direction is a **minimal macOS feel**: quiet chrome, content first, few borders, generous spacing. The app should feel like a native Mac pro app (Final Cut Pro, Photos, Finder), running on Windows. It is *inspired by* macOS but uses no Apple assets: SF Pro, SF Symbols and Apple's artwork are licensed for Apple platforms only and must not be bundled.

**Window and chrome**
- A frameless Tauri window (`decorations: false`) with a **unified title bar and toolbar**, about 52 px tall. The project name is centred in medium weight. Toolbar items are icon-only buttons with tooltips, grouped with spacing rather than dividers.
- **Window controls**: macOS-style "traffic light" buttons (12 px circles, red/amber/green, glyphs on hover) on the **left** by default. Settings → General → "Window controls: macOS style | Windows style" switches to standard right-aligned Windows caption buttons. Windows 11 snap layouts must keep working either way (expose the maximise hit-test region via `WM_NCHITTEST` / Tauri's `startDragging` plus a native snap-layout flyout on hover of the maximise button when in Windows style).
- **Materials**: the sidebar and inspector use Windows 11 **Mica** (fallback: Acrylic on Windows 10, solid colour when transparency effects are off) via the `window-vibrancy` crate. This gives the frosted, translucent macOS sidebar look. The viewer area is always opaque neutral grey, so images are judged accurately.
- **Layout vocabulary**:
  - a left **source-list sidebar** (Media, Bins, Smart Collections, People) with disclosure triangles and small-caps section headers
  - a centre content area (viewer, chart, timeline)
  - a right **inspector** (tuning and frame details) that can be toggled with a toolbar button, as in Finder and Xcode
- **Dialogs** are **sheets** that slide down from under the title bar (export, settings for the current project). Small choices use **popovers** anchored to their control, with an arrow. The app-wide Settings window is a separate window with an icon toolbar of tabs (a macOS Preferences pattern).

**Look**
- **Colour**:
  - Dark mode by default; light mode follows the Windows app theme unless overridden.
  - Dark surfaces: window `#1E1E1E`, sidebar material tint `#2A2A2A @ 70%`, controls `#3A3A3C`, separators `rgba(255,255,255,0.08)`.
  - Light surfaces: window `#F5F5F7`, controls `#FFFFFF`, separators `rgba(0,0,0,0.08)`.
  - A single **accent colour** (default blue `#0A84FF` dark / `#007AFF` light; user-selectable from 8 accents) for selection, focus rings, primary buttons and active toggles.
  - Everything else is greyscale. Score colours are the only other chromatic element.
- **Type**: **Inter** (bundled; the closest freely licensed match to SF Pro, using the `cv11`, `ss01` features and tight tracking at display sizes) for the UI, and **JetBrains Mono** for timecode and numeric readouts with tabular numbers. Sizes 11/12/13 (body)/15/20. Weights 400/500/600 only. Sentence-case labels, no all-caps except sidebar section headers (11 px, 600, +0.06em tracking).
- **Shape**: corner radius 6 px for controls, 10 px for panels and popovers, 12 px for sheets and windows. 1 px hairline separators instead of boxes. Shadows only on floating layers (popover, sheet, menu): `0 8px 30px rgba(0,0,0,0.35)`.
- **Controls**: macOS-like:
  - pill **segmented controls** (used for flag tiers, sort and overlay modes)
  - **switch** toggles
  - thin sliders (track 4 px, white 18 px knob with a subtle shadow, accent-filled range)
  - **stepper** fields for typed values
  - popup buttons with chevrons
  - search fields with rounded ends
  - Hover states are subtle (≈ +4% lightness).
- **Icons**: **Lucide** (ISC licence) at a 1.5 px stroke, 16/18 px, chosen to resemble SF Symbols' line weight. No filled or multicolour icons.
- **Density**: 8 px spacing grid; 28 px default control height (24 px compact mode); sidebar rows 28 px.
- **Motion**: short, physical, never decorative. Sheets and popovers use 180–240 ms spring easing (`cubic-bezier(0.32, 0.72, 0, 1)`), panel collapse 200 ms, hover 80 ms. Everything honours reduced-motion (instant).
- **Score colour scale**: muted perceptual red→amber→green (OKLCH, chroma capped at 0.14 so it sits calmly in the minimal UI), with a colour-blind-safe alternative (blue→orange) in Settings.
- **Empty states**: one line of text, one glyph and one primary action (e.g. "Drop footage here or Add Media…"), centred.

**Accessibility**: everything reachable by keyboard, visible accent focus rings (3 px, 40% alpha), ARIA labels on custom controls, 4.5:1 text contrast in both themes, honours reduced-motion and Windows High Contrast (falls back to system colours).

## 15. Preview and playback (`clippro-playback`)

**Goal**: smooth 1080p-equivalent preview at the source frame rate up to 60 fps, frame-accurate seeking, overlays at 60 fps, with low CPU.

**Transport**: frames are produced in Rust and delivered to the WebView with **WebView2 SharedBuffer** (`ICoreWebView2Environment12::CreateSharedBuffer` + `ICoreWebView2_17::PostSharedBufferToScript`; JS receives them on `chrome.webview` `sharedbufferreceived`). A ring of 4 buffers at the viewer's pixel size (capped at 2560×1440) holds RGBA8 or NV12 frames. JS uploads them to a WebGL2 texture (NV12 → RGB conversion in the shader when NV12 is used). If SharedBuffer is unavailable (old runtime), fall back to a Tauri custom protocol (`clippro://frame/<id>`) that returns JPEG at quality 90.

**Pipeline**: decoder (HW or CPU) → GPU scale and convert (D3D11 compute or video processor) → staging → shared buffer → post. Decode-ahead keeps 8 frames. Reverse play and JKL above 2× decode keyframes only.

**Clock**: audio is the master clock via `cpal` (WASAPI shared mode). Video frames are presented by PTS against the audio clock. When muted or there's no audio, a high-resolution timer is used.

**Overlays**: focus heatmap, zebras and false colour are computed in WebGL shaders from the displayed frame, so they work on any frame, analysed or not. Measurement-derived overlays (tile sharpness grid, motion vectors, global-motion gizmo, histogram from analysis) come from stored measurements of the nearest sample. Log LUTs are applied in the shader as 3D textures (33³ or 65³).

## 16. Interchange (`clippro-interchange`)

### 16.1 Common model
An internal `Timeline` type: `Sequence { name, rate, tracks[Video|Audio] { items[Clip{media_ref, src_in, src_out, rec_in, name, markers[], metadata}] | Gap } , markers[] }`, with rational time (`num/den` of the edit rate) and source timecode. All exporters and importers translate to and from this model.

### 16.2 Exporters
| Format | Spec target | Markers | Colour | Notes |
|---|---|---|---|---|
| FCP7 XML (`xmeml` v5) | Premiere Pro, Resolve | clip markers with name and comment | Premiere marker colour via `<color>` extension where honoured | `pathurl` as `file://localhost/D:/...` URL-encoded; NTSC rates use `ntsc=TRUE` + timebase |
| FCPXML 1.10 (selectable 1.9–latest supported) | FCP 10.6+, Resolve | `<marker>` / `<chapter-marker>` | via keyword/role mapping | assets + formats deduplicated; `tcStart`; rational time strings |
| OTIO (`.otio` JSON, schema `Timeline.1`) | Resolve 18.5+, OTIO tools | `Marker.2` with colour enum | native | `ExternalReference` with `target_url` |
| CMX3600 EDL | Any NLE | `* LOC:` lines (Resolve/Avid locators) | LOC colour names | reel names ≤ 8 chars (configurable 32-char extended) |
| Avid ALE + marker `.txt` | Avid Media Composer | marker list import | Avid colour names | the 1.0 Avid path until AAF passes validation |
| AAF | Avid, Premiere | `DescriptiveMarker` / `CommentMarker` | Avid colour attribute | links to external media via `NetworkLocator`; see §27 risk |
| CSV / JSON | spreadsheets, pipelines | n/a | n/a | one row per select; JSON includes sub-metrics |
| HTML report | humans | n/a | n/a | single self-contained file, thumbnails as base64 WebP, sortable tables |
| PDF contact sheet | clients | n/a | n/a | grid of best-thumbnail frames per select with TC and score |

**Export options**: track layout (selects only | selects + rejects on V2), handles (0–10 s), sequence vs subclips, gaps between selects (0–5 s), audio channels (all | first N | per stream), marker templates (§5.8 EXP-6), file path mode (original | proxy | relative), timeline frame rate (source | custom). Selects-only and sort order carry into the export.

**Validation**: every exporter has snapshot tests (`insta`), schema/DTD validation (FCPXML DTD, xmeml structure checks, OTIO via the `opentimelineio` Python package in CI, AAF via `pyaaf2` in CI), and round-trip tests (`import(export(t)) ≅ t`).

### 16.3 Importers (P11)
FCP7 XML, FCPXML, OTIO, EDL (with reel→media matching), AAF (read via `cfb`). Media relink priority: exact path → same filename under the project's media roots → fingerprint → reel + timecode range. Unresolved items are listed with a "Locate…" action.

## 17. Audio (`clippro-audio`)
- Decodes every audio stream (or the user-selected ones) at 48 kHz f32. The work runs alongside video analysis in the same worker when the stream is in the same file.
- Per 100 ms block: peak, RMS, momentary loudness (400 ms), short-term (3 s), true peak (4× oversampled), spectral features (low-band energy ratio < 150 Hz, spectral flatness, spectral flux, 50/60 Hz harmonic comb energy), zero-crossing rate.
- Per sample: block features are aggregated over the sample's window (±0.5 / rate).
- Per segment: integrated loudness (EBU R128 gating) is computed at score time from blocks.
- Flags: Audio clipping, Silence, Wind, Hum, Dead/missing channel, Channel imbalance.
- Waveform peaks pyramid (min/max per 256/2048/16384 samples) for the timeline lane.
- Sync: FFT cross-correlation of onset envelopes (ALGORITHMS §8.3).

## 18. AI modules (`clippro-ai`, `clippro-transcribe`)

**Policy**: off by default. The first time a module is enabled, ClipPro shows the model, its licence, its size and its SHA-256, and downloads only after consent. Models are cached in `%LOCALAPPDATA%\ClipPro\models\`. Each AI result is stored with `model_id` + `model_hash`. AI never changes Focus/Exposure/Motion/Audio scores. It contributes only through the optional **Content** group, AI-specific filters and flags (e.g. "No face", "Eyes closed"), search, grouping and edge snapping.

| Module | Model (ONNX unless noted; licence-check each before bundling) | Input | Output |
|---|---|---|---|
| Faces: detection | SCRFD-2.5G or YuNet | sampled frames at 640 px, colour (decoded separately at 2 Hz) | boxes, 5-point landmarks, confidence |
| Faces: embedding | ArcFace-R50 class model (or a permissively licensed equivalent) | aligned 112×112 crops | 512-d embeddings → agglomerative clustering (cosine, threshold 0.45) → People |
| Faces: quality | deterministic on crops: Laplacian sharpness inside the box, eye-aspect ratio from landmarks, face area % | | face_focus, eyes_open, face_size |
| Transcripts | whisper.cpp ggml models (tiny/base/small/medium/large-v3-turbo; default small) | 16 kHz mono mix or chosen channel | words (start, end, prob), segments, language |
| Speaker turns | ECAPA-TDNN speaker embedding ONNX + clustering | speech regions | speaker labels |
| Shot type & scene | OpenCLIP ViT-B/32 image encoder, zero-shot prompts + face-size heuristic | 224 px centre crop + full-frame pad | shot type probabilities, indoor/outdoor, day/night |
| Semantic search | same CLIP image embeddings + CLIP text encoder | query text | ranked samples/segments |
| Aesthetic | linear/MLP head on CLIP embeddings (LAION-aesthetic-style) | CLIP embedding | 0–10 → normalised |
| Take grouping | pHash + CLIP cosine + transcript similarity (ALGORITHMS §8.4) | segments | groups, best-take rank |

Execution providers: DirectML (or Windows ML on supported Windows 11 builds) on the selected GPU, falling back to CPU (`intra_op_num_threads` = workers). Whisper uses CPU by default with an optional Vulkan backend. With "Deterministic AI" checked in Settings, AI runs on the CPU only.

## 19. Ingest, offload, proxies and render (`clippro-ingest`)
- **Watch folders**: `notify` (ReadDirectoryChangesW). A new file is "stable" when its size and mtime are unchanged for 5 s and it can be opened read-shared. Growing MXF/MOV files are analysed only once stable. Optional "include subfolders" and extension filters.
- **Offload**: source card/folder → N destinations. Copies run in parallel streams with an 8 MiB buffer, then are verified by re-reading the destination (xxh64 default; MD5/SHA-1 optional). Produces an ASC MHL v2 `ascmhl/` history folder. Analysis can start from the first verified destination. Resumable, with a full log.
- **Proxy match**: by base name (ignoring suffixes `_proxy`, `_Proxy`, `.proxy`), then duration ±1 frame, then start TC.
- **Proxy generation**: FFmpeg LGPL encoders: `prores_ks` (Proxy/LT), `dnxhd` (DNxHR LB), or hardware H.264/HEVC (`h264_nvenc`/`hevc_nvenc`, `h264_amf`, `h264_qsv`, `h264_mf`). Keeps timecode, reel and audio. Saves next to the source or in a chosen folder.
- **Render selects**: per select, output either stream copy (when in/out land on keyframes, or with the user's "allow nearest keyframe" option) or re-encode with the chosen codec. Filenames come from a template (`{clip}_{index}_{tc_in}`). Embeds source timecode. Optional burn-ins are `LATER`.

## 20. CLI (`clippro-cli.exe`)
```
clippro-cli analyze <paths...> [--project out.clippro] [--rate standard|1|2|4|8|16|all]
                    [--groups focus,exposure,motion,audio] [--workers N] [--cpu-only] [--reproducible]
clippro-cli score   <project.clippro> [--preset NAME|file.toml] [--set key=value ...]
clippro-cli export  <project.clippro> --format xmeml|fcpxml|otio|edl|ale|aaf|csv|json|html|pdf
                    --out PATH [--selects-only] [--handles 1s] [--markers TEMPLATE]
clippro-cli offload <src> --dest <d1> [--dest <d2>] [--hash xxh64|md5|sha1]
clippro-cli info    <media>                # probe + capability report as JSON
clippro-cli selftest                        # runs synthetic corpus, prints pass/fail
```
Exit codes: 0 success, 1 usage error, 2 media error, 3 partial (some clips failed), 4 internal. Output defaults to human-readable text; `--json` makes it machine-readable.

## 21. Settings
- **General**: autosave interval, default preset, language (English only in 1.0), theme, score colour scale, check for updates.
- **Performance**: GPU adapter, hardware decode on/off, use all decoders, CPU workers (Auto/1..N), max concurrent large clips, clip split threshold, AI workers, reproducible mode, analyse while playing (throttle).
- **Analysis defaults**: sample rate, pair sampling, compute groups, auto-start analysis.
- **Compatibility**: force CPU decode for specific codecs, VFR handling, timecode source priority, limited/full range override per clip.
- **Cache**: location, size cap, clear.
- **AI Models**: module toggles, model manager, execution provider, deterministic AI.
- **Keyboard**: rebinding with conflict detection, export/import of keymaps.
- **Export defaults**: per format.

## 22. Performance budgets (acceptance targets)

Reference machines. **GPU-ref**: Ryzen 5 5600 / RTX 3060 12 GB / NVMe. **CPU-ref**: Intel i5-1135G7 with integrated graphics disabled (CPU path), 16 GB RAM.

| Metric | GPU-ref | CPU-ref |
|---|---|---|
| Analysis, 4K H.264 8-bit 30p, Standard (4 Hz), all groups | ≥ 20× realtime | ≥ 5× realtime |
| Analysis, 4K ProRes 422 HQ 25p, Standard | ≥ 12× realtime | ≥ 4× realtime |
| Analysis, 1080p H.264, Draft (1 Hz) | ≥ 80× realtime | ≥ 25× realtime |
| Re-score after a slider change, 100 h at 4 Hz | ≤ 150 ms | ≤ 400 ms |
| Re-score, 10 h | ≤ 20 ms | ≤ 50 ms |
| UI frame time while dragging sliders | ≤ 16 ms (60 fps) | ≤ 33 ms |
| Seek to an arbitrary frame (long-GOP 4K H.264) | ≤ 150 ms | ≤ 400 ms |
| Cold app start to interactive | ≤ 2 s | ≤ 3 s |
| Open a 100 h analysed project | ≤ 3 s | ≤ 5 s |
| Memory, 100 h project open, idle | ≤ 1.5 GB | ≤ 1.5 GB |

Benchmarks live in `crates/*/benches` and `clippro-cli bench`. CI tracks regressions (> 10% slower fails the job on the self-hosted runner, if one exists; otherwise the result is reported only).

## 23. Reliability, logging and diagnostics
- `tracing` logs go to `%LOCALAPPDATA%\ClipPro\logs\clippro-YYYY-MM-DD.log` (7 days kept). UI errors are forwarded to the backend log.
- Every media failure is captured per clip (status "Failed: <reason>", with Retry and "Retry with CPU decode"). One bad file never stops the queue.
- Panics are caught at worker boundaries, so a worker panic marks only its clip as failed. A global panic hook writes a minidump-style report (backtrace and state, no media content).
- Help → "Create diagnostics bundle" zips logs, system info, capability matrix and project settings (no media, no thumbnails unless the user opts in).
- Single instance: opening a `.clippro` file passes it to the running instance.

## 24. Packaging, distribution and compliance
- **Bundling**: Tauri NSIS installer (per-user install by default, per-machine optional), with the WebView2 Evergreen bootstrapper embedded. A portable ZIP build is also produced. File association for `.clippro`. Start-menu shortcut. The `clippro-cli.exe` folder is optionally added to PATH.
- **Payload**: `ClipPro.exe`, `clippro-cli.exe`, FFmpeg LGPL DLLs (`avcodec`, `avformat`, `avutil`, `swscale`, `swresample`, `avfilter` if used), `onnxruntime.dll` + `DirectML.dll` (AI component, optional installer section), fonts, presets, calibration, LUTs, licences folder.
- **Signing**: Authenticode-sign the exe, DLLs we build and the installer (certificate supplied by the owner; the signing step is skipped with a warning when unavailable). Without signing, SmartScreen will warn, which is documented.
- **Updates**: `tauri-plugin-updater` with a signed `latest.json` hosted on a GitHub Releases repo (pattern already used by the owner for ReelCutter). Checks are user-controlled; updates install only after approval.
- **Licence compliance**: an LGPL FFmpeg build is dynamically linked, its source offer and build config are shipped in `licenses/`, and the user can replace the DLLs. A third-party notices file is generated by `cargo-about` and `license-checker` (npm). Each AI model's licence is shown before download, and models with non-commercial licences are excluded or marked.

## 25. Security and privacy
- No telemetry. Network access happens only for (a) the update check (opt-in, can be disabled), and (b) model downloads (explicit). Both use HTTPS with pinned hosts and SHA-256 verification.
- Tauri capabilities are least-privilege: the frontend can't touch the filesystem directly. All paths go through backend commands, which validate them.
- CSP: `default-src 'self'; img-src 'self' data: blob: clippro:; style-src 'self' 'unsafe-inline'`. No remote content is ever loaded into the WebView.
- Importers treat XML/AAF/EDL as untrusted: no external entity resolution (quick-xml has no DTD fetching), size limits, and fuzz targets (`cargo-fuzz`) for every parser.
- Source media is opened read-only, always.

## 26. Testing and verification strategy
1. **Synthetic corpus** (`clippro-testgen`): generates clips with known ground truth (blur ramps, exposure ramps, clipping, solid frames, bars, hard cuts, pans/tilts/zooms/rotations at known speeds, shake, flicker at 50/60/100/120 Hz, holds/duplicates, low light with noise, letterbox, VFR, and audio with clipping, silence, wind-like noise, 50/60 Hz hum and speech). It uses the bundled FFmpeg (lavfi) plus Rust image synthesis. Ground truth is written to `testdata/truth/*.json`. Details in PLAN P0.4.
2. **Metric tests**: monotonicity (sharper → higher Sharpness), detection precision/recall per flag ≥ 0.95 on the corpus, motion direction error ≤ 10°, speed error ≤ 15%.
3. **GPU/CPU parity tests** (§8.3); skipped with a notice on CI runners without a GPU, and run locally plus on WARP for shader correctness (WARP is acceptable for correctness, not speed).
4. **Scoring property tests** (§11 invariants) with `proptest`.
5. **Interchange tests** (§16.2).
6. **Project tests**: migrations, crash recovery (kill during write), relink.
7. **UI**: Vitest unit tests for parsing (typed slider values), stores and components; E2E with WebdriverIO + `tauri-driver` for W1 end to end on the synthetic corpus.
8. **Performance benches** (§22).
9. **Fuzzing**: importers, typed-value parser, preset parser.
10. **Human acceptance checkpoints** (cannot be automated; listed in PLAN): exports opened in Premiere, Resolve, Avid and FCP; real-footage spot checks; subjective calibration review.

## 27. Risks and mitigations
| Risk | Impact | Mitigation |
|---|---|---|
| AAF writing is complex and poorly documented | Avid handoff | Ship ALE + marker list + EDL for Avid in 1.0 (P7). Build AAF in P7b against the AAF Object Spec v1.1, validated with pyaaf2 and a human import test. Mark AAF "beta" if Avid imports aren't confirmed. |
| SRV-bindable D3D11VA decoder textures unsupported on some drivers | zero-copy path | Fall back to a GPU copy into SRV texture (still fast); then CPU readback |
| WebView2 SharedBuffer performance or availability | preview smoothness | Benchmark in P6 spike; fallback custom protocol JPEG; worst case native child HWND swapchain (documented alternative) |
| Metric normalisation constants mis-calibrated on real footage | quality of selects | Calibration tooling (P2.9), real-footage review checkpoint, `calibration.toml` versioned separately from code |
| Long-GOP sampling cost on high-bitrate 4K/8K HEVC | speed | NONREF skipping, hardware decode, section splitting, Draft rate guidance |
| FFmpeg LGPL constraints (no x264/x265) | proxy encoding | Hardware encoders + ProRes/DNx (LGPL) + Media Foundation H.264 |
| AI model licences | redistribution | Download-on-demand with licence display; only permissive models listed by default |
| DirectML maintenance status | AI GPU acceleration | `ort` EP abstraction; Windows ML EP when available; CPU always works |
| Build drifting from spec | correctness | Every feature carries explicit acceptance tests, run before it is marked done |

## 28. Out of scope for 1.0 (backlog)
Native BRAW/R3D/ARRIRAW decode via vendor SDKs; image sequences; macOS build; smart reframe (vertical crops from face/subject tracking exported as transforms); burn-in renders; collaborative review links; localisation; scripting/plugin API; Premiere/Resolve panel extensions for one-click send; map view for GPS metadata.

## 29. Glossary
- **Sample**: one analysed point in time (a frame, optionally with its pair frame).
- **Measure**: a raw number from a kernel (e.g. Laplacian variance).
- **Sub-metric**: a normalised 0–1 quantity users can weight (e.g. Sharpness).
- **Group**: Focus, Exposure, Motion, Audio, Content.
- **Overall / frame score**: the weighted blend of groups after modifiers, 0–1, shown as 0–100.
- **Segment / select**: a contiguous qualifying range of a clip, with handles.
- **Flag**: a boolean condition per sample (e.g. Shaky); contiguous flagged samples form a **run**.
- **Tier**: what a flag does to scoring (force exclude … force include).
- **Engine version**: version of the measurement maths; part of the reproducibility key.
