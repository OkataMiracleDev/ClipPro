//! PLAN P7b: ClipPro's AAF is read back by pyaaf2, an independent implementation, with the same
//! edit, media, timecode and markers; and P11.2: our reader reads a pyaaf2-authored file.
//! Needs Python with pyaaf2 in `third_party/pyaaf2` (see docs/BLOCKERS.md B-2); skips otherwise.

use clippro_interchange::model::{ClipItem, Item, Marker, MarkerColor, MediaRef, Timeline, Track};
use clippro_types::FrameRate;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn pyaaf2() -> bool {
    root().join("third_party/pyaaf2/aaf2").exists() && Command::new("python").arg("--version").output().is_ok_and(|o| o.status.success())
}

fn verify(path: &Path) -> Value {
    let out = Command::new("python")
        .env("PYTHONPATH", root().join("third_party/pyaaf2"))
        .arg(root().join("scripts/aaf/verify.py"))
        .arg(path)
        .output()
        .expect("python");
    assert!(out.status.success(), "pyaaf2 couldn't read {}:\n{}", path.display(), String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).expect("json")
}

fn media(name: &str, start: i64, rate: FrameRate) -> MediaRef {
    MediaRef { id: 1, name: name.into(), path: PathBuf::from(format!("D:/Footage/Day 1/{name}.mov")), rate, duration_frames: 500, start_tc_frames: start, drop_frame: false, width: 1920, height: 1080, audio_channels: 2, reel: None }
}

fn clip(m: usize, a: i64, b: i64, markers: Vec<Marker>) -> Item {
    Item::Clip(ClipItem { media: m, name: format!("c{m}"), src_in: a, src_out: b, markers, select_index: None, score: None })
}

#[test]
fn pyaaf2_reads_our_aaf() {
    if !pyaaf2() {
        eprintln!("pyaaf2 missing: skipping");
        return;
    }
    let r = FrameRate::new(25, 1);
    let mk = Marker { at: 10, duration: 20, name: "Select 1 · 82".into(), comment: "Shaky".into(), color: MarkerColor::Blue };
    let tl = Timeline {
        name: "Day 1 selects".into(),
        rate: r,
        drop_frame: false,
        start_tc_frames: 90_000,
        width: 1920,
        height: 1080,
        media: vec![media("A001C003", 900_000, r), media("A002C004", 901_000, r)],
        video: vec![
            Track { name: "V1".into(), items: vec![clip(0, 6, 119, vec![mk]), Item::Gap(25), clip(1, 0, 144, vec![])] },
            Track { name: "V2".into(), items: vec![Item::Gap(40), clip(1, 200, 250, vec![])] },
        ],
        with_audio: true,
    };
    let dir = root().join("target/tmp-aaf-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("selects.aaf");
    std::fs::write(&path, clippro_interchange::aaf::export(&tl).unwrap()).unwrap();
    let v = verify(&path);
    assert_eq!(v["name"], "Day 1 selects");
    assert_eq!(v["product"], "ClipPro");
    assert_eq!(v["mobs"], 5, "composition + 2 × (master, source)");
    assert_eq!(v["timecode"]["start"], 90_000);
    assert_eq!(v["timecode"]["fps"], 25);
    let t1 = &v["tracks"][0];
    assert_eq!((t1["slot"].as_u64(), t1["length"].as_i64()), (Some(1), Some(113 + 25 + 144)));
    let items = t1["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!((items[0]["kind"].as_str(), items[0]["start"].as_i64(), items[0]["length"].as_i64()), (Some("clip"), Some(6), Some(113)));
    assert_eq!(items[0]["url"], "file:///D:/Footage/Day%201/A001C003.mov");
    assert_eq!(items[0]["tc"], 900_000);
    assert_eq!((items[1]["kind"].as_str(), items[1]["length"].as_i64()), (Some("filler"), Some(25)));
    assert_eq!((items[2]["mob"].as_str(), items[2]["tc"].as_i64()), (Some("A002C004"), Some(901_000)));
    let t2 = &v["tracks"][1];
    assert_eq!(t2["items"][1]["start"], 200);
    let m = &v["markers"][0];
    assert_eq!((m["position"].as_i64(), m["length"].as_i64()), (Some(10), Some(20)));
    assert_eq!(m["comment"], "Select 1 · 82 – Shaky");
    assert_eq!(m["color"], serde_json::json!([0, 0, 65535]));
    assert_eq!(m["described"], serde_json::json!([1]));
    // And our reader agrees with what we wrote.
    let back = clippro_interchange::aaf::import(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(clippro_interchange::import::edit_signature(&back), clippro_interchange::import::edit_signature(&tl));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// P11.2: a file authored by pyaaf2 (scripts/aaf/make_reference.py) reads into the same edit.
#[test]
fn reads_a_pyaaf2_authored_file() {
    let p = root().join("testdata/interchange/pyaaf2_reference.aaf");
    let Ok(bytes) = std::fs::read(&p) else {
        eprintln!("fixture missing: skipping");
        return;
    };
    let t = clippro_interchange::aaf::import(&bytes).unwrap();
    assert_eq!(t.name, "ClipPro Selects");
    assert_eq!((t.rate, t.start_tc_frames), (FrameRate::new(25, 1), 90_000));
    let clips: Vec<(i64, i64, i64)> = clippro_interchange::import::edit_signature(&t).iter().map(|c| (c.2, c.3, c.4)).collect();
    assert_eq!(clips, vec![(6, 119, 0), (6, 119, 138)]);
    assert_eq!(t.media[0].path, PathBuf::from("D:/Footage/A001C003.mov"));
    assert_eq!((t.media[0].start_tc_frames, t.media[1].start_tc_frames), (900_000, 901_000));
    let Item::Clip(c) = &t.video[0].items[0] else { panic!() };
    assert_eq!((c.markers[0].at, c.markers[0].duration, c.markers[0].color), (19, 75, MarkerColor::Green));
    assert_eq!(c.markers[0].name, "Select 1 - 82");
}
