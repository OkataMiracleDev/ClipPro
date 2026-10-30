//! Speaker turns (SPEC AI-3, PLAN P12.4): Kaldi-style 80-bin fbank features, ECAPA-TDNN
//! embeddings over 1.5 s windows of speech, and clustering into speakers.

use crate::faces::cluster;
use crate::models::installed_path;
use crate::runtime::session;
use ndarray::Array3;
use ort::session::Session;
use ort::value::Tensor;
use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;
use serde::{Deserialize, Serialize};

pub const RATE: usize = 16_000;
const FRAME: usize = 400; // 25 ms
const SHIFT: usize = 160; // 10 ms
const NFFT: usize = 512;
const MELS: usize = 80;
/// Same-speaker windows cluster within this cosine distance (calibrated for ECAPA-512).
pub const SPEAKER_DISTANCE: f32 = 0.55;

fn mel(f: f32) -> f32 {
    1127.0 * (1.0 + f / 700.0).ln()
}

/// Triangular mel filters over the NFFT/2+1 bins (20 Hz to Nyquist), as Kaldi builds them.
fn filters() -> Vec<Vec<(usize, f32)>> {
    let (lo, hi) = (mel(20.0), mel(RATE as f32 / 2.0));
    let delta = (hi - lo) / (MELS + 1) as f32;
    let bin_hz = RATE as f32 / NFFT as f32;
    (0..MELS)
        .map(|m| {
            let (l, c, r) = (lo + m as f32 * delta, lo + (m + 1) as f32 * delta, lo + (m + 2) as f32 * delta);
            (1..NFFT / 2)
                .filter_map(|k| {
                    let x = mel(k as f32 * bin_hz);
                    let w = if x > l && x <= c { (x - l) / (c - l) } else if x > c && x < r { (r - x) / (r - c) } else { 0.0 };
                    (w > 0.0).then_some((k, w))
                })
                .collect()
        })
        .collect()
}

/// Log mel filterbank energies (one row per 10 ms), mean-normalised over the input, from
/// mono 16 kHz audio in [-1, 1] (scaled to 16-bit range as WeSpeaker's training data was).
pub fn fbank(audio: &[f32]) -> Vec<[f32; MELS]> {
    if audio.len() < FRAME {
        return Vec::new();
    }
    let n = 1 + (audio.len() - FRAME) / SHIFT;
    let fft = FftPlanner::new().plan_fft_forward(NFFT);
    let povey: Vec<f32> = (0..FRAME).map(|i| (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (FRAME - 1) as f32).cos()).powf(0.85)).collect();
    let fl = filters();
    let mut out = Vec::with_capacity(n);
    let mut buf = vec![Complex32::new(0.0, 0.0); NFFT];
    for f in 0..n {
        let mut x: Vec<f32> = audio[f * SHIFT..f * SHIFT + FRAME].iter().map(|v| v * 32768.0).collect();
        let mean = x.iter().sum::<f32>() / FRAME as f32;
        x.iter_mut().for_each(|v| *v -= mean);
        for i in (1..FRAME).rev() {
            x[i] -= 0.97 * x[i - 1];
        }
        x[0] -= 0.97 * x[0];
        for (i, b) in buf.iter_mut().enumerate() {
            *b = Complex32::new(if i < FRAME { x[i] * povey[i] } else { 0.0 }, 0.0);
        }
        fft.process(&mut buf);
        let power: Vec<f32> = buf[..=NFFT / 2].iter().map(|c| c.norm_sqr()).collect();
        let mut row = [0f32; MELS];
        for (m, filt) in fl.iter().enumerate() {
            let e: f32 = filt.iter().map(|(k, w)| power[*k] * w).sum();
            row[m] = e.max(f32::EPSILON).ln();
        }
        out.push(row);
    }
    // Cepstral mean normalisation.
    let mut mean = [0f32; MELS];
    for r in &out {
        for (a, b) in mean.iter_mut().zip(r) {
            *a += b;
        }
    }
    mean.iter_mut().for_each(|m| *m /= out.len() as f32);
    for r in &mut out {
        for (a, b) in r.iter_mut().zip(&mean) {
            *a -= b;
        }
    }
    out
}

pub struct SpeakerEngine {
    s: Session,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub start_us: i64,
    pub end_us: i64,
    pub speaker: usize,
}

impl SpeakerEngine {
    pub fn load(deterministic: bool) -> Result<SpeakerEngine, String> {
        let p = installed_path("ecapa-voxceleb-512").ok_or("the speaker model isn't installed")?;
        Ok(SpeakerEngine { s: session(&p, deterministic)? })
    }

    /// Unit-length 192-d embedding of a stretch of speech.
    pub fn embed(&mut self, audio: &[f32]) -> Result<Vec<f32>, String> {
        let f = fbank(audio);
        if f.is_empty() {
            return Err("too short to embed".into());
        }
        let t = f.len();
        let flat: Vec<f32> = f.iter().flatten().copied().collect();
        let input = Tensor::from_array(Array3::from_shape_vec((1, t, MELS), flat).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let out = self.s.run(ort::inputs!["feats" => input]).map_err(|e| e.to_string())?;
        let (_, d) = out["embs"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        let mut v = d.to_vec();
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
        v.iter_mut().for_each(|x| *x /= n);
        Ok(v)
    }

    /// Speaker turns over the speech regions (µs spans) of a 16 kHz mono clip: 1.5 s windows
    /// every 0.75 s, clustered, then adjacent windows of the same speaker merged.
    pub fn turns(&mut self, audio: &[f32], speech: &[(i64, i64)]) -> Result<Vec<Turn>, String> {
        let mut wins: Vec<(i64, i64, Vec<f32>)> = Vec::new();
        for &(a, b) in speech {
            let (mut t, end) = (a, b);
            while t < end {
                let e = (t + 1_500_000).min(end);
                if e - t >= 500_000 {
                    let (i, j) = ((t as usize * RATE / 1_000_000).min(audio.len()), (e as usize * RATE / 1_000_000).min(audio.len()));
                    if let Ok(v) = self.embed(&audio[i..j]) {
                        wins.push((t, e, v));
                    }
                }
                t += 750_000;
            }
        }
        let labels = cluster(&wins.iter().map(|w| w.2.clone()).collect::<Vec<_>>(), SPEAKER_DISTANCE);
        let mut out: Vec<Turn> = Vec::new();
        for ((a, b, _), l) in wins.iter().zip(labels) {
            match out.last_mut() {
                Some(t) if t.speaker == l && *a <= t.end_us => t.end_us = t.end_us.max(*b),
                _ => out.push(Turn { start_us: *a, end_us: *b, speaker: l }),
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(f0: f32, secs: f32, seed: u32) -> Vec<f32> {
        // A buzzy harmonic source with a formant-like tilt: crude, but voice-shaped.
        (0..(secs * RATE as f32) as usize)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let vib = 1.0 + 0.02 * (std::f32::consts::TAU * 5.0 * t + seed as f32).sin();
                (1..20).map(|h| (std::f32::consts::TAU * f0 * vib * h as f32 * t).sin() / (h as f32).powf(1.2)).sum::<f32>() * 0.05
            })
            .collect()
    }

    #[test]
    fn fbank_shape() {
        let f = fbank(&voice(120.0, 1.0, 1));
        assert_eq!(f.len(), 98);
        let col0: f32 = f.iter().map(|r| r[0]).sum::<f32>() / f.len() as f32;
        assert!(col0.abs() < 1e-3, "mean-normalised");
        assert!(fbank(&[0.0; 100]).is_empty());
    }

    #[test]
    fn embeddings_with_installed_model() {
        let Ok(mut e) = SpeakerEngine::load(true) else {
            eprintln!("speaker model not installed: skipping");
            return;
        };
        let (a1, a2, b) = (e.embed(&voice(110.0, 2.0, 1)).unwrap(), e.embed(&voice(110.0, 2.0, 7)).unwrap(), e.embed(&voice(240.0, 2.0, 3)).unwrap());
        assert_eq!(a1.len(), 192);
        let cos = |x: &[f32], y: &[f32]| x.iter().zip(y).map(|(p, q)| p * q).sum::<f32>();
        eprintln!("same {:.3} different {:.3}", cos(&a1, &a2), cos(&a1, &b));
        assert!(cos(&a1, &a2) > cos(&a1, &b));
        let mut audio = voice(110.0, 3.0, 1);
        audio.extend(voice(240.0, 3.0, 3));
        let t = e.turns(&audio, &[(0, 6_000_000)]).unwrap();
        assert!(t.len() >= 2 && t[0].speaker != t.last().unwrap().speaker, "{t:?}");
    }
}
