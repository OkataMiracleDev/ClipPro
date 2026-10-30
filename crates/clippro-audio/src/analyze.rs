//! Per-block audio measurement (ALGORITHMS §4). Input arrives as planar f32 at 48 kHz in any block
//! sizes; every 100 ms (4800 samples) one row of measures is produced.

use crate::set::{AudioSet, Cols};
use ebur128::{EbuR128, Mode};
use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

pub const RATE: usize = 48_000;
pub const BLOCK: usize = 4_800; // 100 ms
const NFFT: usize = 4_096;
/// Hum needs sub-hertz resolution around 50/60 Hz harmonics: 1 s of audio, zero-padded.
const HUM_FFT: usize = 65_536;
const HUM_EVERY: usize = 5; // blocks (500 ms)

fn db(x: f64) -> f32 {
    if x <= 1e-12 { -120.0 } else { (20.0 * x.log10()) as f32 }
}

fn power_db(p: f64) -> f32 {
    if p <= 1e-24 { -240.0 } else { (10.0 * p.log10()) as f32 }
}

pub struct AudioAnalyzer {
    channels: usize,
    ebu: Option<EbuR128>,
    pending: Vec<Vec<f32>>,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    prev_lf: Vec<f32>,
    hum_fft: Arc<dyn Fft<f32>>,
    hum_buf: std::collections::VecDeque<f32>,
    hum_window: Vec<f32>,
    last_hum: f32,
    /// Rows before this index have their final (centred) hum score.
    hum_filled: usize,
    cols: Cols,
    ch_rms: Vec<Vec<f32>>,
    start_us: i64,
}

impl AudioAnalyzer {
    /// `start_us`: time of the first sample relative to the first video frame.
    pub fn new(channels: usize, start_us: i64) -> AudioAnalyzer {
        let channels = channels.max(1);
        let ebu = EbuR128::new(channels as u32, RATE as u32, Mode::M | Mode::S | Mode::TRUE_PEAK).ok();
        let window: Vec<f32> = (0..NFFT).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / NFFT as f32).cos()).collect();
        AudioAnalyzer {
            channels,
            ebu,
            pending: vec![Vec::new(); channels],
            fft: FftPlanner::new().plan_fft_forward(NFFT),
            window,
            prev_lf: Vec::new(),
            hum_fft: FftPlanner::new().plan_fft_forward(HUM_FFT),
            hum_buf: std::collections::VecDeque::with_capacity(RATE),
            hum_window: (0..RATE).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / RATE as f32).cos()).collect(),
            last_hum: 0.0,
            hum_filled: 0,
            cols: Cols::default(),
            ch_rms: vec![Vec::new(); channels],
            start_us,
        }
    }

    /// Feeds planar samples (one slice per channel; extra channels are ignored).
    pub fn push(&mut self, block: &[Vec<f32>]) {
        for c in 0..self.channels {
            let src = block.get(c).or(block.last());
            if let Some(src) = src {
                self.pending[c].extend_from_slice(src);
            }
        }
        while self.pending[0].len() >= BLOCK {
            let chunk: Vec<Vec<f32>> = self.pending.iter_mut().map(|p| p.drain(..BLOCK).collect()).collect();
            self.measure(&chunk);
        }
    }

    fn measure(&mut self, ch: &[Vec<f32>]) {
        let n = BLOCK;
        // Loudness and true peak (BS.1770 via ebur128).
        let (mut lm, mut ls, mut tp, mut lblock) = (-120.0f32, -120.0f32, -120.0f32, -120.0f32);
        if let Some(e) = self.ebu.as_mut() {
            let refs: Vec<&[f32]> = ch.iter().map(|c| c.as_slice()).collect();
            if e.add_frames_planar_f32(&refs).is_ok() {
                lm = e.loudness_momentary().map(|v| v as f32).unwrap_or(-120.0).max(-120.0);
                ls = e.loudness_shortterm().map(|v| v as f32).unwrap_or(-120.0).max(-120.0);
                lblock = e.loudness_window(100).map(|v| v as f32).unwrap_or(-120.0).max(-120.0);
                tp = (0..self.channels).map(|c| e.prev_true_peak(c as u32).unwrap_or(0.0)).fold(0.0f64, f64::max).pipe(db);
            }
        }
        // Mono mix and time-domain measures.
        let mono: Vec<f32> = (0..n).map(|i| ch.iter().map(|c| c[i]).sum::<f32>() / ch.len() as f32).collect();
        let mut peak = 0f32;
        let mut sum2 = 0f64;
        let mut zc = 0usize;
        for (i, v) in mono.iter().enumerate() {
            peak = peak.max(v.abs());
            sum2 += (*v as f64) * (*v as f64);
            if i > 0 && (mono[i - 1] >= 0.0) != (*v >= 0.0) {
                zc += 1;
            }
        }
        // Clip events: runs of ≥ 3 consecutive samples at |x| ≥ 0.999, on any channel.
        let mut clips = 0u32;
        for c in ch {
            let mut run = 0;
            for v in c {
                if v.abs() >= 0.999 {
                    run += 1;
                    if run == 3 {
                        clips += 1;
                    }
                } else {
                    run = 0;
                }
            }
        }
        // Clipping that went through a lossy codec no longer reaches full scale; it shows as
        // flat tops: a peak near 0 dBFS with many samples close to it (ADR-004).
        if clips == 0 {
            for c in ch {
                let pk = c.iter().fold(0f32, |m, v| m.max(v.abs()));
                if pk >= 0.891 {
                    // ≥ −1 dBFS
                    let near = c.iter().filter(|v| v.abs() >= pk * 0.944).count(); // within 0.5 dB
                    if near as f32 / c.len() as f32 > 0.35 {
                        clips += 1;
                    }
                }
            }
        }
        for (k, c) in ch.iter().enumerate() {
            let e = c.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / n as f64;
            self.ch_rms[k].push(db(e.sqrt()));
        }
        // Spectrum of the first NFFT samples (Hann).
        let mut buf: Vec<Complex32> = (0..NFFT).map(|i| Complex32::new(mono[i] * self.window[i], 0.0)).collect();
        self.fft.process(&mut buf);
        let bin_hz = RATE as f32 / NFFT as f32;
        let pw: Vec<f64> = buf[..NFFT / 2].iter().map(|c| (c.norm_sqr() as f64) / NFFT as f64).collect();
        let band = |lo: f32, hi: f32| -> f64 {
            let a = (lo / bin_hz).ceil() as usize;
            let b = ((hi / bin_hz).floor() as usize).min(pw.len() - 1);
            if b < a { 0.0 } else { pw[a..=b].iter().sum() }
        };
        let total = band(20.0, 20_000.0).max(1e-20);
        let lf_ratio = (band(20.0, 150.0) / total) as f32;
        let speech_band = (band(300.0, 3_400.0) / total) as f32;
        // Spectral flatness over 100 Hz–8 kHz.
        let (a, b) = ((100.0 / bin_hz).ceil() as usize, (8_000.0 / bin_hz).floor() as usize);
        let seg = &pw[a..=b];
        let gm = (seg.iter().map(|p| (p + 1e-20).ln()).sum::<f64>() / seg.len() as f64).exp();
        let am = seg.iter().sum::<f64>() / seg.len() as f64;
        let flatness = if am > 1e-18 { (gm / am) as f32 } else { 1.0 };
        // Positive spectral flux in 20–150 Hz.
        let (la, lb) = ((20.0 / bin_hz).ceil() as usize, (150.0 / bin_hz).floor() as usize);
        let lf: Vec<f32> = pw[la..=lb].iter().map(|p| (p.sqrt()) as f32).collect();
        let flux = if self.prev_lf.len() == lf.len() { lf.iter().zip(&self.prev_lf).map(|(x, y)| (x - y).max(0.0)).sum::<f32>() } else { 0.0 };
        let lf_sum = lf.iter().sum::<f32>().max(1e-9);
        self.prev_lf = lf;
        // Hum: harmonics of 50/60 Hz standing out of their neighbourhood (1 s window).
        self.hum_buf.extend(mono.iter().copied());
        while self.hum_buf.len() > RATE {
            self.hum_buf.pop_front();
        }
        // The 1 s window ends at this block; its score belongs to the block 0.5 s back, so earlier
        // rows are filled in as later audio arrives (and the tail in `finish`).
        let idx = self.cols.rms_dbfs.len();
        if self.hum_buf.len() == RATE && idx.is_multiple_of(HUM_EVERY) {
            self.last_hum = self.hum_score();
            let centre = idx.saturating_sub(5);
            for k in self.hum_filled..=centre.min(idx.saturating_sub(1)) {
                if let Some(h) = self.cols.hum.get_mut(k) {
                    *h = self.last_hum;
                }
            }
            self.hum_filled = centre + 1;
        }
        let hum = self.last_hum;
        let c = &mut self.cols;
        c.peak_dbfs.push(db(peak as f64));
        c.rms_dbfs.push(db((sum2 / n as f64).sqrt()));
        c.true_peak_dbtp.push(tp);
        c.lufs_m.push(lm);
        c.lufs_s.push(ls);
        c.clip_events.push(clips as f32);
        c.lf_ratio.push(lf_ratio);
        c.flatness.push(flatness);
        c.flux_lf.push(flux / lf_sum);
        c.hum.push(hum);
        c.speech_band.push(speech_band);
        c.zcr.push(zc as f32 / n as f32);
        c.lufs_b.push(lblock);
    }

    fn hum_score(&self) -> f32 {
        let mut buf: Vec<Complex32> = vec![Complex32::new(0.0, 0.0); HUM_FFT];
        for (i, (v, w)) in self.hum_buf.iter().zip(&self.hum_window).enumerate() {
            buf[i] = Complex32::new(v * w, 0.0);
        }
        self.hum_fft.process(&mut buf);
        let bin_hz = RATE as f32 / HUM_FFT as f32;
        let pdb = |f: f32| -> f32 {
            let (a, b) = (((f - 1.0) / bin_hz).floor() as usize, ((f + 1.0) / bin_hz).ceil() as usize);
            power_db(buf[a..=b].iter().map(|c| c.norm_sqr() as f64).fold(0.0, f64::max))
        };
        [50.0f32, 60.0]
            .iter()
            .map(|f0| {
                (1..=5)
                    .map(|h| {
                        let f = f0 * h as f32;
                        let mut neigh: Vec<f32> = (4..=20).step_by(2).flat_map(|d| [pdb(f - d as f32), pdb(f + d as f32)]).collect();
                        neigh.sort_by(f32::total_cmp);
                        smoothstep(6.0, 18.0, pdb(f) - neigh[neigh.len() / 2])
                    })
                    .sum::<f32>()
                    / 5.0
            })
            .fold(0.0, f32::max)
    }

    /// Finishes (a trailing partial block is dropped) and derives noise floor, VAD, wind and flags.
    pub fn finish(mut self) -> AudioSet {
        let last = self.last_hum;
        for k in self.hum_filled..self.cols.hum.len() {
            self.cols.hum[k] = last;
        }
        AudioSet::derive(self.cols, self.ch_rms, self.start_us)
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
