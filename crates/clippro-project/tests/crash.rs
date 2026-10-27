//! P3.1: a process killed mid-write leaves a recoverable project with every committed write.

use clippro_project::{Db, NewMedia, RunParams};
use clippro_types::{GroupMask, SampleRate};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn params() -> RunParams {
    RunParams { engine_version: 2, calibration_version: 2, rate: SampleRate::Standard, pair_sampling: true, groups: GroupMask::ALL_COMPUTE }
}

fn child(path: &Path) {
    let (db, _) = Db::open(path).unwrap();
    let m = db.media_all().unwrap()[0].id;
    let run = db.run_for(m, &params()).unwrap();
    let out = std::io::stdout();
    let payload = vec![7u8; 4096];
    for i in 0.. {
        db.save_chunk(run.id, 0, i, i as usize * 256, 256, &payload).unwrap();
        let mut o = out.lock();
        writeln!(o, "{i}").unwrap();
        o.flush().unwrap();
    }
}

#[test]
fn killed_writer_recovers() {
    if let Ok(p) = std::env::var("CLIPPRO_CRASH_CHILD") {
        child(Path::new(&p));
        return;
    }
    let dir: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-project-tests/crash");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("crash.clippro");
    let db = Db::create(&path, "Crash").unwrap();
    db.add_media(&NewMedia {
        path: Path::new("D:/x.mp4"),
        fingerprint: "fp",
        content_fingerprint: "fp",
        size: 1,
        probe_json: "{}",
        duration_us: 1,
        width: 1,
        height: 1,
        fps: (25, 1),
        reel: None,
        camera: None,
        start_tc: None,
    })
    .unwrap();
    db.close().unwrap();

    let mut c = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "killed_writer_recovers", "--nocapture", "--test-threads=1"])
        .env("CLIPPRO_CRASH_CHILD", &path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut last_committed: i64 = -1;
    let reader = BufReader::new(c.stdout.take().unwrap());
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if let Ok(i) = line.trim().parse::<i64>() {
            last_committed = i;
            if i >= 200 {
                c.kill().unwrap();
                break;
            }
        }
    }
    let _ = c.wait();
    assert!(last_committed >= 200, "child didn't write ({last_committed})");

    let (db, report) = Db::open(&path).unwrap();
    assert!(report.recovered, "unclean shutdown not detected");
    assert!(db.integrity_ok().unwrap());
    let m = db.media_all().unwrap()[0].id;
    let run = db.run_for(m, &params()).unwrap();
    let chunks = db.chunks(run.id).unwrap();
    assert!(chunks.len() as i64 > last_committed, "{} chunks, child reported {last_committed}", chunks.len());
    for (i, ch) in chunks.iter().enumerate() {
        assert_eq!(ch.chunk_index as usize, i);
        assert_eq!(ch.data.len(), 4096);
    }
    db.close().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let _ = std::fs::remove_dir(dir.parent().unwrap());
}
