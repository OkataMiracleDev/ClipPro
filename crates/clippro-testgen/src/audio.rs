//! Synthetic audio with known events (PLAN P0.4 `audio_mix`, `multicam_*`).

use serde_json::{json, Value};
use std::f32::consts::TAU;

const SR: f32 = 48_000.0;

struct Lcg(u64);

impl Lcg {
    fn noise(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }
}

/// Two-pole resonator (formant filter).
struct Reso {
    a1: f32,
    a2: f32,
    g: f32,
    y1: f32,
    y2: f32,
}

impl Reso {
    fn new(freq: f32, bw: f32) -> Reso {
        let r = (-std::f32::consts::PI * bw / SR).exp();
        let a1 = -2.0 * r * (TAU * freq / SR).cos();
        let a2 = r * r;
        Reso { a1, a2, g: 1.0 - r, y1: 0.0, y2: 0.0 }
    }
    fn step(&mut self, x: f32) -> f32 {
        let y = self.g * x - self.a1 * self.y1 - self.a2 * self.y2;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Speech-like signal: a glottal pulse train through three formants, syllable-modulated.
pub fn speech(dur: f32, seed: u64) -> Vec<f32> {
    let n = (dur * SR) as usize;
    let mut f = [Reso::new(700.0, 110.0), Reso::new(1200.0, 120.0), Reso::new(2500.0, 160.0)];
    let mut rng = Lcg(seed);
    let mut phase = 0.0f32;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / SR;
        let f0 = 120.0 + 15.0 * (TAU * 0.7 * t).sin();
        phase += f0 / SR;
        let pulse = if phase >= 1.0 {
            phase -= 1.0;
            1.0
        } else {
            0.0
        };
        let syl = (0.5 + 0.5 * (TAU * 4.0 * t).sin()).powf(1.5) * if (t * 1.3).fract() < 0.85 { 1.0 } else { 0.05 };
        let x = pulse * 8.0 + 0.02 * rng.noise();
        let y = f[0].step(x) + 0.6 * f[1].step(x) + 0.3 * f[2].step(x);
        out.push(0.5 * syl * y);
    }
    let peak = out.iter().fold(0f32, |m, v| m.max(v.abs())).max(1e-6);
    out.iter().map(|v| v / peak * 0.35).collect()
}

/// The `audio_mix` clip: tone, clipping, silence, wind, hum, speech. Returns stereo + truth.
pub fn mix(dur: f32) -> (Vec<Vec<f32>>, Value) {
    let n = (dur * SR) as usize;
    let mut x = vec![0f32; n];
    let mut rng = Lcg(42);
    let sp = speech(2.0, 7);
    let mut lp = 0.0f32;
    let mut gust = 0.0f32;
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f32 / SR;
        *v = if t < 2.0 {
            0.25 * (TAU * 440.0 * t).sin()
        } else if t < 3.0 {
            (1.5 * (TAU * 440.0 * t).sin()).clamp(-1.0, 1.0)
        } else if t < 5.0 {
            0.0
        } else if t < 6.5 {
            // Wind: low-passed noise with gusts.
            lp += (rng.noise() - lp) * 0.012;
            gust += (rng.noise().abs() - gust) * 0.0002;
            lp * 6.0 * (0.4 + gust * 2.0)
        } else if t < 8.5 {
            let mut h = 0.0;
            for k in 1..=5 {
                h += (0.3 / k as f32) * (TAU * 50.0 * k as f32 * t).sin();
            }
            h + 0.002 * rng.noise()
        } else {
            let j = i - (8.5 * SR) as usize;
            sp.get(j).copied().unwrap_or(0.0)
        };
    }
    let truth = json!([
        {"t": [0.2, 1.8], "expect": {"no_audio_flags": ["audio_clipping", "silence", "wind", "hum"]}},
        {"t": [2.1, 2.9], "expect": {"audio_flags": ["audio_clipping"]}},
        {"t": [3.3, 4.7], "expect": {"audio_flags": ["silence"]}},
        {"t": [5.2, 6.3], "expect": {"audio_flags": ["wind"]}},
        {"t": [6.8, 8.3], "expect": {"audio_flags": ["hum"]}},
        {"t": [8.7, 9.9], "expect": {"speech": true}}
    ]);
    (vec![x.clone(), x], truth)
}

/// Signal for sync tests: speech plus irregular clicks (sharp onsets).
pub fn sync_signal(dur: f32) -> Vec<f32> {
    let mut v = speech(dur, 99);
    let mut rng = Lcg(5);
    let mut t = 0.3f32;
    while t < dur - 0.1 {
        let i = (t * SR) as usize;
        for k in 0..240 {
            if i + k < v.len() {
                v[i + k] += 0.6 * (-(k as f32) / 40.0).exp() * if k % 2 == 0 { 1.0 } else { -1.0 };
            }
        }
        t += 0.35 + 0.9 * (rng.noise() * 0.5 + 0.5);
    }
    v.iter().map(|x| x.clamp(-1.0, 1.0)).collect()
}
