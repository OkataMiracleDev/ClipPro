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
