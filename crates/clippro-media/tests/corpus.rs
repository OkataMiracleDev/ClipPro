//! P1 corpus tests. Needs the synthetic corpus: `clippro-testgen --out testdata/gen [--mini]`.
//! Tests skip (with a message) when the corpus is missing.

use clippro_media::audio::decode_audio;
use clippro_media::*;
use clippro_types::SampleRate;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

fn corpus() -> Option<PathBuf> {
    let d = std::env::var("CLIPPRO_CORPUS").map(PathBuf::from).unwrap_or_else(|_| root().join("gen"));
    if d.join("blur_ramp.mp4").exists() {
        Some(d)
    } else {
        eprintln!("corpus missing at {}: skipping", d.display());
        None
    }
}

fn truths() -> Vec<Value> {
    let mut v: Vec<Value> = std::fs::read_dir(root().join("truth"))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
                .filter_map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok())
                .collect()
        })
        .unwrap_or_default();
    v.sort_by_key(|t| t["name"].as_str().unwrap_or("").to_string());
    v
}

#[test]
fn probe_every_corpus_clip() {
    let Some(dir) = corpus() else { return };
    let mut n = 0;
    for t in truths() {
        let file = dir.join(t["file"].as_str().unwrap());
        if !file.exists() {
            continue;
        }
        let info = probe(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        let v = info.video.as_ref().expect("video stream");
        let dur = t["duration_s"].as_f64().unwrap();
        assert!((info.duration_us as f64 / 1e6 - dur).abs() <= 0.08, "{}: duration {} vs {dur}", file.display(), info.duration_us);
        if t.get("vfr").is_none() {
            assert!((v.fps.fps() - 25.0).abs() < 0.01, "{}: fps {}", file.display(), v.fps.fps());
        }
        if let Some(bd) = t.get("bit_depth").and_then(|b| b.as_u64()) {
            assert_eq!(v.bit_depth as u64, bd, "{}", file.display());
        }
        if let Some(c) = t.get("chroma").and_then(|c| c.as_str()) {
            assert_eq!(v.chroma, c);
        }
        if let Some(c) = t.get("codec").and_then(|c| c.as_str()) {
            assert_eq!(v.codec, c);
        }
        if let Some(tc) = t.get("timecode").and_then(|c| c.as_str()) {
            assert_eq!(info.start_timecode.as_deref(), Some(tc), "{}: tags {:?}", file.display(), info.tags);
        }
        if let Some(r) = t.get("reel").and_then(|c| c.as_str()) {
            assert_eq!(info.reel.as_deref(), Some(r), "{}: tags {:?}", file.display(), info.tags);
        }
        n += 1;
    }
    assert!(n >= 30, "only {n} clips probed");
    let a = probe(&dir.join("audio_mix.mp4")).unwrap();
    assert_eq!(a.audio.len(), 1);
    assert_eq!(a.audio[0].channels, 2);
}

fn collect(path: &Path, rate: SampleRate, pair: bool, hw: HwMode) -> (Vec<(usize, i64, i64, Option<i64>)>, video::DecodeStats) {
    let info = probe(path).unwrap();
    let plan = sample_plan(info.duration_us, info.fps(), rate);
    let mut dec = VideoDecoder::open(path, &info, &DecodeOptions { threads: 2, hw }).unwrap();
    let mut got = Vec::new();
    let stats = dec
        .decode_samples(&plan, 0, pair, |s| {
            got.push((s.index, s.target_us, s.frame.pts_us, s.pair.as_ref().map(|p| p.pts_us)));
            true
        })
        .unwrap();
    (got, stats)
}

#[test]
fn sampling_times_and_pairs() {
    let Some(dir) = corpus() else { return };
    for name in ["pan_right.mp4", "prores_hq.mov", "dnxhr.mxf", "hevc10.mp4"] {
        let p = dir.join(name);
        if !p.exists() {
            continue;
        }
        let info = probe(&p).unwrap();
        let plan = sample_plan(info.duration_us, info.fps(), SampleRate::Standard);
        let (got, _) = collect(&p, SampleRate::Standard, true, HwMode::Off);
        assert_eq!(got.len(), plan.len(), "{name}");
        for (i, (idx, tgt, pts, pair)) in got.iter().enumerate() {
            assert_eq!(*idx, i);
            assert!(*pts >= *tgt - 1000 && *pts < *tgt + 41_000, "{name} #{i}: target {tgt} pts {pts}");
            if i + 1 < got.len() {
                let pp = pair.unwrap_or_else(|| panic!("{name} #{i} missing pair"));
                assert!((pp - pts - 40_000).abs() <= 1000, "{name} #{i}: pair {pp} vs {pts}");
            }
        }
        // The last frame is always sampled.
        let last_frame = info.fps().frames_to_us((info.duration_us as f64 / 40_000.0).round() as i64 - 1);
        assert!((got.last().unwrap().2 - last_frame).abs() <= 41_000, "{name}: last {:?} vs {last_frame}", got.last());
    }
}

#[test]
fn every_frame_mode_and_vfr() {
    let Some(dir) = corpus() else { return };
    let p = dir.join("tripod_static.mp4");
    let (got, _) = collect(&p, SampleRate::All, false, HwMode::Off);
    assert_eq!(got.len(), 150);
    assert!(got.windows(2).all(|w| w[1].2 > w[0].2));
    let p = dir.join("vfr.mkv");
    let (got, _) = collect(&p, SampleRate::Standard, true, HwMode::Off);
    assert!(got.len() >= 19);
    for w in got.windows(2) {
        assert!(w[1].2 >= w[0].2, "non-monotonic vfr pts");
    }
    for (_, tgt, pts, _) in &got {
        assert!(*pts >= *tgt - 1000, "vfr frame before target");
    }
}

#[test]
fn draft_rate_skips_frames_on_long_gop() {
    let Some(dir) = corpus() else { return };
    let p = dir.join("long_clip.mp4");
    let info = probe(&p).unwrap();
    let total = (info.duration_us as f64 / 40_000.0).round() as u64;
    let (got, stats) = collect(&p, SampleRate::Draft, false, HwMode::Off);
    assert_eq!(got.len() as u64, (info.duration_us / 1_000_000 + 1) as u64);
    eprintln!("long_clip draft: decoded {} of {total} frames, {} seeks", stats.frames_decoded, stats.seeks);
    assert!((stats.frames_decoded as f64) <= 0.6 * total as f64, "decoded {} of {total}", stats.frames_decoded);
}

#[test]
fn hardware_decode_matches_cpu() {
    let Some(dir) = corpus() else { return };
    for name in ["pan_right.mp4", "hevc10.mp4"] {
        let p = dir.join(name);
        if !p.exists() {
            continue;
        }
        let info = probe(&p).unwrap();
        let plan: Vec<i64> = sample_plan(info.duration_us, info.fps(), SampleRate::Draft);
        let lumas = |hw: HwMode| -> (Vec<Vec<f32>>, bool) {
            let mut dec = VideoDecoder::open(&p, &info, &DecodeOptions { threads: 2, hw }).unwrap();
            let mut out = Vec::new();
            let st = dec
                .decode_samples(&plan, 0, false, |s| {
                    let l = &s.frame.luma;
                    let mut row = vec![0f32; l.width];
                    let mut all = Vec::with_capacity(l.width * l.height);
                    for y in 0..l.height {
                        l.row_f32(y, &mut row);
                        all.extend_from_slice(&row);
                    }
                    out.push(all);
                    true
                })
                .unwrap();
            (out, st.hw)
        };
        let (cpu, _) = lumas(HwMode::Off);
        let (gpu, used_hw) = lumas(HwMode::Auto);
        if !used_hw {
            eprintln!("{name}: hardware decode unavailable; skipping parity");
            continue;
        }
        assert_eq!(cpu.len(), gpu.len());
        let bits = info.video.as_ref().unwrap().bit_depth as i32;
        let one_code = 1.0 / (219.0 * 2f32.powi(bits - 8)) + 1e-6;
        for (a, b) in cpu.iter().zip(&gpu) {
            assert_eq!(a.len(), b.len());
            let ok = a.iter().zip(b).filter(|(x, y)| (*x - *y).abs() <= one_code).count();
            let frac = ok as f64 / a.len() as f64;
            assert!(frac >= 0.999, "{name}: only {:.4} of pixels within one code value", frac);
        }
    }
}

#[test]
fn audio_events() {
    let Some(dir) = corpus() else { return };
    let p = dir.join("audio_mix.mp4");
    let mut ch0: Vec<f32> = Vec::new();
    let st = decode_audio(&p, None, 0, |block| {
        ch0.extend_from_slice(&block[0]);
        true
    })
    .unwrap();
    assert_eq!(st.channels, 2);
    let sr = 48_000.0;
    assert!((ch0.len() as f64 / sr - 10.0).abs() < 0.1, "{} samples", ch0.len());
    let rms = |a: f64, b: f64| {
        let s = &ch0[(a * sr) as usize..(b * sr) as usize];
        (s.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / s.len() as f64).sqrt()
    };
    let peak = |a: f64, b: f64| ch0[(a * sr) as usize..(b * sr) as usize].iter().fold(0f32, |m, v| m.max(v.abs()));
    assert!(rms(3.3, 4.7) < 1e-3, "silence rms {}", rms(3.3, 4.7));
    assert!(peak(2.1, 2.9) > 0.95, "clipping peak {}", peak(2.1, 2.9));
    assert!((rms(0.5, 1.5) - 0.25 / 2f64.sqrt()).abs() < 0.02, "tone rms {}", rms(0.5, 1.5));
    // Event boundary: silence starts at 3.0 s (±1 ms of 48 kHz, allowing AAC priming).
    let win = 480; // 10 ms
    let start = (2.8 * sr) as usize;
    let first_quiet = (start..ch0.len() - win)
        .step_by(48)
        .find(|&i| (ch0[i..i + win].iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / win as f64).sqrt() < 1e-3)
        .unwrap();
    assert!((first_quiet as f64 / sr - 3.0).abs() < 0.03, "silence starts at {}", first_quiet as f64 / sr);
}

#[test]
fn corrupt_and_truncated_files_error_cleanly() {
    let Some(dir) = corpus() else { return };
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-media-test");
    std::fs::create_dir_all(&tmp).unwrap();
    let src = std::fs::read(dir.join("pan_right.mp4")).unwrap();
    let trunc = tmp.join("truncated.mp4");
    std::fs::write(&trunc, &src[..src.len() / 3]).unwrap();
    let junk = tmp.join("junk.mp4");
    std::fs::write(&junk, vec![0x5Au8; 100_000]).unwrap();
    assert!(probe(&junk).is_err());
    // The truncated file may probe; decoding must not panic and must return.
    if let Ok(info) = probe(&trunc) {
        if let Ok(mut d) = VideoDecoder::open(&trunc, &info, &DecodeOptions::default()) {
            let plan = sample_plan(info.duration_us, info.fps(), SampleRate::Standard);
            let _ = d.decode_samples(&plan, 0, true, |_| true);
        }
    }
    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
fn preview_frames_play_forward_without_seeking() {
    let Some(dir) = corpus() else { return };
    let p = dir.join("pan_right.mp4");
    let info = probe(&p).unwrap();
    let step = info.fps().frame_duration_us();
    for hw in [HwMode::Off, HwMode::Auto] {
        let mut dec = VideoDecoder::open(&p, &info, &DecodeOptions { threads: 2, hw }).unwrap();
        let start = 1_000_000i64;
        let mut last = -1;
        for k in 0..48 {
            let t = start + (k as f64 * step) as i64;
            let f = dec.frame_near(t).unwrap().expect("frame");
            let pts = dec.rel_pts_us(&f);
            assert!((pts - t).abs() <= (step / 2.0) as i64 + 1, "target {t}, got {pts}");
            assert!(pts > last, "frames advance");
            last = pts;
        }
        assert_eq!(dec.stats.seeks, 1, "playback decodes forward");
        // Re-requesting the same time shows the same frame without decoding.
        let again = dec.frame_near(last).unwrap().unwrap();
        assert_eq!(dec.rel_pts_us(&again), last);
        // Scrubbing back seeks.
        let f = dec.frame_near(200_000).unwrap().unwrap();
        assert!((dec.rel_pts_us(&f) - 200_000).abs() <= (step / 2.0) as i64 + 1);
        assert_eq!(dec.stats.seeks, 2);
        let (w, h, rgb) = dec.to_rgb(&f, 320).unwrap();
        assert_eq!(w, 320);
        assert_eq!(rgb.len(), (w * h * 3) as usize);
        assert!(rgb.iter().any(|&v| v > 16), "non-black picture");
    }
}

#[test]
fn audio_seek_and_rate() {
    let Some(dir) = corpus() else { return };
    let p = dir.join("audio_mix.mp4");
    let sr = 44_100.0;
    let mut ch0: Vec<f32> = Vec::new();
    let opts = audio::AudioOptions { index: None, video_start_us: 0, from_us: Some(2_500_000), out_rate: sr as i32 };
    let st = audio::decode_audio_with(&p, &opts, |block| {
        ch0.extend_from_slice(&block[0]);
        true
    })
    .unwrap();
    assert_eq!(st.start_us, 2_500_000);
    assert!((ch0.len() as f64 / sr - 7.5).abs() < 0.1, "{} samples", ch0.len());
    // The silence that starts at 3.0 s in the clip starts 0.5 s into this output.
    let win = 441;
    let first_quiet = (0..ch0.len() - win)
        .step_by(44)
        .find(|&i| (ch0[i..i + win].iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / win as f64).sqrt() < 1e-3)
        .unwrap();
    assert!((first_quiet as f64 / sr - 0.5).abs() < 0.03, "silence starts at {}", first_quiet as f64 / sr);
}

/// PLAN P6.1: scrubbing shows exactly the frame at the requested time, across the codec corpus
/// (long-GOP H.264/HEVC, intra ProRes/DNx, 10-bit 4:2:2, VFR).
#[test]
fn preview_seek_accuracy_across_codecs() {
    let Some(dir) = corpus() else { return };
    for name in ["pan_right.mp4", "long_clip.mp4", "hevc10.mp4", "prores_hq.mov", "dnxhr.mxf", "h264_422_10.mov", "vfr.mkv"] {
        let p = dir.join(name);
        if !p.exists() {
            continue;
        }
        let info = probe(&p).unwrap();
        let (all, _) = collect(&p, SampleRate::All, false, HwMode::Off);
        let pts: Vec<i64> = all.iter().map(|x| x.2).collect();
        assert!(pts.len() > 10, "{name}: {} frames", pts.len());
        let half = (info.fps().frame_duration_us() / 2.0) as i64;
        let expected = |t: i64| pts.iter().copied().filter(|&q| q <= t + half).max().unwrap_or(pts[0]);
        let mut dec = VideoDecoder::open(&p, &info, &DecodeOptions { threads: 2, hw: HwMode::Off }).unwrap();
        let span = *pts.last().unwrap();
        let mut rng = 0x9e37_79b9_7f4a_7c15u64;
        let mut misses = Vec::new();
        let mut last = 0i64;
        for k in 0..30 {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            // Mix random scrubs with one- and three-frame forward steps (the playback path).
            let t = match k % 3 {
                0 => (rng % span.max(1) as u64) as i64,
                1 => (last + 2 * half).min(span),
                _ => (last + 6 * half).min(span),
            };
            last = t;
            let f = dec.frame_near(t).unwrap().expect("frame");
            let got = dec.rel_pts_us(&f);
            if got != expected(t) {
                misses.push((t, got, expected(t)));
            }
        }
        assert!(misses.is_empty(), "{name}: wrong frames (target, got, want): {misses:?}");
    }
}
