//! P3.5 / P4.6: CLI integration (analyze, score, info, presets, exit codes, JSON).
//! Needs the corpus (`clippro-testgen --out testdata/gen --mini`); skips when missing.

use std::path::{Path, PathBuf};
use std::process::Command;

fn exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_clippro-cli"))
}

fn corpus() -> Option<PathBuf> {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/gen");
    d.join("tripod_static.mp4").exists().then_some(d)
}

#[test]
fn analyze_score_info_and_exit_codes() {
    let Some(dir) = corpus() else {
        eprintln!("corpus missing: skipping");
        return;
    };
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-cli-test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let proj = tmp.join("t.clippro");

    let out = Command::new(exe())
        .args(["--json", "analyze"])
        .arg(dir.join("tripod_static.mp4"))
        .arg(dir.join("solid_black.mp4"))
        .arg("--project")
        .arg(&proj)
        .arg("--no-cache")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["imported"], 2);
    assert!(v["clips"].as_array().unwrap().iter().all(|c| c["state"] == "done"));

    let out = Command::new(exe()).args(["--json", "score"]).arg(&proj).args(["--preset", "Cleanup", "--set", "score=20,100"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let sel = v["selects"].as_array().unwrap();
    assert!(sel.iter().any(|s| s["name"] == "tripod_static"));
    assert!(!sel.iter().any(|s| s["name"] == "solid_black"));

    // Usage errors exit 1; media errors exit 2.
    let bad = Command::new(exe()).arg("score").arg(&proj).args(["--preset", "nope"]).output().unwrap();
    assert_eq!(bad.status.code(), Some(1));
    let bad = Command::new(exe()).arg("score").arg(&proj).args(["--set", "bogus=1"]).output().unwrap();
    assert_eq!(bad.status.code(), Some(1));
    let junk = tmp.join("junk.mp4");
    std::fs::write(&junk, b"junk").unwrap();
    let bad = Command::new(exe()).arg("info").arg(&junk).output().unwrap();
    assert_eq!(bad.status.code(), Some(2));
    let ok = Command::new(exe()).args(["--json", "info"]).arg(dir.join("prores_hq.mov")).output().unwrap();
    assert_eq!(ok.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&ok.stdout).unwrap();
    assert_eq!(v["video"]["codec"], "prores");
    let p = Command::new(exe()).args(["--json", "presets"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&p.stdout).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 19);
    std::fs::remove_dir_all(&tmp).unwrap();
}
