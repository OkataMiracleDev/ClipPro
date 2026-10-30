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

/// P5.3 context menu: Retry (with CPU), Move to next, Link proxy.
#[test]
fn retry_bump_and_link_proxy() {
    let Some(dir) = corpus() else { return };
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-retry-test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let s = Session::create(&tmp.join("p.clippro"), "Retry", SessionOptions { cache_path: None, ..opts(&tmp) }).unwrap();
    s.import(&[dir.join("tripod_static.mp4"), dir.join("solid_black.mp4")]).unwrap();
    s.queue_analysis().unwrap();
    s.wait(|_| {});
    let st = s.clip_statuses().unwrap();
    let tripod = st.iter().find(|c| c.name == "tripod_static").unwrap().clip;
    let media = st.iter().find(|c| c.name == "tripod_static").unwrap().media;

    assert!(s.retry(tripod, true).unwrap(), "retry queues the clip");
    s.bump(tripod).unwrap();
    s.wait(|_| {});
    assert!(s.clip_statuses().unwrap().iter().all(|c| c.state == RunState::Done));

    // Wrong length is refused; a matching file links and becomes the preview/analysis source.
    assert!(s.link_proxy(tripod, &dir.join("long_clip.mp4")).is_err());
    s.link_proxy(tripod, &dir.join("pan_left.mp4")).unwrap();
    s.wait(|_| {});
    let (path, _) = s.preview_source(media).unwrap().unwrap();
    assert!(path.ends_with("pan_left.mp4"), "{}", path.display());
    assert!(s.clip_statuses().unwrap().iter().all(|c| c.state == RunState::Done));
    assert_eq!(s.clip_statuses().unwrap().len(), 2, "the proxy isn't a clip");
    s.close().unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

/// PLAN P9.2: the Pan / Tilt / Twist / Locked-Off presets select the matching corpus clips.
#[test]
fn camera_presets_pick_matching_clips() {
    let Some(dir) = corpus() else { return };
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-camera-test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let s = Session::create(&tmp.join("p.clippro"), "Camera", SessionOptions { cache_path: None, ..opts(&tmp) }).unwrap();
    let names = ["pan_left", "pan_right", "tilt_up", "tilt_down", "rotate_cw", "rotate_ccw", "tripod_static"];
    s.import(&names.iter().map(|n| dir.join(format!("{n}.mp4"))).collect::<Vec<_>>()).unwrap();
    s.queue_analysis().unwrap();
    s.wait(|_| {});
    let by_id: std::collections::HashMap<_, _> = s.clip_statuses().unwrap().into_iter().map(|c| (c.clip, c.name)).collect();
    for (preset, want) in [("Pan", &["pan_left", "pan_right"][..]), ("Tilt", &["tilt_up", "tilt_down"]), ("Twist", &["rotate_cw", "rotate_ccw"]), ("Locked-Off", &["tripod_static"])] {
        let settings = Settings::from_preset(&builtin(preset).unwrap()).unwrap();
        let top = s.camera_matches(&settings, 7).unwrap();
        let got: Vec<&str> = top.iter().map(|(c, _)| by_id[c].as_str()).collect();
        eprintln!("{preset}: {:?}", top.iter().map(|(c, v)| format!("{} {:.2}", by_id[c], v)).collect::<Vec<_>>());
        let mut head: Vec<&str> = got[..want.len()].to_vec();
        head.sort();
        let mut w = want.to_vec();
        w.sort();
        assert_eq!(head, w, "{preset} ranks {got:?}");
        assert!(top[want.len() - 1].1 > top[want.len()].1 + 0.05, "{preset}: matches clearly separated: {top:?}");
    }
    s.close().unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

/// PLAN P9.7: overrides survive re-scoring; annotations follow their select into exports.
#[test]
fn overrides_and_annotations() {
    let Some(dir) = corpus() else { return };
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-override-test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let s = Session::create(&tmp.join("p.clippro"), "Overrides", SessionOptions { cache_path: None, ..opts(&tmp) }).unwrap();
    s.import(&[dir.join("tripod_static.mp4"), dir.join("solid_black.mp4")]).unwrap();
    s.queue_analysis().unwrap();
    s.wait(|_| {});
    let st = s.clip_statuses().unwrap();
    let id = |n: &str| st.iter().find(|c| c.name == n).unwrap().clip;
    let (tripod, black) = (id("tripod_static"), id("solid_black"));
    let mut settings = Settings::from_preset(&builtin("Cleanup").unwrap()).unwrap();
    let count = |s: &Session, set: &Settings, c| s.score(set).unwrap().clips.iter().find(|x| x.clip == c).map_or(0, |x| x.segments.len());
    assert_eq!(count(&s, &settings, black), 0);

    let o = s.add_override(black, "force_in", 1_000_000, 3_000_000).unwrap();
    assert_eq!(count(&s, &settings, black), 1, "force-in adds a select");
    settings.score_lo = 0.9; // re-score with very different settings
    assert_eq!(count(&s, &settings, black), 1, "the override survives re-scoring");
    assert!(s.add_override(black, "explode", 0, 1).is_err());
    assert_eq!(s.list_overrides().unwrap().len(), 1);
    s.remove_override(o).unwrap();
    assert_eq!(count(&s, &settings, black), 0);

    settings = Settings::from_preset(&builtin("Cleanup").unwrap()).unwrap();
    let seg = s.score(&settings).unwrap().clips.iter().find(|x| x.clip == tripod).unwrap().segments[0].clone();
    s.set_annotation(tripod, seg.start_us + 100_000, seg.end_us, 5, Some("purple".into()), "Hero shot").unwrap();
    assert_eq!(s.annotations().unwrap().len(), 1);
    assert_eq!(count(&s, &settings, tripod), 1, "annotations don't change scoring");
    let opts = clippro_interchange::ExportOptions { marker_name: "{rating}|{label}|{note}".into(), ..Default::default() };
    let out = tmp.join("sel.json");
    s.export_to(&settings, SortOrder::Timeline, &opts, clippro_interchange::Format::Json, &out).unwrap();
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains("★★★★★|purple|Hero shot"), "{text}");
    // Clearing every field removes the annotation.
    s.set_annotation(tripod, seg.start_us, seg.end_us, 0, None, "").unwrap();
    assert!(s.annotations().unwrap().is_empty());
    s.close().unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

/// PLAN P10.1/P10.3: audio is analysed with the clip and drives scoring — Audio-First excludes
/// the silence in `audio_mix` and keeps its clean tone; inspector sees the Audio sub-metrics.
#[test]
fn audio_drives_scoring() {
    let Some(dir) = corpus() else { return };
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-audio-test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let s = Session::create(&tmp.join("p.clippro"), "Audio", SessionOptions { cache_path: None, ..opts(&tmp) }).unwrap();
    s.import(&[dir.join("audio_mix.mp4")]).unwrap();
    s.queue_analysis().unwrap();
    s.wait(|_| {});
    let clip = s.clip_statuses().unwrap()[0].clip;
    let input = s.clip_inputs().unwrap().into_iter().find(|c| c.id == clip).unwrap();
    assert!(input.extra.contains_key(&clippro_score::SubMetric::Loudness), "audio columns reach scoring");
    let mut settings = Settings::from_preset(&builtin("Audio-First").unwrap()).unwrap();
    // The corpus clip is short (2 s tone, 1.5 s speech) over a frozen picture: allow short
    // selects and ignore the video Duplicate flag so the audio decides.
    settings.len_min_s = 1.0;
    settings.tiers[clippro_types::FlagKind::Duplicate as usize] = clippro_types::Tier::Neutral;
    let sel = s.score(&settings).unwrap();
    let segs = &sel.clips[0].segments;
    eprintln!("Audio-First selects: {:?}", segs.iter().map(|g| (g.start_us / 1000, g.end_us / 1000)).collect::<Vec<_>>());
    assert!(!segs.is_empty(), "the tone or speech is selectable");
    assert!(segs.iter().all(|g| g.end_us <= 3_200_000 || g.start_us >= 4_800_000), "no select inside the silence (3–5 s)");
    assert!(segs.iter().any(|g| g.start_us <= 9_000_000 && g.end_us >= 9_500_000), "the speech is selected");
    assert!(segs.iter().all(|g| !(g.start_us < 2_800_000 && g.end_us > 2_200_000)), "the clipped tone is not selected");
    let k = input.ms.time_us.iter().position(|t| *t >= 4_000_000).unwrap();
    let ins = s.inspect(clip, &settings, k).unwrap();
    assert!(ins.breakdown.flags.contains(&clippro_types::FlagKind::Silence), "{:?}", ins.breakdown.flags);
    assert!(ins.reasons.iter().any(|r| r.contains("Silence")), "{:?}", ins.reasons);
    s.close().unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

/// PLAN P11.3/P11.4: a timeline exported from the selects comes back through FCP7 XML, relinks
/// to the project, scores like the selects it came from, and exports back with score markers.
#[test]
fn imported_timeline_review_and_export_back() {
    use clippro_core::review::{export_back, ReviewOptions};
    let Some(dir) = corpus() else { return };
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-review-test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let s = Session::create(&tmp.join("p.clippro"), "Review", SessionOptions { cache_path: None, ..opts(&tmp) }).unwrap();
    s.import(&[dir.join("tripod_static.mp4"), dir.join("handheld_shake.mp4"), dir.join("blur_ramp.mp4")]).unwrap();
    s.queue_analysis().unwrap();
    s.wait(|_| {});
    let settings = Settings::from_preset(&builtin("Cleanup").unwrap()).unwrap();
    let ex = clippro_interchange::ExportOptions { handles: false, ..Default::default() };
    let tl = s.export_timeline(&settings, SortOrder::Timeline, &ex).unwrap();
    let xml = clippro_interchange::Format::Xmeml.render(&tl);
    let back = clippro_interchange::import::import(&xml, clippro_types::FrameRate::new(25, 1)).unwrap();
    let (rv, clips) = s.review_timeline(back, &settings, &ReviewOptions::default()).unwrap();
    assert!(rv.relink.missing.is_empty(), "{:?}", rv.relink);
    assert!(!rv.items.is_empty());
    let sel = s.score(&settings).unwrap();
    for (it, r) in rv.items.iter().zip(sel.sorted(SortOrder::Timeline)) {
        assert_eq!(it.clip, Some(r.clip));
        let mean = it.mean.unwrap();
        assert!((mean - r.segment.mean).abs() < 0.05, "{} vs {}", mean, r.segment.mean);
        assert!(!it.bars.is_empty());
    }
    // A deliberately poor item: the blurriest stretch of blur_ramp, as an editor might cut it.
    let blur = clips.iter().find(|c| c.name == "blur_ramp").unwrap();
    let worst = (0..blur.scores.score.len()).min_by(|a, b| blur.scores.score[*a].total_cmp(&blur.scores.score[*b])).unwrap();
    let t = blur.ms.time_us[worst];
    let mut cut = rv.timeline.clone();
    let mi = cut.media.iter().position(|m| m.path == blur.media.path).unwrap_or_else(|| {
        cut.media.push(blur.media.clone());
        cut.media.len() - 1
    });
    let f = cut.media[mi].rate.us_to_frames(t).saturating_sub(5).max(0);
    cut.video[0].items.push(clippro_interchange::model::Item::Clip(clippro_interchange::model::ClipItem {
        media: mi,
        name: "blur_ramp".into(),
        src_in: f,
        src_out: f + 10,
        markers: vec![],
        select_index: None,
        score: None,
    }));
    let (rv2, clips2) = s.review_timeline(cut, &settings, &ReviewOptions::default()).unwrap();
    let last = rv2.items.last().unwrap();
    assert!(last.weak, "{last:?}");
    let out = export_back(&rv2, &clips2, &[], true);
    let marks: Vec<_> = out.video[0].items.iter().filter_map(|i| if let clippro_interchange::model::Item::Clip(c) = i { c.markers.iter().find(|m| m.name.starts_with("ClipPro ")) } else { None }).collect();
    assert_eq!(marks.len(), rv2.items.len());
    assert_eq!(marks.last().unwrap().color, clippro_interchange::MarkerColor::Red);
    s.close().unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

/// PLAN P11.5: `multicam_b` starts 3.48 s into `multicam_a` — audio sync finds it within a
/// frame with a confident PSR; groups persist; the stacked timeline and FCPXML mc-clip follow.
#[test]
fn multicam_sync_groups() {
    use clippro_core::multicam::SyncMethod;
    let Some(dir) = corpus() else { return };
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-multicam-test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let s = Session::create(&tmp.join("p.clippro"), "Multicam", SessionOptions { cache_path: None, ..opts(&tmp) }).unwrap();
    s.import(&[dir.join("multicam_a.mp4"), dir.join("multicam_b.mp4"), dir.join("prores_hq.mov")]).unwrap();
    let st = s.clip_statuses().unwrap();
    let id = |n: &str| st.iter().find(|c| c.name == n).unwrap().clip;
    let (a, b, p) = (id("multicam_a"), id("multicam_b"), id("prores_hq"));
    let g = s.sync_clips("Scene 1", &[b, a], SyncMethod::Audio, &Default::default()).unwrap();
    assert!(g.warnings.is_empty(), "{:?}", g.warnings);
    // Equal lengths: the first clip is the reference, so b sits at 0 and a at −3.48 s.
    let off = |c| g.members.iter().find(|m| m.clip == c).unwrap();
    let rel = off(b).offset_us.unwrap() - off(a).offset_us.unwrap();
    assert!((rel - 3_480_000).abs() <= 40_000, "offset {rel} µs, want 3.48 s ± 1 frame");
    let psr = off(if g.reference == a { b } else { a }).psr.unwrap();
    assert!(psr >= 6.0, "PSR {psr}");
    eprintln!("multicam_a/b: offset {:.4} s, PSR {psr:.1}", rel as f64 / 1e6);
    // Persisted and listed.
    let gid = s.save_sync_group(&g).unwrap();
    let back = s.sync_groups().unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!((back[0].id, back[0].members.len()), (Some(gid), 2));
    // Stacked tracks: the reference on V1, the other angle 3.48 s (87 frames) apart.
    let tl = s.multicam_timeline(&back[0]).unwrap();
    assert_eq!(tl.video.len(), 2);
    let gaps: Vec<i64> = tl.video.iter().map(|t| if let clippro_interchange::model::Item::Gap(n) = t.items[0] { n } else { 0 }).collect();
    assert_eq!(gaps.iter().sum::<i64>(), 87, "{gaps:?}");
    let x = clippro_interchange::fcpxml::export_multicam(&tl);
    clippro_interchange::validate::well_formed(&x).unwrap();
    assert_eq!(x.matches("<mc-angle ").count(), 2);
    s.delete_sync_group(gid).unwrap();
    assert!(s.sync_groups().unwrap().is_empty());
    // Timecode mode: prores_hq carries 01:00:00:00; multicam_a has none.
    let t = s.sync_clips("TC", &[a, p], SyncMethod::Timecode, &Default::default()).unwrap();
    assert_eq!(t.reference, p);
    assert_eq!(t.members.iter().find(|m| m.clip == p).unwrap().offset_us, Some(0));
    assert_eq!(t.members.iter().find(|m| m.clip == a).unwrap().offset_us, None);
    assert_eq!(t.warnings.len(), 1, "{:?}", t.warnings);
    assert!(s.sync_clips("one", &[a], SyncMethod::Audio, &Default::default()).is_err());
    s.close().unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}
