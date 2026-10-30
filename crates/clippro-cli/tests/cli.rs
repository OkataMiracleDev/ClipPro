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

    // Every export format writes a file; XML formats are well formed.
    for fmt in ["xmeml", "fcpxml", "otio", "edl", "ale", "avid-markers", "csv", "json", "html", "contact-sheet"] {
        let out_path = tmp.join(format!("selects.{fmt}"));
        let out = Command::new(exe())
            .arg("export")
            .arg(&proj)
            .args(["--format", fmt, "--preset", "Cleanup", "--layout", "with-rejects"])
            .arg("--out")
            .arg(&out_path)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{fmt}: {}", String::from_utf8_lossy(&out.stderr));
        let text = std::fs::read_to_string(&out_path).unwrap();
        if fmt == "avid-markers" {
            assert!(text.lines().any(|l| l.starts_with("ClipPro\t")), "no markers");
        } else {
            assert!(text.contains("tripod_static"), "{fmt} lacks the select");
        }
        if fmt == "xmeml" || fmt == "fcpxml" {
            clippro_interchange::validate::well_formed(&text).unwrap_or_else(|e| panic!("{fmt}: {e}"));
        }
        if fmt == "otio" || fmt == "json" {
            serde_json::from_str::<serde_json::Value>(&text).unwrap();
        }
    }

    // AAF (binary): a compound file that our reader takes back.
    let aaf = tmp.join("selects.aaf");
    let out = Command::new(exe()).arg("export").arg(&proj).args(["--format", "aaf", "--preset", "Cleanup", "--out"]).arg(&aaf).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "aaf: {}", String::from_utf8_lossy(&out.stderr));
    let back = clippro_interchange::aaf::import(&std::fs::read(&aaf).unwrap()).unwrap();
    assert!(back.media.iter().any(|m| m.name == "tripod_static"));

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

    // P13.4: stringout export and rendered selects.
    let so = tmp.join("stringout.xml");
    let out = Command::new(exe())
        .arg("export")
        .arg(&proj)
        .args(["--format", "xmeml", "--preset", "Cleanup", "--layout", "stringout", "--group-by", "clip", "--separator", "slug", "--separator-length", "1s", "--out"])
        .arg(&so)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(std::fs::read_to_string(&so).unwrap().contains("<effectid>Slug</effectid>"));
    let bad = Command::new(exe()).arg("export").arg(&proj).args(["--format", "xmeml", "--layout", "stringout", "--group-by", "mood", "--out"]).arg(&so).output().unwrap();
    assert_eq!(bad.status.code(), Some(1));
    if clippro_ingest::ffmpeg::ffmpeg_exe().is_some() {
        let out = Command::new(exe())
            .args(["--json", "render"])
            .arg(&proj)
            .args(["--preset", "Cleanup", "--copy", "--nearest-keyframe", "--out"])
            .arg(tmp.join("renders"))
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let r = v.as_array().unwrap();
        assert!(!r.is_empty() && r.iter().all(|o| o["copied"] == true && Path::new(o["path"].as_str().unwrap()).exists()), "{v}");
    }
    std::fs::remove_dir_all(&tmp).unwrap();
}

/// P13.4: offload (exit 0, JSON report, usage error without a destination) and the self-test.
#[test]
fn offload_and_selftest() {
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-cli-offload");
    let _ = std::fs::remove_dir_all(&tmp);
    let card = tmp.join("CARD01");
    std::fs::create_dir_all(card.join("CLIP")).unwrap();
    std::fs::write(card.join("CLIP/A.mxf"), vec![7u8; 100_000]).unwrap();
    std::fs::write(card.join("index.xml"), b"<i/>").unwrap();
    let out = Command::new(exe())
        .args(["--json", "offload"])
        .arg(&card)
        .arg("--dest")
        .arg(tmp.join("D1"))
        .arg("--dest")
        .arg(tmp.join("D2"))
        .args(["--hash", "md5"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!((v["files"].as_u64(), v["copied"].as_u64()), (Some(2), Some(2)), "{v}");
    assert!(tmp.join("D2/CARD01/CLIP/A.mxf").exists());
    let mhl = std::fs::read_to_string(v["manifests"][0].as_str().unwrap()).unwrap();
    assert!(mhl.contains("<md5 action=\"original\""));
    let none = Command::new(exe()).arg("offload").arg(&card).output().unwrap();
    assert_eq!(none.status.code(), Some(1), "--dest is required");
    let bad = Command::new(exe()).arg("offload").arg(&card).arg("--dest").arg(tmp.join("D3")).args(["--hash", "crc"]).output().unwrap();
    assert_eq!(bad.status.code(), Some(1));
    if clippro_ingest::ffmpeg::ffmpeg_exe().is_some() {
        let out = Command::new(exe()).args(["--json", "selftest", "--dir"]).arg(tmp.join("selftest")).output().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
        assert_eq!(out.status.code(), Some(0), "{v:#}\n{}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(v["passed"], v["total"]);
        assert!(v["total"].as_u64().unwrap() >= 9);
        assert!(!tmp.join("selftest").exists(), "selftest cleans up without --keep");
    }
    std::fs::remove_dir_all(&tmp).unwrap();
}
