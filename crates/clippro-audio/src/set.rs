//! Audio measurements for a clip (100 ms rows), the derived voice activity, wind, noise floor and
//! flags, alignment to the video samples for scoring, and the stored binary form.

use crate::analyze::smoothstep;
use clippro_types::FlagKind;

pub const BLOCK_US: i64 = 100_000;

/// Measured columns, one value per 100 ms block.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cols {
    pub peak_dbfs: Vec<f32>,
    pub rms_dbfs: Vec<f32>,
    pub true_peak_dbtp: Vec<f32>,
    pub lufs_m: Vec<f32>,
    pub lufs_s: Vec<f32>,
    pub clip_events: Vec<f32>,
    pub lf_ratio: Vec<f32>,
    pub flatness: Vec<f32>,
    pub flux_lf: Vec<f32>,
    pub hum: Vec<f32>,
    pub speech_band: Vec<f32>,
    pub zcr: Vec<f32>,
    /// K-weighted loudness of the 100 ms block itself (LUFS); silence boundaries use it.
    pub lufs_b: Vec<f32>,
}

impl Cols {
    fn all(&self) -> [&Vec<f32>; 13] {
        [&self.peak_dbfs, &self.rms_dbfs, &self.true_peak_dbtp, &self.lufs_m, &self.lufs_s, &self.clip_events, &self.lf_ratio, &self.flatness, &self.flux_lf, &self.hum, &self.speech_band, &self.zcr, &self.lufs_b]
    }
    fn all_mut(&mut self) -> [&mut Vec<f32>; 13] {
        [
            &mut self.peak_dbfs,
            &mut self.rms_dbfs,
            &mut self.true_peak_dbtp,
            &mut self.lufs_m,
            &mut self.lufs_s,
            &mut self.clip_events,
            &mut self.lf_ratio,
            &mut self.flatness,
            &mut self.flux_lf,
            &mut self.hum,
            &mut self.speech_band,
            &mut self.zcr,
            &mut self.lufs_b,
        ]
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioSet {
    /// Time of block 0 relative to the first video frame (µs).
    pub start_us: i64,
    pub cols: Cols,
    /// Per-channel RMS (dBFS) per block.
    pub ch_rms: Vec<Vec<f32>>,
    pub noise_floor: Vec<f32>,
    pub vad: Vec<f32>,
    pub wind: Vec<f32>,
    /// Audio flag bits (`FlagKind::bit`) per block.
    pub flags: Vec<u32>,
}

/// Audio columns aligned to the video samples, normalised for scoring (ALGORITHMS §3 table).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Aligned {
    /// Short-term loudness (LUFS; target-type sub-metric).
    pub loudness: Vec<f32>,
    pub clean: Vec<f32>,
    pub noise_floor: Vec<f32>,
    pub speech: Vec<f32>,
    pub wind: Vec<f32>,
    pub hum: Vec<f32>,
    pub flags: Vec<u32>,
}

fn sustained(raw: &[bool], min: usize) -> Vec<bool> {
    let mut out = vec![false; raw.len()];
    let mut i = 0;
    while i < raw.len() {
        if !raw[i] {
            i += 1;
            continue;
        }
        let a = i;
        while i < raw.len() && raw[i] {
            i += 1;
        }
        if i - a >= min {
            out[a..i].iter_mut().for_each(|x| *x = true);
        }
    }
    out
}

impl AudioSet {
    pub fn len(&self) -> usize {
        self.cols.rms_dbfs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn derive(cols: Cols, ch_rms: Vec<Vec<f32>>, start_us: i64) -> AudioSet {
        let n = cols.rms_dbfs.len();
        // Noise floor: P10 of RMS over ±5 s.
        let noise_floor: Vec<f32> = (0..n)
            .map(|k| {
                let (a, b) = (k.saturating_sub(50), (k + 51).min(n));
                let mut w: Vec<f32> = cols.rms_dbfs[a..b].to_vec();
                w.sort_by(f32::total_cmp);
                w[(w.len() - 1) / 10]
            })
            .collect();
        // Voice activity, with a 300 ms hangover and 3-of-5 majority smoothing.
        let raw: Vec<bool> = (0..n)
            .map(|k| cols.rms_dbfs[k] > noise_floor[k] + 6.0 && cols.flatness[k] < 0.5 && cols.speech_band[k] > 0.5 && (0.02..=0.35).contains(&cols.zcr[k]))
            .collect();
        let mut hang = vec![false; n];
        let mut left = 0;
        for k in 0..n {
            if raw[k] {
                left = 3;
                hang[k] = true;
            } else if left > 0 {
                left -= 1;
                hang[k] = true;
            }
        }
        let vad: Vec<f32> = (0..n)
            .map(|k| {
                let (a, b) = (k.saturating_sub(2), (k + 3).min(n));
                let on = hang[a..b].iter().filter(|x| **x).count();
                if on * 2 > b - a { 1.0 } else { 0.0 }
            })
            .collect();
        // Wind: low-frequency energy that gusts (variable LF flux over 1 s), not steady hum.
        let wind: Vec<f32> = (0..n)
            .map(|k| {
                if cols.rms_dbfs[k] <= -45.0 {
                    return 0.0;
                }
                let (a, b) = (k.saturating_sub(5), (k + 5).min(n));
                let w = &cols.flux_lf[a..b];
                let m = w.iter().sum::<f32>() / w.len() as f32;
                let var = (w.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / w.len() as f32).sqrt();
                // Calibrated on the corpus wind (ALGORITHMS §4 cal values).
                smoothstep(0.3, 0.55, cols.lf_ratio[k]) * smoothstep(0.1, 0.35, m + var) * (1.0 - cols.hum[k])
            })
            .collect();
        // Flags.
        let mut flags = vec![0u32; n];
        let mut set = |raw: Vec<bool>, f: FlagKind| {
            for (k, on) in raw.into_iter().enumerate() {
                if on {
                    flags[k] |= f.bit();
                }
            }
        };
        set((0..n).map(|k| cols.clip_events[k] >= 1.0 || cols.true_peak_dbtp[k] > -0.1).collect(), FlagKind::AudioClipping);
        set(sustained(&(0..n).map(|k| cols.lufs_b[k] < -50.0).collect::<Vec<_>>(), 10), FlagKind::Silence);
        set(sustained(&wind.iter().map(|w| *w > 0.5).collect::<Vec<_>>(), 5), FlagKind::Wind);
        set(sustained(&cols.hum.iter().map(|h| *h > 0.5).collect::<Vec<_>>(), 20), FlagKind::Hum);
        if ch_rms.len() >= 2 && n > 0 {
            let mean = |c: &Vec<f32>| {
                let e: f64 = c.iter().map(|d| 10f64.powf(*d as f64 / 10.0)).sum::<f64>() / c.len().max(1) as f64;
                (10.0 * e.max(1e-24).log10()) as f32
            };
            let means: Vec<f32> = ch_rms.iter().map(mean).collect();
            let dead = means.iter().any(|m| *m < -80.0) && means.iter().any(|m| *m > -60.0);
            set(vec![dead; n], FlagKind::DeadChannel);
            if !dead {
                set(sustained(&(0..n).map(|k| (ch_rms[0][k] - ch_rms[1][k]).abs() > 10.0).collect::<Vec<_>>(), 50), FlagKind::ChannelImbalance);
            }
        }
        AudioSet { start_us, cols, ch_rms, noise_floor, vad, wind, flags }
    }

    fn block_range(&self, from_us: i64, to_us: i64) -> std::ops::Range<usize> {
        let n = self.len() as i64;
        let a = ((from_us - self.start_us) as f64 / BLOCK_US as f64).floor().clamp(0.0, n as f64) as usize;
        let b = ((to_us - self.start_us) as f64 / BLOCK_US as f64).ceil().clamp(0.0, n as f64) as usize;
        a..b.max(a)
    }

    /// BS.1770 integrated loudness over [from, to) from the stored momentary values (400 ms
    /// windows every 100 ms = the standard's 75 %-overlapped gating blocks). `None` when silent.
    pub fn integrated_lufs(&self, from_us: i64, to_us: i64) -> Option<f32> {
        let r = self.block_range(from_us, to_us);
        let e: Vec<f64> = self.cols.lufs_m[r].iter().filter(|l| **l > -70.0).map(|l| 10f64.powf((*l as f64 + 0.691) / 10.0)).collect();
        if e.is_empty() {
            return None;
        }
        let ungated = -0.691 + 10.0 * (e.iter().sum::<f64>() / e.len() as f64).log10();
        let rel = ungated - 10.0;
        let g: Vec<f64> = e.into_iter().filter(|z| -0.691 + 10.0 * z.log10() > rel).collect();
        (!g.is_empty()).then(|| (-0.691 + 10.0 * (g.iter().sum::<f64>() / g.len() as f64).log10()) as f32)
    }

    /// Columns for the video samples at `times_us`, each covering ± half the sample interval.
    pub fn align(&self, times_us: &[i64], rate_hz: f64) -> Aligned {
        let half = (5e5 / rate_hz.max(0.1)) as i64;
        let mut out = Aligned::default();
        for &t in times_us {
            let r = self.block_range(t - half, t + half);
            let at = (((t - self.start_us) / BLOCK_US) as usize).min(self.len().saturating_sub(1));
            if self.is_empty() || r.is_empty() {
                for v in [&mut out.loudness, &mut out.clean, &mut out.noise_floor, &mut out.speech, &mut out.wind, &mut out.hum] {
                    v.push(f32::NAN);
                }
                out.flags.push(0);
                continue;
            }
            let mean = |c: &[f32]| c[r.clone()].iter().sum::<f32>() / r.len() as f32;
            let max = |c: &[f32]| c[r.clone()].iter().cloned().fold(f32::MIN, f32::max);
            out.loudness.push(self.cols.lufs_s[at].max(-70.0));
            // Clip events per second in ±0.5 s.
            let cr = self.block_range(t - 500_000, t + 500_000);
            let per_s = self.cols.clip_events[cr.clone()].iter().sum::<f32>() / (cr.len().max(1) as f32 / 10.0);
            out.clean.push((-per_s / 0.5).exp());
            out.noise_floor.push(smoothstep(-70.0, -35.0, mean(&self.noise_floor)));
            out.speech.push(mean(&self.vad));
            out.wind.push(max(&self.wind));
            out.hum.push(max(&self.cols.hum));
            out.flags.push(self.flags[r].iter().fold(0, |a, f| a | f));
        }
        out
    }

    pub fn encode(&self) -> Vec<u8> {
        let n = self.len();
        let mut b = Vec::with_capacity(16 + n * 4 * (16 + self.ch_rms.len()));
        b.extend_from_slice(b"CPA1");
        b.extend_from_slice(&self.start_us.to_le_bytes());
        b.extend_from_slice(&(n as u32).to_le_bytes());
        b.push(self.ch_rms.len() as u8);
        let mut put = |v: &[f32]| v.iter().for_each(|x| b.extend_from_slice(&x.to_le_bytes()));
        for c in self.cols.all() {
            put(c);
        }
        for c in &self.ch_rms {
            put(c);
        }
        put(&self.noise_floor);
        put(&self.vad);
        put(&self.wind);
        for f in &self.flags {
            b.extend_from_slice(&f.to_le_bytes());
        }
        zstd::encode_all(&b[..], 3).unwrap_or(b)
    }

    pub fn decode(bytes: &[u8]) -> Option<AudioSet> {
        let b = zstd::decode_all(bytes).ok()?;
        if b.get(..4)? != b"CPA1" {
            return None;
        }
        let start_us = i64::from_le_bytes(b.get(4..12)?.try_into().ok()?);
        let n = u32::from_le_bytes(b.get(12..16)?.try_into().ok()?) as usize;
        let chans = *b.get(16)? as usize;
        let mut pos = 17;
        let take = |pos: &mut usize| -> Option<Vec<f32>> {
            let s = b.get(*pos..*pos + n * 4)?;
            *pos += n * 4;
            Some(s.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
        };
        let mut cols = Cols::default();
        for c in cols.all_mut() {
            *c = take(&mut pos)?;
        }
        let ch_rms = (0..chans).map(|_| take(&mut pos)).collect::<Option<Vec<_>>>()?;
        let noise_floor = take(&mut pos)?;
        let vad = take(&mut pos)?;
        let wind = take(&mut pos)?;
        let flags = b.get(pos..pos + n * 4)?.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        Some(AudioSet { start_us, cols, ch_rms, noise_floor, vad, wind, flags })
    }
}
