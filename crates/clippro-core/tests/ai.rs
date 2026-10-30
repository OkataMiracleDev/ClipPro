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
