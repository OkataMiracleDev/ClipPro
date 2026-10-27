//! `clippro-cli` (SPEC §20): headless analysis, scoring and media inspection.
//! Exit codes: 0 success, 1 usage error, 2 media error, 3 partial (some clips failed), 4 internal.

use clap::{Parser, Subcommand};
use clippro_analysis::jobs::{Event, SchedulerConfig};
use clippro_analysis::AnalyzeOptions;
use clippro_core::{Session, SessionOptions};
use clippro_media::HwMode;
use clippro_project::RunState;
use clippro_score::{builtin, Preset, Settings, SortOrder};
use clippro_types::typed::{parse_count, parse_duration, parse_score};
use clippro_types::{FlagKind, GroupMask, MetricGroup, SampleRate, Tier};
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "clippro-cli", version, about = "ClipPro headless analysis and scoring")]
struct Cli {
    /// Machine-readable JSON output.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Probe a media file and print its streams and metadata.
    Info { media: PathBuf },
    /// Import media into a project (created if missing) and analyse it.
    Analyze {
        /// Files and/or folders.
        paths: Vec<PathBuf>,
        #[arg(long)]
        project: PathBuf,
        /// draft | fast | standard | detailed | thorough | all (or 1, 2, 4, 8, 16).
        #[arg(long, default_value = "standard")]
        rate: String,
        /// Comma-separated compute groups: focus,exposure,motion,audio.
        #[arg(long, default_value = "focus,exposure,motion,audio")]
        groups: String,
        #[arg(long)]
        workers: Option<usize>,
        /// Decode on the CPU only.
        #[arg(long)]
        cpu_only: bool,
        /// Disable pair sampling.
        #[arg(long)]
        no_pair: bool,
        /// Don't read or write the global measurement cache.
        #[arg(long)]
        no_cache: bool,
    },
    /// Score an analysed project and print the selects.
    Score {
        project: PathBuf,
        /// Built-in preset name or a .toml preset file.
        #[arg(long, default_value = "All-Rounder")]
        preset: String,
        /// Override settings: score=55,100  length=2s,none  cap=3  gap=0.5s  handles=3
        /// weight.focus=40  sub.sharpness=0.8  flag.shaky=force_exclude
        #[arg(long = "set")]
        sets: Vec<String>,
        /// timeline | score | score-asc | duration | duration-asc
        #[arg(long, default_value = "timeline")]
        sort: String,
    },
    /// Export the selects for an NLE or as a report.
    Export {
        project: PathBuf,
        /// xmeml (Premiere/Resolve) | fcpxml (FCP/Resolve) | otio | edl | ale | avid-markers | csv | json | html | contact-sheet
        #[arg(long)]
        format: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "All-Rounder")]
        preset: String,
        #[arg(long = "set")]
        sets: Vec<String>,
        #[arg(long, default_value = "timeline")]
        sort: String,
        /// selects | with-rejects (whole clips, rejects on V2)
        #[arg(long, default_value = "selects")]
        layout: String,
        /// Export only the core of each select, without handles.
        #[arg(long)]
        no_handles: bool,
        /// Gap between selects, e.g. 1s.
        #[arg(long, default_value = "0")]
        gap: String,
        #[arg(long)]
        no_markers: bool,
        #[arg(long)]
        no_audio: bool,
        /// Sequence name.
        #[arg(long, default_value = "ClipPro Selects")]
        name: String,
    },
    /// List built-in presets.
    Presets,
}

#[derive(Debug)]
struct Failure {
    code: u8,
    msg: String,
}

fn fail(code: u8, msg: impl Into<String>) -> Failure {
    Failure { code, msg: msg.into() }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(if e.use_stderr() { 1 } else { 0 });
        }
    };
    match run(&cli) {
        Ok(code) => ExitCode::from(code),
        Err(f) => {
            if cli.json {
                println!("{}", json!({ "error": f.msg, "code": f.code }));
            } else {
                eprintln!("error: {}", f.msg);
            }
            ExitCode::from(f.code)
        }
    }
}

fn run(cli: &Cli) -> Result<u8, Failure> {
    match &cli.cmd {
        Cmd::Info { media } => info(media, cli.json),
        Cmd::Analyze { paths, project, rate, groups, workers, cpu_only, no_pair, no_cache } => {
            analyze(paths, project, rate, groups, *workers, *cpu_only, *no_pair, *no_cache, cli.json)
        }
        Cmd::Score { project, preset, sets, sort } => score(project, preset, sets, sort, cli.json),
        Cmd::Export { project, format, out, preset, sets, sort, layout, no_handles, gap, no_markers, no_audio, name } => {
            export(project, format, out, preset, sets, sort, layout, *no_handles, gap, *no_markers, *no_audio, name, cli.json)
        }
        Cmd::Presets => {
            let names: Vec<String> = clippro_score::builtins().into_iter().map(|p| p.preset.name).collect();
            if cli.json {
                println!("{}", json!(names));
            } else {
                for n in names {
                    println!("{n}");
                }
            }
            Ok(0)
        }
    }
}

fn info(media: &Path, json_out: bool) -> Result<u8, Failure> {
    let info = clippro_media::probe(media).map_err(|e| fail(2, e.to_string()))?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&info).map_err(|e| fail(4, e.to_string()))?);
        return Ok(0);
    }
    println!("{}", info.path);
    println!("  container  {}  ({} bytes)", info.container, info.size);
    println!("  duration   {:.3} s", info.duration_us as f64 / 1e6);
    if let Some(v) = &info.video {
        println!(
            "  video      {} {} {}x{} {:.3} fps {}-bit {} {}{}",
            v.codec,
            v.profile,
            v.width,
            v.height,
            v.fps.fps(),
            v.bit_depth,
            v.chroma,
            v.color_transfer,
            if v.vfr { " VFR" } else { "" }
        );
    }
    for a in &info.audio {
        println!("  audio #{}   {} {} ch {} Hz {}", a.index, a.codec, a.channels, a.sample_rate, a.layout);
    }
    for (k, v) in [("timecode", &info.start_timecode), ("reel", &info.reel), ("camera", &info.camera_model), ("created", &info.creation_time)] {
        if let Some(v) = v {
            println!("  {k:<10} {v}");
        }
    }
    if info.proxy_required {
        println!("  (camera RAW: link a proxy to analyse)");
    }
    Ok(0)
}

fn parse_groups(s: &str) -> Result<GroupMask, Failure> {
    let mut m = GroupMask(0);
    for g in s.split(',').map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()) {
        let grp = match g.as_str() {
            "focus" => MetricGroup::Focus,
            "exposure" => MetricGroup::Exposure,
            "motion" => MetricGroup::Motion,
            "audio" => MetricGroup::Audio,
            other => return Err(fail(1, format!("unknown group '{other}'"))),
        };
        m = m.with(grp, true);
    }
    if m.is_empty() {
        return Err(fail(1, "at least one compute group is required"));
    }
    Ok(m)
}

fn session_opts(rate: SampleRate, groups: GroupMask, workers: Option<usize>, cpu_only: bool, pair: bool, cache: bool) -> SessionOptions {
    let mut o = SessionOptions::default();
    o.analyze = AnalyzeOptions { rate, pair_sampling: pair, groups, ..AnalyzeOptions::default() };
    if cpu_only {
        o.analyze.decode.hw = HwMode::Off;
    }
    if let Some(w) = workers {
        o.scheduler = SchedulerConfig { workers: w.max(1), ..o.scheduler };
    }
    if !cache {
        o.cache_path = None;
    }
    o
}

#[allow(clippy::too_many_arguments)]
fn analyze(
    paths: &[PathBuf],
    project: &Path,
    rate: &str,
    groups: &str,
    workers: Option<usize>,
    cpu_only: bool,
    no_pair: bool,
    no_cache: bool,
    json_out: bool,
) -> Result<u8, Failure> {
    let rate = SampleRate::parse(rate).ok_or_else(|| fail(1, format!("unknown sample rate '{rate}'")))?;
    let groups = parse_groups(groups)?;
    let opts = session_opts(rate, groups, workers, cpu_only, !no_pair, !no_cache);
    let s = if project.exists() {
        let (s, rep) = Session::open(project, opts).map_err(|e| fail(4, e.to_string()))?;
        if rep.recovered && !json_out {
            eprintln!("note: the project wasn't closed cleanly last time; continuing from the last saved state");
        }
        s
    } else {
        let name = project.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Project".into());
        Session::create(project, &name, opts).map_err(|e| fail(4, e.to_string()))?
    };
    let rep = s.import(paths).map_err(|e| fail(4, e.to_string()))?;
    if !json_out {
        eprintln!(
            "imported {} clip(s){}{}{}",
            rep.added.len(),
            if rep.duplicates.is_empty() { String::new() } else { format!(", {} duplicate(s) skipped", rep.duplicates.len()) },
            if rep.proxies_linked.is_empty() { String::new() } else { format!(", {} proxy link(s)", rep.proxies_linked.len()) },
            if rep.cache_hits == 0 { String::new() } else { format!(", {} from cache", rep.cache_hits) }
        );
        for (p, e) in &rep.failed {
            eprintln!("  couldn't import {}: {e}", p.display());
        }
    }
    let queued = s.queue_analysis().map_err(|e| fail(4, e.to_string()))?;
    let started = std::time::Instant::now();
    // Overall progress = (finished clips + fraction of running clips) / queued clips.
    let mut fraction: std::collections::HashMap<u64, f64> = Default::default();
    s.wait(|e| {
        match e {
            Event::Progress { job, done, total } => {
                fraction.insert(*job, *done as f64 / (*total).max(1) as f64);
            }
            Event::Finished { job } | Event::Failed { job, .. } => {
                fraction.insert(*job, 1.0);
            }
            _ => {}
        }
        if !json_out && queued > 0 {
            let finished = fraction.values().filter(|f| **f >= 1.0).count();
            let pct = 100.0 * fraction.values().sum::<f64>() / queued as f64;
            eprint!("\ranalysing: {finished}/{queued} clips done ({pct:>5.1}%)  ");
            let _ = std::io::stderr().flush();
        }
    });
    if !json_out && queued > 0 {
        eprintln!("\ranalysed {queued} clip(s) in {:.1}s          ", started.elapsed().as_secs_f64());
    }
    let errors = s.take_storage_errors();
    let statuses = s.clip_statuses().map_err(|e| fail(4, e.to_string()))?;
    let failed = statuses.iter().filter(|c| c.state == RunState::Failed).count();
    if json_out {
        let clips: Vec<_> = statuses
            .iter()
            .map(|c| json!({ "clip": c.clip.0, "name": c.name, "path": c.path, "state": format!("{:?}", c.state).to_lowercase(), "error": c.error }))
            .collect();
        println!(
            "{}",
            json!({ "project": project, "imported": rep.added.len(), "duplicates": rep.duplicates.len(), "cache_hits": rep.cache_hits,
                    "import_failures": rep.failed.iter().map(|(p, e)| json!({"path": p, "error": e})).collect::<Vec<_>>(),
                    "clips": clips, "storage_errors": errors })
        );
    } else {
        for c in statuses.iter().filter(|c| c.state == RunState::Failed) {
            eprintln!("  failed: {} ({})", c.name, c.error.as_deref().unwrap_or("unknown error"));
        }
        for e in &errors {
            eprintln!("  storage error: {e}");
        }
    }
    s.close().map_err(|e| fail(4, e.to_string()))?;
    Ok(if failed > 0 || !rep.failed.is_empty() || !errors.is_empty() { 3 } else { 0 })
}

fn load_preset(name: &str) -> Result<Preset, Failure> {
    let p = Path::new(name);
    if p.extension().map(|e| e == "toml").unwrap_or(false) {
        let text = std::fs::read_to_string(p).map_err(|e| fail(1, format!("{name}: {e}")))?;
        return Preset::parse(&text).map_err(|e| fail(1, format!("{name}: {e}")));
    }
    builtin(name).ok_or_else(|| fail(1, format!("unknown preset '{name}' (see `clippro-cli presets`)")))
}

/// Applies a `--set key=value` override to resolved settings.
fn apply_set(s: &mut Settings, kv: &str) -> Result<(), Failure> {
    let (k, v) = kv.split_once('=').ok_or_else(|| fail(1, format!("--set expects key=value, got '{kv}'")))?;
    let bad = |m: String| fail(1, format!("--set {kv}: {m}"));
    let pair = |v: &str| -> Result<(String, String), Failure> {
        v.split_once(',').map(|(a, b)| (a.to_string(), b.to_string())).ok_or_else(|| bad("expects two values separated by a comma".into()))
    };
    match k {
        "score" => {
            let (a, b) = pair(v)?;
            s.score_lo = parse_score(&a).map_err(|e| bad(e.to_string()))? as f32;
            s.score_hi = parse_score(&b).map_err(|e| bad(e.to_string()))? as f32;
        }
        "length" => {
            let (a, b) = pair(v)?;
            s.len_min_s = parse_duration(&a).map_err(|e| bad(e.to_string()))?.unwrap_or(0.0);
            s.len_max_s = parse_duration(&b).map_err(|e| bad(e.to_string()))?;
        }
        "cap" => s.max_per_clip = parse_count(v).map_err(|e| bad(e.to_string()))?,
        "gap" => s.gap_s = parse_duration(v).map_err(|e| bad(e.to_string()))?.unwrap_or(0.0),
        "handles" => s.handles_samples = v.parse().map_err(|_| bad("expects a whole number".into()))?,
        "centre" => s.centre_weighting = v.parse().map_err(|_| bad("expects 0–1".into()))?,
        _ => {
            if let Some(g) = k.strip_prefix("weight.") {
                let grp = match g {
                    "focus" => MetricGroup::Focus,
                    "exposure" => MetricGroup::Exposure,
                    "motion" => MetricGroup::Motion,
                    "audio" => MetricGroup::Audio,
                    "content" => MetricGroup::Content,
                    _ => return Err(bad("unknown group".into())),
                };
                s.weights[grp as usize] = v.parse().map_err(|_| bad("expects a number".into()))?;
            } else if let Some(sub) = k.strip_prefix("sub.") {
                let sm = clippro_score::SubMetric::from_key(sub).ok_or_else(|| bad("unknown sub-metric".into()))?;
                let st = s.sub_mut(sm);
                if v == "off" {
                    st.enabled = false;
                } else {
                    st.enabled = true;
                    st.pref = v.parse::<f32>().map_err(|_| bad("expects −1..1 or off".into()))?.clamp(-1.0, 1.0);
                }
            } else if let Some(f) = k.strip_prefix("flag.") {
                let fk = FlagKind::from_key(f).ok_or_else(|| bad("unknown flag".into()))?;
                let tier: Tier = serde_json::from_str(&format!("\"{v}\"")).map_err(|_| bad("expects force_exclude|disfavour|neutral|favour|force_include".into()))?;
                s.tiers[fk as usize] = tier;
            } else {
                return Err(bad("unknown key".into()));
            }
        }
    }
    Ok(())
}

fn tc(us: i64) -> String {
    let t = us.max(0) as f64 / 1e6;
    let (h, m, s) = ((t / 3600.0) as i64, ((t / 60.0) as i64) % 60, t % 60.0);
    format!("{h:02}:{m:02}:{s:06.3}")
}

fn sort_order(sort: &str) -> Result<SortOrder, Failure> {
    Ok(match sort {
        "timeline" => SortOrder::Timeline,
        "score" => SortOrder::ScoreDesc,
        "score-asc" => SortOrder::ScoreAsc,
        "duration" => SortOrder::DurationDesc,
        "duration-asc" => SortOrder::DurationAsc,
        other => return Err(fail(1, format!("unknown sort '{other}'"))),
    })
}

#[allow(clippy::too_many_arguments)]
fn export(
    project: &Path,
    format: &str,
    out: &Path,
    preset: &str,
    sets: &[String],
    sort: &str,
    layout: &str,
    no_handles: bool,
    gap: &str,
    no_markers: bool,
    no_audio: bool,
    name: &str,
    json_out: bool,
) -> Result<u8, Failure> {
    if !project.exists() {
        return Err(fail(1, format!("{} doesn't exist", project.display())));
    }
    let report = clippro_core::report::ReportKind::parse(format);
    let fmt = clippro_interchange::Format::parse(format);
    if fmt.is_none() && report.is_none() {
        return Err(fail(1, format!("unknown format '{format}'")));
    }
    let mut settings = Settings::from_preset(&load_preset(preset)?).map_err(|e| fail(1, e.to_string()))?;
    for kv in sets {
        apply_set(&mut settings, kv)?;
    }
    let layout = match layout {
        "selects" => clippro_interchange::Layout::SelectsOnly,
        "with-rejects" => clippro_interchange::Layout::SelectsOverRejects,
        other => return Err(fail(1, format!("unknown layout '{other}'"))),
    };
    let gap_s = parse_duration(gap).map_err(|e| fail(1, format!("--gap: {e}")))?.unwrap_or(0.0);
    let opts = clippro_interchange::ExportOptions {
        name: name.to_string(),
        layout,
        handles: !no_handles,
        gap_s,
        with_audio: !no_audio,
        markers: !no_markers,
        ..clippro_interchange::ExportOptions::default()
    };
    let mut so = SessionOptions::default();
    so.cache_path = None;
    let (s, _) = Session::open(project, so).map_err(|e| fail(4, e.to_string()))?;
    let n = match (fmt, report) {
        (Some(fmt), _) => s.export_to(&settings, sort_order(sort)?, &opts, fmt, out),
        (None, Some(kind)) => {
            let title = if name.is_empty() { project.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() } else { name.to_string() };
            let ro = clippro_core::report::ReportOptions { title, generated: clippro_core::report::utc_now_string(), ..Default::default() };
            s.write_report(&settings, sort_order(sort)?, kind, &ro, out)
        }
        (None, None) => Err(clippro_core::SessionError::Other(format!("unknown format {format}"))),
    }
    .map_err(|e| fail(4, e.to_string()))?;
    s.close().map_err(|e| fail(4, e.to_string()))?;
    if json_out {
        println!("{}", json!({ "path": out, "format": format, "selects": n }));
    } else {
        println!("wrote {n} select(s) to {}", out.display());
    }
    Ok(0)
}

fn score(project: &Path, preset: &str, sets: &[String], sort: &str, json_out: bool) -> Result<u8, Failure> {
    if !project.exists() {
        return Err(fail(1, format!("{} doesn't exist", project.display())));
    }
    let preset = load_preset(preset)?;
    let mut settings = Settings::from_preset(&preset).map_err(|e| fail(1, e.to_string()))?;
    for kv in sets {
        apply_set(&mut settings, kv)?;
    }
    let order = match sort {
        "timeline" => SortOrder::Timeline,
        "score" => SortOrder::ScoreDesc,
        "score-asc" => SortOrder::ScoreAsc,
        "duration" => SortOrder::DurationDesc,
        "duration-asc" => SortOrder::DurationAsc,
        other => return Err(fail(1, format!("unknown sort '{other}'"))),
    };
    let mut opts = SessionOptions::default();
    opts.cache_path = None;
    let (s, _) = Session::open(project, opts).map_err(|e| fail(4, e.to_string()))?;
    let sel = s.score(&settings).map_err(|e| fail(4, e.to_string()))?;
    let names = s.clip_names().map_err(|e| fail(4, e.to_string()))?;
    let analysed = sel.clips.len();
    let total = s.clip_statuses().map_err(|e| fail(4, e.to_string()))?.len();
    let refs = sel.sorted(order);
    if json_out {
        let rows: Vec<_> = refs
            .iter()
            .map(|r| {
                let (name, path) = names.get(&r.clip).cloned().unwrap_or_default();
                json!({ "clip": r.clip.0, "name": name, "path": path, "start_us": r.segment.start_us, "end_us": r.segment.end_us,
                        "in_us": r.segment.in_us, "out_us": r.segment.out_us, "score": r.segment.mean,
                        "kind": format!("{:?}", r.segment.kind).to_lowercase() })
            })
            .collect();
        println!(
            "{}",
            json!({ "preset": settings.preset_name, "clips_scored": analysed, "clips_total": total, "selects": rows,
                    "summary": { "count": sel.summary.segment_count, "selects_us": sel.summary.selects_us,
                                 "footage_us": sel.summary.footage_us, "percent": sel.summary.percent,
                                 "flag_runs": sel.summary.flag_runs.iter().map(|(f, n)| (f.key(), *n)).collect::<std::collections::BTreeMap<_, _>>() } })
        );
    } else {
        println!("{} selects · {} · {:.1}% of footage · preset {}", sel.summary.segment_count, tc(sel.summary.selects_us), sel.summary.percent, settings.preset_name);
        if analysed < total {
            println!("({} of {total} clips analysed; the rest aren't scored yet)", analysed);
        }
        for r in &refs {
            let name = names.get(&r.clip).map(|n| n.0.as_str()).unwrap_or("?");
            println!("  {:<28} {} – {}  score {:>5.1}{}", name, tc(r.segment.start_us), tc(r.segment.end_us), r.segment.mean * 100.0,
                if r.segment.kind == clippro_score::SegmentKind::Auto { "" } else { "  (forced)" });
        }
    }
    s.close().map_err(|e| fail(4, e.to_string()))?;
    Ok(0)
}
