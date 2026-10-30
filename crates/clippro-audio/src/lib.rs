//! Audio analysis (SPEC P10, ALGORITHMS §4): loudness, clipping, noise floor, voice activity,
//! wind and hum per 100 ms, the audio flags, and their alignment to the video samples.

pub mod analyze;
pub mod set;

pub use analyze::{AudioAnalyzer, BLOCK, RATE};
pub use set::{Aligned, AudioSet, BLOCK_US};

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn run(ch: Vec<Vec<f32>>) -> AudioSet {
        let mut a = AudioAnalyzer::new(ch.len(), 0);
        // Feed in odd-sized pieces, as a decoder would.
        let n = ch[0].len();
        let mut i = 0;
        while i < n {
            let j = (i + 1_777).min(n);
            a.push(&ch.iter().map(|c| c[i..j].to_vec()).collect::<Vec<_>>());
            i = j;
        }
        a.finish()
    }

    fn sine(amp_dbfs: f32, secs: f32, hz: f32) -> Vec<f32> {
        let a = 10f32.powf(amp_dbfs / 20.0);
        (0..(secs * RATE as f32) as usize).map(|i| a * (TAU * hz * i as f32 / RATE as f32).sin()).collect()
    }

    /// EBU Tech 3341 test cases 1 and 2: stereo 1 kHz sine at −23 / −33 dBFS reads −23 / −33
    /// LUFS (±0.1), through our own gating of the stored momentary blocks.
    #[test]
    fn ebu_3341_cases_1_and_2() {
        for (dbfs, want) in [(-23.0f32, -23.0f32), (-33.0, -33.0)] {
            let s = sine(dbfs, 20.0, 1_000.0);
            let set = run(vec![s.clone(), s]);
            let got = set.integrated_lufs(0, 20_000_000).unwrap();
            assert!((got - want).abs() <= 0.1, "{dbfs} dBFS → {got} LUFS");
        }
    }

    #[test]
    fn flags_from_synthetic_signals() {
        // 3 s tone, 2 s clipped tone, 2 s silence, 3 s 50 Hz hum with harmonics.
        let mut x = sine(-12.0, 3.0, 440.0);
        x.extend(sine(3.5, 2.0, 440.0).iter().map(|v| v.clamp(-1.0, 1.0)));
        x.extend(vec![0.0; 2 * RATE]);
        let hum: Vec<f32> = (0..3 * RATE)
            .map(|i| (1..=5).map(|k| (0.3 / k as f32) * (TAU * 50.0 * k as f32 * i as f32 / RATE as f32).sin()).sum::<f32>())
            .collect();
        x.extend(hum);
        let set = run(vec![x.clone(), x]);
        let has = |t0: f32, t1: f32, f: clippro_types::FlagKind| {
            let (a, b) = ((t0 * 10.0) as usize, (t1 * 10.0) as usize);
            set.flags[a..b].iter().all(|x| x & f.bit() != 0)
        };
        let none = |t0: f32, t1: f32, f: clippro_types::FlagKind| {
            let (a, b) = ((t0 * 10.0) as usize, (t1 * 10.0) as usize);
            set.flags[a..b].iter().all(|x| x & f.bit() == 0)
        };
        use clippro_types::FlagKind::*;
        assert!(none(0.5, 2.9, AudioClipping) && none(0.5, 2.9, Silence) && none(0.5, 2.9, Hum));
        assert!(has(3.1, 4.9, AudioClipping));
        assert!(has(5.4, 6.8, Silence));
        assert!(has(7.5, 9.8, Hum));
        assert!(none(0.0, 9.9, DeadChannel));
    }

    #[test]
    fn dead_channel_and_codec_round_trip() {
        let s = sine(-20.0, 3.0, 1_000.0);
        let set = run(vec![s, vec![0.0; 3 * RATE]]);
        assert!(set.flags.iter().all(|f| f & clippro_types::FlagKind::DeadChannel.bit() != 0));
        let back = AudioSet::decode(&set.encode()).unwrap();
        assert_eq!(back, set);
        assert!(AudioSet::decode(b"nonsense").is_none());
    }

    #[test]
    fn alignment_to_video_samples() {
        let mut x = sine(-20.0, 2.0, 440.0);
        x.extend(vec![0.0; 2 * RATE]);
        let set = run(vec![x]);
        let times: Vec<i64> = (0..16).map(|k| k * 250_000).collect();
        let a = set.align(&times, 4.0);
        assert_eq!(a.loudness.len(), 16);
        assert!(a.clean.iter().all(|c| (*c - 1.0).abs() < 1e-6));
        assert!(a.flags[14] & clippro_types::FlagKind::Silence.bit() != 0);
        assert!(a.flags[4] & clippro_types::FlagKind::Silence.bit() == 0);
        // Beyond the audio: no data.
        let past = set.align(&[9_000_000], 4.0);
        assert!(past.loudness[0].is_nan());
    }
}
