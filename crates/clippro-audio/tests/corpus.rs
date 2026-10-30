//! PLAN P10.1: the `audio_mix` corpus clip — every truth interval gets the right audio flags and
//! voice activity, and event boundaries land within ±50 ms. Skips without the corpus.

use clippro_audio::{AudioAnalyzer, AudioSet};
use clippro_types::FlagKind;
use std::path::Path;

fn analyse(p: &Path) -> AudioSet {
    let mut a: Option<AudioAnalyzer> = None;
    let st = clippro_media::audio::decode_audio(p, None, 0, |block| {
        a.get_or_insert_with(|| AudioAnalyzer::new(block.len(), 0)).push(block);
        true
    })
    .unwrap();
    let mut a = a.unwrap();
    a = { let x = a; x };
    let mut set = a.finish();
    set.start_us = st.start_us;
    set
}

#[test]
fn audio_mix_matches_truth() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
    let clip = root.join("gen/audio_mix.mp4");
    if !clip.exists() {
        eprintln!("corpus missing: skipping");
        return;
    }
    let truth: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("truth/audio_mix.json")).unwrap()).unwrap();
    let set = analyse(&clip);
    let key = |f: &str| FlagKind::ALL.into_iter().find(|k| k.key() == f).unwrap();
    for ev in truth["audio"].as_array().unwrap() {
        let (t0, t1) = (ev["t"][0].as_f64().unwrap(), ev["t"][1].as_f64().unwrap());
        let a = set.align(&[((t0 + t1) / 2.0 * 1e6) as i64], 1.0 / (t1 - t0));
        let bits = a.flags[0];
        let e = &ev["expect"];
        for f in e["audio_flags"].as_array().into_iter().flatten() {
            assert!(bits & key(f.as_str().unwrap()).bit() != 0, "{t0}-{t1}: expected {f}, flags {bits:#x}");
        }
        for f in e["no_audio_flags"].as_array().into_iter().flatten() {
            assert!(bits & key(f.as_str().unwrap()).bit() == 0, "{t0}-{t1}: unexpected {f}, flags {bits:#x}");
        }
        if e["speech"].as_bool() == Some(true) {
            assert!(a.speech[0] > 0.5, "{t0}-{t1}: speech fraction {}", a.speech[0]);
        }
    }
    // Event boundary: silence starts at 3.0 s (±50 ms, one 100 ms block of slack for gating).
    let first = set.flags.iter().position(|f| f & FlagKind::Silence.bit() != 0).unwrap();
    let t = set.start_us as f64 / 1e6 + first as f64 * 0.1;
    assert!((t - 3.0).abs() <= 0.15, "silence starts at {t}");
    eprintln!("integrated loudness {:?} LUFS", set.integrated_lufs(0, 2_000_000));
}

#[test]
#[ignore]
fn dump_audio_mix() {
    let clip = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/gen/audio_mix.mp4");
    let s = analyse(&clip);
    for k in (0..s.len()).step_by(3) {
        eprintln!("{:>4.1}s pk {:>6.2} tp {:>6.2} rms {:>6.1} lm {:>6.1} clip {} lf {:.2} fl {:.2} flux {:.2} hum {:.2} sp {:.2} zcr {:.3} nf {:>6.1} vad {} wind {:.2} flags {:#x}",
            s.start_us as f64 / 1e6 + k as f64 * 0.1, s.cols.peak_dbfs[k], s.cols.true_peak_dbtp[k], s.cols.rms_dbfs[k], s.cols.lufs_m[k], s.cols.clip_events[k],
            s.cols.lf_ratio[k], s.cols.flatness[k], s.cols.flux_lf[k], s.cols.hum[k], s.cols.speech_band[k], s.cols.zcr[k], s.noise_floor[k], s.vad[k], s.wind[k], s.flags[k]);
    }
}
