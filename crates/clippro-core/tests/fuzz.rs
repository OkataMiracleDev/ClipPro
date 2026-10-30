//! P14.6 robustness: mutation fuzzing of every parser that reads files from outside the app —
//! timelines (EDL, FCP7 XML, FCPXML, OTIO, AAF), stored measurement blobs and audio sets. A
//! parser may reject an input but must never panic. Compressed formats are also mutated inside
//! the zstd layer so the decoder itself gets exercised.
//!
//! Time per target: `CLIPPRO_FUZZ_SECS` (default 3 s; the hardening run uses 1800).
//! Stable Rust stands in for `cargo fuzz` (nightly + libFuzzer), which this toolchain lacks.

use clippro_core::{Session, SessionOptions};
use clippro_types::FrameRate;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::time::{Duration, Instant};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// One to four random edits: bit flips, interesting bytes, truncation, chunk copy/delete/insert.
fn mutate(r: &mut Rng, seed: &[u8]) -> Vec<u8> {
    let mut v = seed.to_vec();
    for _ in 0..1 + r.below(4) {
        let n = v.len();
        match r.below(7) {
            0 if n > 0 => {
                let i = r.below(n);
                v[i] ^= 1 << r.below(8);
            }
            1 if n > 0 => {
                let i = r.below(n);
                v[i] = [0x00, 0xFF, 0x7F, 0x80, 0x01, b'<', b'"', b'{'][r.below(8)];
            }
            2 if n > 1 => v.truncate(r.below(n)),
            3 if n > 4 => {
                let (a, len) = (r.below(n), 1 + r.below(64));
                let chunk: Vec<u8> = v[a..(a + len).min(n)].to_vec();
                let at = r.below(v.len());
                v.splice(at..at, chunk);
            }
            4 if n > 4 => {
                let (a, len) = (r.below(n), 1 + r.below(64));
                v.drain(a..(a + len).min(n));
            }
            5 => {
                let at = r.below(n + 1);
                let junk: Vec<u8> = (0..1 + r.below(16)).map(|_| r.next() as u8).collect();
                v.splice(at..at, junk);
            }
            _ if n >= 4 => {
                // A 32-bit length or count field set to something extreme.
                let i = r.below(n - 3);
                let x = [0u32, 1, 0xFFFF, 0x7FFF_FFFF, 0xFFFF_FFFF][r.below(5)];
                v[i..i + 4].copy_from_slice(&x.to_le_bytes());
            }
            _ => {}
        }
    }
    v
}

fn budget() -> Duration {
    Duration::from_secs_f64(std::env::var("CLIPPRO_FUZZ_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(3.0))
}

/// Runs `f` on mutations of the seeds until the budget is spent; panics are collected.
fn fuzz(name: &str, seeds: &[Vec<u8>], inner_zstd: bool, f: impl Fn(&[u8])) {
    assert!(!seeds.is_empty(), "{name}: no seeds");
    let plain: Vec<Vec<u8>> = if inner_zstd { seeds.iter().map(|s| zstd::decode_all(&s[..]).unwrap_or_default()).collect() } else { Vec::new() };
    let mut r = Rng(0x9E37_79B9_7F4A_7C15 ^ name.len() as u64);
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let (start, mut runs, mut panics) = (Instant::now(), 0u64, Vec::new());
    while start.elapsed() < budget() {
        let k = r.below(seeds.len());
        let input = if inner_zstd && r.below(2) == 0 && !plain[k].is_empty() {
            zstd::bulk::compress(&mutate(&mut r, &plain[k]), 1).unwrap_or_default()
        } else {
            mutate(&mut r, &seeds[k])
        };
        if catch_unwind(AssertUnwindSafe(|| f(&input))).is_err() && panics.len() < 5 {
            panics.push(input);
        }
        runs += 1;
    }
    std::panic::set_hook(prev);
    eprintln!("{name}: {runs} inputs, {} panic(s)", panics.len());
    if !panics.is_empty() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fuzz-crashes");
        std::fs::create_dir_all(&dir).ok();
        for (i, p) in panics.iter().enumerate() {
            std::fs::write(dir.join(format!("{name}-{i}.bin")), p).ok();
        }
        panic!("{name}: {} input(s) panicked; saved to target/fuzz-crashes", panics.len());
    }
}

/// Inputs the long fuzz runs found; each must be rejected cleanly.
#[test]
fn known_crashers_are_rejected() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fuzz");
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        let b = std::fs::read(&p).unwrap();
        let _ = clippro_interchange::import::import_bytes(&b, FrameRate::new(25, 1));
        eprintln!("{} handled", p.display());
    }
}

#[test]
fn timeline_importers_never_panic() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/interchange");
    let seeds: Vec<Vec<u8>> = std::fs::read_dir(&dir).unwrap().filter_map(|e| std::fs::read(e.ok()?.path()).ok()).collect();
    let rate = FrameRate::new(25, 1);
    fuzz("timelines", &seeds, false, |b| {
        let _ = clippro_interchange::import::import_bytes(b, rate);
    });
}

#[test]
fn stored_blobs_never_panic() {
    let gen = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/gen");
    if !gen.join("audio_mix.mp4").exists() {
        eprintln!("corpus missing: skipping");
        return;
    }
    // Real blobs from a real analysis.
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-fuzz");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("media")).unwrap();
    for f in ["audio_mix.mp4", "handheld_shake.mp4"] {
        std::fs::copy(gen.join(f), tmp.join("media").join(f)).unwrap();
    }
    let s = Session::create(&tmp.join("p.clippro"), "fuzz", SessionOptions { cache_path: None, ..SessionOptions::default() }).unwrap();
    s.import(&[tmp.join("media")]).unwrap();
    s.queue_analysis().unwrap();
    s.wait(|_| {});
    let cal = clippro_measure::calibration::Calibration::embedded();
    let ms: Vec<Vec<u8>> = s.clip_inputs().unwrap().iter().map(|c| clippro_measure::codec::encode(&c.ms, cal).unwrap()).collect();
    let audio: Vec<Vec<u8>> = s.clip_audio().unwrap().iter().map(|(_, a)| a.encode()).collect();
    s.close().unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
    assert!(ms.len() == 2 && !audio.is_empty());
    fuzz("measurements", &ms, true, |b| {
        let _ = clippro_measure::codec::decode(b);
    });
    fuzz("audio", &audio, true, |b| {
        let _ = clippro_audio::AudioSet::decode(b);
    });
}
