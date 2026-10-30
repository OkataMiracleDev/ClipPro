//! P13.2/P13.3 against the corpus: proxies keep timecode, reel and length; rendered selects
//! last as long as the select (±1 frame), stream-copied from keyframes when they can be.
//! Needs the corpus and `ffmpeg.exe` on PATH (`source scripts/env.sh`); skips otherwise.

use clippro_ingest::proxy::{make_proxy, ProxyCodec, ProxyOptions, ProxySource};
use clippro_ingest::render::{render, RenderCodec, RenderItem, RenderOptions};
use clippro_types::FrameRate;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

fn corpus() -> Option<PathBuf> {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/gen");
    if d.join("prores_hq.mov").exists() && clippro_ingest::ffmpeg::ffmpeg_exe().is_some() {
        Some(d)
    } else {
        eprintln!("corpus or ffmpeg missing: skipping");
        None
    }
}

fn tmp(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target").join(format!("tmp-encode-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn proxy_keeps_timecode_reel_and_length() {
    let Some(dir) = corpus() else { return };
    let t = tmp("proxy");
    let src = dir.join("prores_hq.mov");
    let info = clippro_media::probe(&src).unwrap();
    let opts = ProxyOptions { codec: ProxyCodec::ProresProxy, max_height: 360, out_dir: Some(t.clone()) };
    let mut last = 0.0;
    let out = make_proxy(
        &ProxySource { path: src.clone(), duration_us: info.duration_us, start_tc: info.start_timecode.clone(), reel: info.reel.clone() },
        &opts,
        &AtomicBool::new(false),
        |p| last = p,
    )
    .unwrap();
    assert_eq!(out, t.join("prores_hq_Proxy.mov"));
    assert_eq!(last, 1.0);
    let p = clippro_media::probe(&out).unwrap();
    let v = p.video.as_ref().unwrap();
    assert_eq!((v.height, v.codec.as_str(), v.profile.as_str()), (360, "prores", "Proxy"), "{v:?}");
    assert_eq!(v.frames, info.video.as_ref().unwrap().frames, "every frame kept");
    assert_eq!(p.start_timecode.as_deref(), Some("01:00:00:00"));
    assert_eq!(p.reel.as_deref(), Some("A001C003"));
    assert!((p.duration_us - info.duration_us).abs() <= 40_000, "{} vs {}", p.duration_us, info.duration_us);
    assert!(!t.join("prores_hq_Proxy.partial.mov").exists());
    std::fs::remove_dir_all(&t).unwrap();
}

#[test]
fn rendered_selects_match_their_length() {
    let Some(dir) = corpus() else { return };
    let t = tmp("render");
    let src = dir.join("tripod_static.mp4");
    let keys = clippro_media::keyframes(&src).unwrap();
    let k = keys[1];
    let item = |a: i64, b: i64| RenderItem { src: src.clone(), start_us: a, end_us: b, rate: FrameRate::new(25, 1), start_tc_frames: Some(90_000), drop_frame: false, reel: None };
    let opts = |copy: bool, nearest: bool| RenderOptions { stream_copy: copy, allow_nearest_keyframe: nearest, codec: RenderCodec::Prores422, out_dir: t.clone(), template: String::new() };
    // Length by frame count: a stream-copied H.264 track keeps its B-frame delay as an edit,
    // which pads the container duration by two frames without adding any.
    let dur = |p: &Path| {
        let v = clippro_media::probe(p).unwrap().video.unwrap();
        v.frames.map(|n| n as i64 * 40_000).unwrap_or(0)
    };
    let no = AtomicBool::new(false);
    // In point on a keyframe: stream copy, exact.
    let a = render(&item(k, k + 2_000_000), &keys, &opts(true, false), &t.join("a.mov"), &no, |_| {}).unwrap();
    assert!(a.copied && a.start_us == k);
    // Copies end on a decode-order packet: never short, at most the B-frame delay (2) long.
    assert!((2_000_000..=2_080_000).contains(&dur(&a.path)), "copy {}", dur(&a.path));
    // Off a keyframe: re-encoded to the exact span…
    let mid = k + 520_000; // 13 frames in
    let b = render(&item(mid, mid + 1_200_000), &keys, &opts(true, false), &t.join("b.mov"), &no, |_| {}).unwrap();
    assert!(!b.copied);
    assert!((dur(&b.path) - 1_200_000).abs() <= 40_000, "encode {}", dur(&b.path));
    assert_eq!(clippro_media::probe(&b.path).unwrap().video.unwrap().codec, "prores");
    let tc = clippro_media::probe(&b.path).unwrap().start_timecode.unwrap();
    let want = clippro_types::Timecode::from_frames(90_000 + FrameRate::new(25, 1).us_to_frames_round(mid), FrameRate::new(25, 1), false).to_string();
    assert_eq!(tc, want, "source timecode at the in point");
    // …or copied from the keyframe before it when allowed.
    let c = render(&item(mid, mid + 1_200_000), &keys, &opts(true, true), &t.join("c.mov"), &no, |_| {}).unwrap();
    assert!(c.copied && c.start_us == k);
    assert!((1_720_000..=1_800_000).contains(&dur(&c.path)), "nearest-keyframe copy {}", dur(&c.path));
    std::fs::remove_dir_all(&t).unwrap();
}

#[test]
fn cancel_leaves_nothing() {
    let Some(dir) = corpus() else { return };
    let t = tmp("cancel");
    let src = dir.join("prores_hq.mov");
    let opts = ProxyOptions { codec: ProxyCodec::ProresProxy, max_height: 540, out_dir: Some(t.clone()) };
    let r = make_proxy(&ProxySource { path: src, duration_us: 2_000_000, ..Default::default() }, &opts, &AtomicBool::new(true), |_| {});
    assert!(r.is_err());
    assert_eq!(std::fs::read_dir(&t).unwrap().count(), 0);
    std::fs::remove_dir_all(&t).unwrap();
}
