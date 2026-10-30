//! P12 end-to-end: a talking-head clip (a public-domain portrait over the TTS speech fixture)
//! through faces, CLIP, Whisper and speaker turns, then People, search and Content scoring.
//! Needs ffmpeg.exe and the AI models (`CLIPPRO_MODELS`); skips when either is missing.

use clippro_core::ai::AiOptions;
use clippro_core::{Session, SessionOptions};
use clippro_score::{builtin, Settings, SubMetric};
use std::path::Path;
use std::sync::atomic::AtomicBool;

#[test]
fn ai_pass_people_search_and_content() {
    let Some(ff) = clippro_ingest::ffmpeg::ffmpeg_exe() else {
        eprintln!("ffmpeg missing: skipping");
        return;
    };
    let needed = ["yunet-2023mar", "sface-2021dec", "clip-b32-vision", "clip-b32-text", "clip-b32-tokenizer", "whisper-base", "ecapa-voxceleb-512"];
    if let Some(m) = needed.iter().find(|m| clippro_ai::models::installed_path(m).is_none()) {
        eprintln!("model {m} missing: skipping");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tmp = root.join("target/tmp-ai-test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("media")).unwrap();
    let clip = tmp.join("media/interview.mp4");
    let st = std::process::Command::new(ff)
        .args(["-v", "error", "-loop", "1", "-framerate", "25", "-i"])
        .arg(root.join("testdata/faces/einstein.jpg"))
        .arg("-i")
        .arg(root.join("testdata/speech/fox.wav"))
        .args(["-vf", "scale=640:-2,format=yuv420p", "-c:v", "mpeg4", "-q:v", "3", "-c:a", "aac", "-shortest"])
        .arg(&clip)
        .status()
        .unwrap();
    assert!(st.success());

    let s = Session::create(&tmp.join("p.clippro"), "AI", SessionOptions::default()).unwrap();
    s.import(&[tmp.join("media")]).unwrap();
    s.queue_analysis().unwrap();
    s.wait(|_| {});
    let opts = AiOptions { faces: true, clip: true, transcripts: true, speakers: true, whisper: "whisper-base".into(), language: None, deterministic: true };
    let pending = s.ai_pending(&opts).unwrap();
    assert_eq!(pending.len(), 1);
    s.run_ai(pending[0], &opts, &AtomicBool::new(false), |_, _| {}).unwrap();
    assert!(s.ai_pending(&opts).unwrap().is_empty(), "everything stored");

    let people = s.people().unwrap();
    assert_eq!(people.len(), 1, "{people:?}");
    assert_eq!(people[0].clips, vec![pending[0]]);
    s.rename_person(people[0].id, "Albert").unwrap();
    s.recluster_people().unwrap();
    assert_eq!(s.people().unwrap()[0].name, "Albert", "names survive re-clustering");

    let t = s.transcript(pending[0]).unwrap().expect("transcript");
    assert_eq!(t.language, "en");
    assert!(t.text().to_lowercase().contains("fox"), "{}", t.text());
    let hits = s.search_transcripts("brown fox").unwrap();
    assert_eq!(hits.len(), 1);
    let vis = s.search_visual("a black and white photo of a man", true, 5).unwrap();
    assert!(!vis.is_empty());
    let summary = &s.clip_ai().unwrap()[0];
    assert!(summary.faces_done && summary.clip_done && summary.transcript);
    assert_eq!(summary.people.len(), 1);
    assert!(summary.shot.is_some());

    // Content sub-metrics reach the engine.
    let inputs = s.clip_inputs_for(&["close_up".into()]).unwrap();
    for k in [SubMetric::FaceFocus, SubMetric::FaceSize, SubMetric::Aesthetic, SubMetric::ShotTypeMatch] {
        let v = inputs[0].extra.get(&k).unwrap_or_else(|| panic!("{k:?} missing"));
        assert!(v.iter().any(|x| x.is_finite()), "{k:?}");
    }
    assert!(inputs[0].snaps.is_some());
    let mut settings = Settings::from_preset(&builtin("Interview").unwrap()).unwrap();
    settings.snap_to_sentences = true;
    settings.best_take_only = true;
    s.score(&settings).unwrap();
    s.close().unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

/// P12.6: the three takes of one shot group together and take 2 (the steady one) ranks best.
#[test]
fn takes_group_and_best_take() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let gen = root.join("testdata/gen");
    if !gen.join("takes_1.mp4").exists() || clippro_ai::models::installed_path("clip-b32-vision").is_none() {
        eprintln!("corpus or CLIP missing: skipping");
        return;
    }
    let tmp = root.join("target/tmp-ai-takes");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("media")).unwrap();
    for i in 1..=3 {
        std::fs::copy(gen.join(format!("takes_{i}.mp4")), tmp.join(format!("media/takes_{i}.mp4"))).unwrap();
    }
    let s = Session::create(&tmp.join("p.clippro"), "Takes", SessionOptions::default()).unwrap();
    s.import(&[tmp.join("media")]).unwrap();
    s.queue_analysis().unwrap();
    s.wait(|_| {});
    let opts = AiOptions { clip: true, deterministic: true, ..AiOptions::default() };
    for c in s.ai_pending(&opts).unwrap() {
        s.run_ai(c, &opts, &AtomicBool::new(false), |_, _| {}).unwrap();
    }
    let names = s.clip_names().unwrap();
    // Cleanup keeps all three takes; one select each.
    let mut settings = Settings::from_preset(&builtin("Cleanup").unwrap()).unwrap();
    settings.max_per_clip = Some(1);
    let sel = s.score(&settings).unwrap();
    let groups = s.take_groups(&sel).unwrap();
    let clips_in_group = |g: usize| groups.iter().filter(|x| x.1 == g).map(|x| names[&sel.clips[x.0 .0].clip].0.clone()).collect::<std::collections::BTreeSet<_>>();
    assert_eq!(groups.len(), 3, "{groups:?}");
    assert_eq!(clips_in_group(groups[0].1).len(), 3, "all three takes in one group: {groups:?}");
    settings.best_take_only = true;
    let best = s.score(&settings).unwrap();
    let kept: Vec<String> = best.clips.iter().filter(|c| !c.segments.is_empty()).map(|c| names[&c.clip].0.clone()).collect();
    assert_eq!(kept, vec!["takes_2".to_string()]);
    assert_eq!(best.summary.segment_count, 1);
    s.close().unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

/// P12.8: core analysis throughput drops at most 15% while the AI queue runs (it yields to
/// analysis). Timing-sensitive, so run on demand: `cargo test --release -p clippro-core --test ai
/// -- --ignored throughput`.
#[test]
#[ignore]
fn throughput_with_ai_running() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let gen = root.join("testdata/gen");
    if !gen.join("takes_1.mp4").exists() || clippro_ai::models::installed_path("clip-b32-vision").is_none() {
        eprintln!("corpus or CLIP missing: skipping");
        return;
    }
    let tmp = root.join("target/tmp-ai-throughput");
    let _ = std::fs::remove_dir_all(&tmp);
    // Most of the corpus (about 30 clips), so the batch takes long enough to measure.
    let batch: Vec<String> = std::fs::read_dir(&gen)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .filter(|f| !f.starts_with("takes_") && !f.starts_with("long_clip") && !f.ends_with(".json") && !f.contains("Proxy"))
        .collect();
    let opts = || SessionOptions { cache_path: None, ..SessionOptions::default() };
    let analyse = |s: &Session| {
        let t = std::time::Instant::now();
        s.queue_analysis().unwrap();
        s.wait(|_| {});
        t.elapsed().as_secs_f64()
    };
    let setup = |name: &str| {
        let d = tmp.join(name);
        std::fs::create_dir_all(d.join("ai")).unwrap();
        std::fs::create_dir_all(d.join("batch")).unwrap();
        for i in 1..=3 {
            std::fs::copy(gen.join(format!("takes_{i}.mp4")), d.join(format!("ai/takes_{i}.mp4"))).unwrap();
        }
        for f in &batch {
            if gen.join(f).is_file() {
                std::fs::copy(gen.join(f), d.join("batch").join(f)).unwrap();
            }
        }
        let s = Session::create(&d.join("p.clippro"), name, opts()).unwrap();
        s.import(&[d.join("ai")]).unwrap();
        analyse(&s);
        s.import(&[d.join("batch")]).unwrap();
        (s, d)
    };
    // Baseline: the batch alone.
    let (s, _) = setup("base");
    let base = analyse(&s);
    s.close().unwrap();
    // The same batch while the AI queue works on the takes.
    let (s, _) = setup("ai");
    let s = std::sync::Arc::new(s);
    let ai_opts = AiOptions { faces: true, clip: true, ..AiOptions::default() };
    let mut pending = s.ai_pending(&ai_opts).unwrap();
    // Steady state: the models are loaded and the queue is mid-way when new footage arrives.
    s.run_ai(pending.remove(0), &ai_opts, &AtomicBool::new(false), |_, _| {}).unwrap();
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let worker = {
        let (s, stop) = (s.clone(), stop.clone());
        std::thread::spawn(move || {
            for c in pending {
                let _ = s.run_ai(c, &ai_opts, &stop, |_, _| {});
            }
        })
    };
    std::thread::sleep(std::time::Duration::from_millis(300));
    let with_ai = analyse(&s);
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    worker.join().unwrap();
    let loss = 1.0 - base / with_ai;
    eprintln!("analysis alone {base:.2}s, with AI {with_ai:.2}s, throughput drop {:.1}%", loss * 100.0);
    assert!(loss <= 0.15, "throughput dropped {:.1}%", loss * 100.0);
    drop(s);
    std::fs::remove_dir_all(&tmp).unwrap();
}
