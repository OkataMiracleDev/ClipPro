//! `offload`, `proxy`, `render`, `bench` and `selftest` (SPEC §20, PLAN P13.4).

use crate::{fail, load_preset, session_opts, sort_order, Failure};
use clippro_core::{Session, SessionOptions};
use clippro_project::RunState;
use clippro_score::{builtin, Settings, SortOrder};
use clippro_types::{FlagKind, GroupMask, SampleRate};
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

fn progress_line(json_out: bool, text: &str) {
    if !json_out {
        eprint!("\r{text}   ");
        let _ = std::io::stderr().flush();
    }
}

pub fn offload(src: &Path, dests: &[PathBuf], hash: &str, json_out: bool) -> Result<u8, Failure> {
    use clippro_ingest::offload::{run, OffloadError, OffloadEvent, OffloadJob};
    let mut job = OffloadJob::new(src, dests.to_vec());
    job.hash = clippro_ingest::hash::HashKind::parse(hash).ok_or_else(|| fail(1, format!("unknown hash '{hash}' (xxh64, xxh3, xxh128, md5, sha1)")))?;
    let mut files = 0;
    let rep = run(&job, &AtomicBool::new(false), |e| match e {
        OffloadEvent::Started { files: n, .. } => files = *n,
        OffloadEvent::Progress { done, total } => progress_line(json_out, &format!("offload: {:.1} of {:.1} GB ({:.0}%)", *done as f64 / 1e9, *total as f64 / 1e9, 100.0 * *done as f64 / (*total).max(1) as f64)),
        _ => {}
    })
    .map_err(|e| match e {
        OffloadError::Io(..) => fail(2, e.to_string()),
        OffloadError::Other(m) => fail(1, m),
        OffloadError::Cancelled => fail(4, "cancelled"),
    })?;
    if json_out {
        println!("{}", serde_json::to_string(&rep).map_err(|e| fail(4, e.to_string()))?);
    } else {
        eprintln!();
        println!(
            "{} files ({:.2} GB) in {:.1}s: {} copied, {} already verified, {} failed",
            files,
            rep.bytes as f64 / 1e9,
            rep.seconds,
            rep.copied,
            rep.skipped,
            rep.failed.len()
        );
        for (p, k, why) in &rep.failed {
            println!("  FAILED {p} → {}: {why}", dests.get(*k).map(|d| d.display().to_string()).unwrap_or_default());
        }
        for m in &rep.manifests {
            println!("  history: {}", m.display());
        }
    }
    Ok(if rep.ok() { 0 } else { 3 })
}

fn open(project: &Path) -> Result<Session, Failure> {
    if !project.exists() {
        return Err(fail(1, format!("{} doesn't exist", project.display())));
    }
    let so = SessionOptions { cache_path: None, ..SessionOptions::default() };
    Session::open(project, so).map(|(s, _)| s).map_err(|e| fail(4, e.to_string()))
}

pub fn proxy(project: &Path, codec: &str, height: u32, out_dir: Option<&Path>, json_out: bool) -> Result<u8, Failure> {
    use clippro_ingest::proxy::{ProxyCodec, ProxyOptions};
    let codec: ProxyCodec = serde_json::from_value(serde_json::Value::String(codec.replace('-', "_"))).map_err(|_| fail(1, format!("unknown codec '{codec}' (prores-proxy, prores-lt, dnxhr-lb, h264)")))?;
    let s = open(project)?;
    let opts = ProxyOptions { codec, max_height: height.max(144), out_dir: out_dir.map(Path::to_path_buf) };
    let clips = s.clip_statuses().map_err(|e| fail(4, e.to_string()))?;
    let mut made = Vec::new();
    let mut failed = Vec::new();
    for (i, c) in clips.iter().enumerate() {
        let r = s.make_proxy(c.clip, &opts, &AtomicBool::new(false), |f| progress_line(json_out, &format!("proxy {}/{}: {} {:.0}%", i + 1, clips.len(), c.name, f * 100.0)));
        match r {
            Ok(p) => made.push(json!({ "clip": c.name, "proxy": p })),
            Err(e) => failed.push(json!({ "clip": c.name, "error": e.to_string() })),
        }
    }
    s.close().map_err(|e| fail(4, e.to_string()))?;
    if json_out {
        println!("{}", json!({ "proxies": made, "failed": failed }));
    } else {
        eprintln!();
        println!("made {} prox{}{}", made.len(), if made.len() == 1 { "y" } else { "ies" }, if failed.is_empty() { String::new() } else { format!(", {} failed", failed.len()) });
        for f in &failed {
            println!("  FAILED {}: {}", f["clip"].as_str().unwrap_or(""), f["error"].as_str().unwrap_or(""));
        }
    }
    Ok(if failed.is_empty() { 0 } else { 3 })
}

#[allow(clippy::too_many_arguments)]
pub fn render(project: &Path, out: &Path, preset: &str, sort: &str, copy: bool, nearest: bool, codec: &str, template: &str, no_handles: bool, json_out: bool) -> Result<u8, Failure> {
    use clippro_ingest::render::{RenderCodec, RenderOptions};
    let codec: RenderCodec = serde_json::from_value(serde_json::Value::String(codec.replace('-', "_"))).map_err(|_| fail(1, format!("unknown codec '{codec}' (prores422, prores-hq, dnxhr-hq, h264)")))?;
    let settings = Settings::from_preset(&load_preset(preset)?).map_err(|e| fail(1, e.to_string()))?;
    let s = open(project)?;
    let opts = RenderOptions { stream_copy: copy, allow_nearest_keyframe: nearest, codec, out_dir: out.to_path_buf(), template: template.to_string() };
    let r = s
        .render_selects(&settings, sort_order(sort)?, &opts, !no_handles, &AtomicBool::new(false), |i, n, f| progress_line(json_out, &format!("render {}/{n}: {:.0}%", i + 1, f * 100.0)))
        .map_err(|e| fail(2, e.to_string()))?;
    s.close().map_err(|e| fail(4, e.to_string()))?;
    if json_out {
        println!("{}", serde_json::to_string(&r).map_err(|e| fail(4, e.to_string()))?);
    } else {
        eprintln!();
        let copied = r.iter().filter(|o| o.copied).count();
        println!("rendered {} select(s) to {} ({copied} stream-copied)", r.len(), out.display());
    }
    Ok(0)
}

/// Analysis speed on the given clips (no cache): realtime factor per clip and overall.
pub fn bench(paths: &[PathBuf], rate: &str, cpu_only: bool, dir: Option<&Path>, json_out: bool) -> Result<u8, Failure> {
    let rate = SampleRate::parse(rate).ok_or_else(|| fail(1, format!("unknown sample rate '{rate}'")))?;
    let dir = dir.map(|d| d.join(format!("clippro-bench-{}", std::process::id()))).unwrap_or_else(|| std::env::temp_dir().join(format!("clippro-bench-{}", std::process::id())));
    std::fs::create_dir_all(&dir).map_err(|e| fail(4, e.to_string()))?;
    let result = (|| {
        let s = Session::create(&dir.join("bench.clippro"), "Bench", session_opts(rate, GroupMask::ALL_COMPUTE, None, cpu_only, true, false)).map_err(|e| fail(4, e.to_string()))?;
        s.import(paths).map_err(|e| fail(2, e.to_string()))?;
        let t0 = std::time::Instant::now();
        s.queue_analysis().map_err(|e| fail(4, e.to_string()))?;
        s.wait(|_| progress_line(json_out, &format!("bench: {:.1}s", t0.elapsed().as_secs_f64())));
        let wall = t0.elapsed().as_secs_f64();
        let st = s.clip_statuses().map_err(|e| fail(4, e.to_string()))?;
        let footage_us: i64 = s.clip_inputs().map_err(|e| fail(4, e.to_string()))?.iter().map(|c| c.duration_us).sum();
        let failed = st.iter().filter(|c| c.state != RunState::Done).count();
        s.close().map_err(|e| fail(4, e.to_string()))?;
        Ok::<_, Failure>((wall, footage_us, st.len(), failed))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    let (wall, footage_us, n, failed) = result?;
    let rt = footage_us as f64 / 1e6 / wall.max(1e-6);
    if json_out {
        println!("{}", json!({ "clips": n, "failed": failed, "footage_s": footage_us as f64 / 1e6, "wall_s": wall, "realtime": rt, "hw_decode": !cpu_only && clippro_media::video::hw_decode_available() }));
    } else {
        eprintln!();
        println!("{n} clip(s), {:.1}s of footage analysed in {wall:.1}s: {rt:.1}× realtime{}", footage_us as f64 / 1e6, if failed > 0 { format!(" ({failed} failed)") } else { String::new() });
    }
    Ok(if failed > 0 { 3 } else { 0 })
}

struct Check {
    name: &'static str,
    ok: bool,
    detail: String,
}

/// Generates three clips with ffmpeg (sharp, black, blurred), analyses and scores them, and
/// checks the results and an export round trip.
pub fn selftest(dir: Option<&Path>, keep: bool, json_out: bool) -> Result<u8, Failure> {
    let dir = dir.map(Path::to_path_buf).unwrap_or_else(|| std::env::temp_dir().join(format!("clippro-selftest-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| fail(4, format!("{}: {e}", dir.display())))?;
    let result = run_selftest(&dir);
    if !keep {
        let _ = std::fs::remove_dir_all(&dir);
    }
    let checks = result?;
    let passed = checks.iter().filter(|c| c.ok).count();
    if json_out {
        let v: Vec<_> = checks.iter().map(|c| json!({ "check": c.name, "pass": c.ok, "detail": c.detail })).collect();
        println!("{}", json!({ "passed": passed, "total": checks.len(), "checks": v }));
    } else {
        for c in &checks {
            println!("{} {:<34} {}", if c.ok { "PASS" } else { "FAIL" }, c.name, c.detail);
        }
        println!("{passed}/{} checks passed", checks.len());
    }
    Ok(if passed == checks.len() { 0 } else { 3 })
}

fn run_selftest(dir: &Path) -> Result<Vec<Check>, Failure> {
    let mut checks = Vec::new();
    let Some(ff) = clippro_ingest::ffmpeg::ffmpeg_exe() else {
        checks.push(Check { name: "ffmpeg.exe present", ok: false, detail: "not found beside clippro-cli or on PATH".into() });
        return Ok(checks);
    };
    checks.push(Check { name: "ffmpeg.exe present", ok: true, detail: ff.display().to_string() });
    let gen = |name: &str, filter: &str, secs: u32| -> Result<PathBuf, Failure> {
        let out = dir.join(name);
        let st = std::process::Command::new(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!("{filter},format=yuv420p"))
            .args(["-f", "lavfi", "-i"])
            .arg(format!("sine=frequency=440:sample_rate=48000:duration={secs}"))
            .args(["-t", &secs.to_string(), "-c:v", "mpeg4", "-q:v", "3", "-g", "25", "-c:a", "aac", "-shortest"])
            .arg(&out)
            .stdin(std::process::Stdio::null())
            .status()
            .map_err(|e| fail(4, format!("ffmpeg: {e}")))?;
        if !st.success() {
            return Err(fail(4, format!("ffmpeg couldn't generate {name}")));
        }
        Ok(out)
    };
    let media = vec![
        gen("sharp.mp4", "testsrc2=size=640x360:rate=25", 6)?,
        gen("black.mp4", "color=c=black:size=640x360:rate=25", 4)?,
        gen("blurred.mp4", "testsrc2=size=640x360:rate=25,scale=64:36,scale=640:360:flags=bilinear", 6)?,
    ];
    checks.push(Check { name: "generate test clips", ok: true, detail: "sharp, black, blurred".into() });
    let opts = session_opts(SampleRate::Standard, GroupMask::ALL_COMPUTE, Some(2), false, true, false);
    let s = Session::create(&dir.join("selftest.clippro"), "Selftest", opts).map_err(|e| fail(4, e.to_string()))?;
    let rep = s.import(&media).map_err(|e| fail(4, e.to_string()))?;
    checks.push(Check { name: "import", ok: rep.added.len() == 3, detail: format!("{} of 3 imported", rep.added.len()) });
    s.queue_analysis().map_err(|e| fail(4, e.to_string()))?;
    s.wait(|_| {});
    let st = s.clip_statuses().map_err(|e| fail(4, e.to_string()))?;
    let done = st.iter().filter(|c| c.state == RunState::Done).count();
    checks.push(Check { name: "analysis", ok: done == 3, detail: format!("{done} of 3 analysed") });
    let id = |n: &str| st.iter().find(|c| c.name == n).map(|c| c.clip);
    let settings = Settings::from_preset(&builtin("All-Rounder").ok_or_else(|| fail(4, "preset missing"))?).map_err(|e| fail(4, e.to_string()))?;
    let sel = s.score(&settings).map_err(|e| fail(4, e.to_string()))?;
    let clip = |n: &str| id(n).and_then(|c| sel.clips.iter().find(|x| x.clip == c));
    let black_flag = clip("black").map(|c| c.scores.flags.iter().filter(|f| *f & FlagKind::SolidColour.bit() != 0).count() as f32 / c.scores.flags.len().max(1) as f32).unwrap_or(0.0);
    checks.push(Check { name: "solid colour flagged", ok: black_flag > 0.8, detail: format!("{:.0}% of black.mp4 samples", black_flag * 100.0) });
    let black_selects = clip("black").map_or(0, |c| c.segments.len());
    checks.push(Check { name: "black clip rejected", ok: black_selects == 0, detail: format!("{black_selects} select(s)") });
    let sharp_selects = clip("sharp").map_or(0, |c| c.segments.len());
    checks.push(Check { name: "sharp clip selected", ok: sharp_selects > 0, detail: format!("{sharp_selects} select(s)") });
    // Focus, measured: median full-frame sharpness (log10) of each clip.
    let inputs = s.clip_inputs().map_err(|e| fail(4, e.to_string()))?;
    let sharpness = |n: &str| {
        let c = id(n).and_then(|c| inputs.iter().find(|x| x.id == c));
        let mut v: Vec<f32> = c.map(|c| c.ms.col(clippro_measure::columns::Col::SharpFull).to_vec()).unwrap_or_default();
        v.retain(|x| x.is_finite());
        v.sort_by(f32::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(f32::NAN)
    };
    let (fs, fb) = (sharpness("sharp"), sharpness("blurred"));
    checks.push(Check { name: "sharp measures sharper than blurred", ok: fs - fb > 0.5, detail: format!("log10 sharpness {fs:.2} vs {fb:.2}") });
    let xml = dir.join("selects.xml");
    let n = s.export_to(&settings, SortOrder::Timeline, &Default::default(), clippro_interchange::Format::Xmeml, &xml).map_err(|e| fail(4, e.to_string()))?;
    let back = std::fs::read_to_string(&xml).ok().and_then(|t| clippro_interchange::import::import(&t, clippro_types::FrameRate::new(25, 1)).ok());
    let back_n = back.as_ref().map_or(0, |t| clippro_interchange::import::edit_signature(t).len());
    checks.push(Check { name: "export and re-import", ok: n > 0 && back_n == n, detail: format!("{n} exported, {back_n} read back") });
    checks.push(Check { name: "GPU decode (informational)", ok: true, detail: if clippro_media::video::hw_decode_available() { "D3D11VA available".into() } else { "CPU decoding".into() } });
    s.close().map_err(|e| fail(4, e.to_string()))?;
    Ok(checks)
}
