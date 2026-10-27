//! M1 end-to-end: import → analyse → score → reopen; proxies, duplicates and the global cache.
//! Needs the corpus (`clippro-testgen --out testdata/gen --mini`); skips when missing.

use clippro_analysis::jobs::SchedulerConfig;
use clippro_core::{Session, SessionOptions};
use clippro_project::RunState;
use clippro_score::{builtin, Settings, SortOrder};
use std::path::{Path, PathBuf};

fn corpus() -> Option<PathBuf> {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/gen");
    if d.join("tripod_static.mp4").exists() {
        Some(d)
    } else {
        eprintln!("corpus missing: skipping");
        None
    }
}

fn opts(cache: &Path) -> SessionOptions {
    SessionOptions {
        scheduler: SchedulerConfig { workers: 4, ..SchedulerConfig::default() },
        cache_path: Some(cache.to_path_buf()),
        ..SessionOptions::default()
    }
}

#[test]
fn import_analyse_score_reopen_and_cache() {
    let Some(dir) = corpus() else { return };
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-session-test");
    let _ = std::fs::remove_dir_all(&tmp);
    let media = tmp.join("media");
    std::fs::create_dir_all(media.join("Proxy")).unwrap();
    for f in ["tripod_static.mp4", "solid_black.mp4", "handheld_shake.mp4"] {
        std::fs::copy(dir.join(f), media.join(f)).unwrap();
    }
    // A proxy for tripod_static (same duration, different content) and a duplicate copy.
    std::fs::copy(dir.join("pan_left.mp4"), media.join("Proxy").join("tripod_static_Proxy.mp4")).unwrap();
    std::fs::copy(dir.join("tripod_static.mp4"), media.join("zz_copy_of_tripod.mp4")).unwrap();
    std::fs::write(media.join("broken.mp4"), b"not a video").unwrap();
    let cache = tmp.join("cache.sqlite");
    let proj = tmp.join("p.clippro");

    let s = Session::create(&proj, "E2E", opts(&cache)).unwrap();
    let rep = s.import(&[media.clone()]).unwrap();
    assert_eq!(rep.added.len(), 3, "{:?}", rep.added);
    assert_eq!(rep.duplicates.len(), 1);
    assert_eq!(rep.proxies_linked.len(), 1, "{:?}", rep.proxies_linked);
    assert_eq!(rep.failed.len(), 1);
    assert_eq!(s.queue_analysis().unwrap(), 3);
    let mut events = 0;
    s.wait(|_| events += 1);
    assert!(events > 0);
    assert!(s.take_storage_errors().is_empty());
    let st = s.clip_statuses().unwrap();
    assert!(st.iter().all(|c| c.state == RunState::Done), "{st:?}");

    let settings = Settings::from_preset(&builtin("Cleanup").unwrap()).unwrap();
    let sel = s.score(&settings).unwrap();
    let names = s.clip_names().unwrap();
    let count = |name: &str| {
        sel.clips.iter().find(|c| names[&c.clip].0 == name).map(|c| c.segments.len()).unwrap_or(usize::MAX)
    };
    assert_eq!(count("solid_black"), 0, "leaders must not be selected");
    // tripod_static was analysed from its proxy (pan_left content), which is still usable.
    assert!(count("tripod_static") >= 1);
    let before = sel.sorted(SortOrder::Timeline);
    assert_eq!(s.queue_analysis().unwrap(), 0, "nothing left to analyse");
    s.close().unwrap();

    // Reopen: same selects without re-analysis.
    let (s, report) = Session::open(&proj, opts(&cache)).unwrap();
    assert!(!report.recovered);
    assert_eq!(s.queue_analysis().unwrap(), 0);
    let again = s.score(&settings).unwrap().sorted(SortOrder::Timeline);
    assert_eq!(before, again);
    s.close().unwrap();

    // A second project reuses the global cache for the same content.
    let proj2 = tmp.join("p2.clippro");
    let s2 = Session::create(&proj2, "Cached", opts(&cache)).unwrap();
    let rep2 = s2.import(&[media.join("handheld_shake.mp4")]).unwrap();
    assert_eq!(rep2.cache_hits, 1);
    assert_eq!(s2.queue_analysis().unwrap(), 0, "cache hit must not decode");
    assert_eq!(s2.score(&settings).unwrap().clips.len(), 1);
    s2.close().unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

/// PLAN P6.4: the inspector agrees with the selects for 1,000 random samples.
#[test]
fn inspect_agrees_with_selects() {
    let Some(dir) = corpus() else { return };
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-inspect-test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let s = Session::create(&tmp.join("p.clippro"), "Inspect", SessionOptions { cache_path: None, ..opts(&tmp) }).unwrap();
    let files: Vec<PathBuf> = ["tripod_static.mp4", "handheld_shake.mp4", "solid_black.mp4", "hard_cuts.mp4", "focus_hunt.mp4"].iter().map(|f| dir.join(f)).collect();
    s.import(&files).unwrap();
    s.queue_analysis().unwrap();
    s.wait(|_| {});
    let clips = s.clip_inputs().unwrap();
    assert_eq!(clips.len(), 5);

    let mut rng = 0x2545_f491_4f6c_dd1du64;
    let mut next = || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    let (mut passes, mut fails) = (0, 0);
    for preset in ["All-Rounder", "Cleanup", "Locked-Off"] {
        let settings = Settings::from_preset(&builtin(preset).unwrap()).unwrap();
        let sel = s.score(&settings).unwrap();
        for _ in 0..334 {
            let c = &clips[(next() % clips.len() as u64) as usize];
            let k = (next() % c.ms.len() as u64) as usize;
            let ins = s.inspect(c.id, &settings, k).unwrap();
            let cs = sel.clips.iter().find(|x| x.clip == c.id).unwrap();
            let in_select = cs.segments.iter().any(|g| (g.first_sample..=g.last_sample).contains(&k));
            assert_eq!(ins.pass, in_select, "{preset} clip {:?} sample {k}: {:?}", c.id, ins.reasons);
            let f = cs.scores.score[k];
            let b = ins.breakdown.score;
            assert!((f.is_nan() && b.is_nan()) || (f - b).abs() < 1e-5, "{preset} sample {k}: engine {f}, explain {b}");
            assert_eq!(ins.breakdown.force_out, cs.scores.force_out[k]);
            assert_eq!(ins.breakdown.force_in, cs.scores.force_in[k]);
            assert!(!(ins.pass && ins.breakdown.force_out), "force-excluded samples are never selected");
            assert!(!ins.reasons.is_empty());
            assert_eq!(ins.raw.len(), clippro_measure::columns::NCOLS);
            if ins.pass {
                passes += 1;
            } else {
                fails += 1;
            }
        }
    }
    eprintln!("inspected {} samples: {passes} in selects, {fails} not", passes + fails);
    assert!(passes > 50 && fails > 50, "both outcomes exercised ({passes}/{fails})");
    s.close().unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

/// PLAN P7.6: the HTML report and contact sheet are self-contained (no external URLs), carry a
/// WebP thumbnail per select and have balanced markup.
#[test]
fn html_report_and_contact_sheet() {
    use clippro_core::report::{render_contact_sheet, render_html, ReportOptions};
    let Some(dir) = corpus() else { return };
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-report-test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let s = Session::create(&tmp.join("p.clippro"), "Report", SessionOptions { cache_path: None, ..opts(&tmp) }).unwrap();
    let files: Vec<PathBuf> = ["tripod_static.mp4", "handheld_shake.mp4", "solid_black.mp4", "prores_hq.mov"].iter().map(|f| dir.join(f)).collect();
    s.import(&files).unwrap();
    s.queue_analysis().unwrap();
    s.wait(|_| {});
    let settings = Settings::from_preset(&builtin("Cleanup").unwrap()).unwrap();
    let t = std::time::Instant::now();
    let d = s.report_data(&settings, SortOrder::Timeline, &ReportOptions { title: "Report <test> & co".into(), generated: "2026-10-27 10:00".into(), thumb_width: 320 }).unwrap();
    eprintln!("report data for {} selects in {:?}", d.rows.len(), t.elapsed());
    assert!(!d.rows.is_empty());
    assert_eq!(d.clips.len(), 4);
    for r in &d.rows {
        assert!(r.thumb.starts_with("data:image/webp;base64,UklGR"), "select {} has no WebP thumbnail", r.index);
        assert!(r.tc_in.contains(':') && r.duration_s > 0.0);
    }
    for (name, html) in [("report", render_html(&d)), ("sheet", render_contact_sheet(&d))] {
        assert!(!html.contains("http://") && !html.contains("https://") && !html.contains("//cdn"), "{name} has an external URL");
        assert!(html.starts_with("<!doctype html>") && html.ends_with("</html>"));
        assert!(html.contains("Report &lt;test&gt; &amp; co"), "{name} escapes the title");
        for tag in ["div", "table", "thead", "tbody", "tr", "td", "th", "span", "h1", "h2", "footer"] {
            let open = html.matches(&format!("<{tag}>")).count() + html.matches(&format!("<{tag} ")).count();
            let close = html.matches(&format!("</{tag}>")).count();
            assert_eq!(open, close, "{name}: <{tag}> opened {open} times, closed {close}");
        }
        assert_eq!(html.matches("data:image/webp").count(), d.rows.len());
        std::fs::write(tmp.join(format!("{name}.html")), &html).unwrap();
    }
    s.close().unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}
