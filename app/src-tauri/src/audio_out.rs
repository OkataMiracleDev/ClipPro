//! Audio for frame-served playback (P6.1, ADR-002): a cpal output stream fed by a decoder
//! thread, whose play position is the master clock the viewer follows. Native `<video>` plays
//! its own audio and doesn't use this.

use clippro_media::audio::{decode_audio_with, AudioOptions};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How far the decoder runs ahead of the device.
const AHEAD_S: f64 = 0.4;

struct Clock {
    start_us: i64,
    rate: u32,
    /// Frames handed to the device so far.
    played: AtomicU64,
    /// At the last callback: frames handed over before it, its wall time (µs since `epoch`) and
    /// the device latency (µs).
    cb_played: AtomicU64,
    cb_at_us: AtomicI64,
    latency_us: AtomicI64,
    epoch: Instant,
    /// The decoder reached the end of the stream.
    ended: AtomicBool,
}

impl Clock {
    /// Media time (µs) of the sample being heard now.
    fn position_us(&self) -> i64 {
        let cb_at = self.cb_at_us.load(Ordering::Acquire);
        let at_cb = self.cb_played.load(Ordering::Acquire) as f64 * 1e6 / self.rate as f64;
        let since = (self.epoch.elapsed().as_micros() as i64 - cb_at).max(0) as f64;
        let handed = self.played.load(Ordering::Acquire) as f64 * 1e6 / self.rate as f64;
        let heard = (at_cb - self.latency_us.load(Ordering::Acquire) as f64 + since).clamp(0.0, handed);
        self.start_us + heard as i64
    }
}

struct Playing {
    stop: Arc<AtomicBool>,
    clock: Arc<Clock>,
}

impl Drop for Playing {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

#[derive(Default)]
pub struct AudioOut {
    playing: Mutex<Option<Playing>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl AudioOut {
    pub fn stop(&self) {
        lock(&self.playing).take();
    }

    /// Media time being heard (µs), or `None` when nothing is playing.
    pub fn position_us(&self) -> Option<i64> {
        lock(&self.playing).as_ref().map(|p| p.clock.position_us())
    }

    /// The stream ran out of audio (end of clip).
    pub fn ended(&self) -> bool {
        lock(&self.playing).as_ref().is_some_and(|p| p.clock.ended.load(Ordering::Acquire) && p.clock.position_us() >= p.clock.start_us + (p.clock.played.load(Ordering::Acquire) as f64 * 1e6 / p.clock.rate as f64) as i64)
    }

    /// Starts playing `path`'s audio from `from_us` (relative to the first video frame) at
    /// `gain` (1 = unity, 0 = silent but clocked). Returns once the device stream is running.
    pub fn play(&self, path: PathBuf, video_start_us: i64, from_us: i64, gain: f32) -> Result<(), String> {
        self.stop();
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("No audio output device")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let config = supported.config();
        let rate = config.sample_rate;
        let channels = config.channels as usize;
        let stop = Arc::new(AtomicBool::new(false));
        let clock = Arc::new(Clock {
            start_us: from_us,
            rate,
            played: AtomicU64::new(0),
            cb_played: AtomicU64::new(0),
            cb_at_us: AtomicI64::new(0),
            latency_us: AtomicI64::new(0),
            epoch: Instant::now(),
            ended: AtomicBool::new(false),
        });
        let ring: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::with_capacity((rate as f64 * AHEAD_S * 2.0) as usize * channels)));

        // Decoder: fills the ring (interleaved, device channel count) and stays AHEAD_S ahead.
        {
            let (ring, stop, clock) = (ring.clone(), stop.clone(), clock.clone());
            std::thread::Builder::new()
                .name("audio-decode".into())
                .spawn(move || {
                    let opts = AudioOptions { index: None, video_start_us, from_us: Some(from_us), out_rate: rate as i32 };
                    let limit = (rate as f64 * AHEAD_S) as usize * channels;
                    let _ = decode_audio_with(&path, &opts, |block| {
                        let n = block.first().map_or(0, Vec::len);
                        let mut inter = Vec::with_capacity(n * channels);
                        for i in 0..n {
                            for c in 0..channels {
                                // Mono to every channel; otherwise channel-for-channel (extra
                                // source channels are dropped, missing ones repeat the last).
                                let src = &block[c.min(block.len() - 1)];
                                inter.push(src[i]);
                            }
                        }
                        loop {
                            if stop.load(Ordering::Acquire) {
                                return false;
                            }
                            if lock(&ring).len() < limit {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        lock(&ring).extend(inter);
                        true
                    });
                    clock.ended.store(true, Ordering::Release);
                })
                .map_err(|e| e.to_string())?;
        }

        // Output: cpal streams aren't Send everywhere, so the stream lives on its own thread.
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        {
            let (stop, clock) = (stop.clone(), clock.clone());
            std::thread::Builder::new()
                .name("audio-out".into())
                .spawn(move || {
                    let cb_clock = clock.clone();
                    let data = move |out: &mut [f32], info: &cpal::OutputCallbackInfo| {
                        let ts = info.timestamp();
                        let latency = ts.playback.duration_since(ts.callback).as_micros() as i64;
                        let before = cb_clock.played.load(Ordering::Acquire);
                        let mut ring = lock(&ring);
                        let take = out.len().min(ring.len() - ring.len() % channels);
                        for (o, s) in out.iter_mut().zip(ring.drain(..take)) {
                            *o = s * gain;
                        }
                        drop(ring);
                        out[take..].fill(0.0);
                        cb_clock.cb_played.store(before, Ordering::Release);
                        cb_clock.latency_us.store(latency, Ordering::Release);
                        cb_clock.cb_at_us.store(cb_clock.epoch.elapsed().as_micros() as i64, Ordering::Release);
                        cb_clock.played.store(before + (take / channels) as u64, Ordering::Release);
                    };
                    let stream = match device.build_output_stream::<f32, _, _>(config, data, |e| eprintln!("audio output: {e}"), None) {
                        Ok(s) => s,
                        Err(e) => {
                            let _ = ready_tx.send(Err(format!("Audio output: {e}")));
                            return;
                        }
                    };
                    if let Err(e) = stream.play() {
                        let _ = ready_tx.send(Err(format!("Audio output: {e}")));
                        return;
                    }
                    let _ = ready_tx.send(Ok(()));
                    while !stop.load(Ordering::Acquire) {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    drop(stream);
                })
                .map_err(|e| e.to_string())?;
        }
        ready_rx.recv_timeout(Duration::from_secs(3)).map_err(|_| "Audio output didn't start".to_string())??;
        *lock(&self.playing) = Some(Playing { stop, clock });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PLAN P6.1: the audio clock tracks real time (A/V drift < 1 frame); runs only with an
    /// output device and `CLIPPRO_PERF=1` (it plays the corpus tone for a few seconds).
    #[test]
    fn audio_clock_tracks_wall_time() {
        if std::env::var("CLIPPRO_PERF").as_deref() != Ok("1") {
            return;
        }
        let clip = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/gen/audio_mix.mp4");
        if !clip.exists() || cpal::default_host().default_output_device().is_none() {
            eprintln!("no corpus or no output device: skipping");
            return;
        }
        let out = AudioOut::default();
        out.play(clip, 0, 1_000_000, 0.0).unwrap(); // silent: only the clock matters here
        std::thread::sleep(Duration::from_millis(500)); // device warm-up
        let (p0, t0) = (out.position_us().unwrap(), Instant::now());
        std::thread::sleep(Duration::from_secs(5));
        let (p1, t1) = (out.position_us().unwrap(), Instant::now());
        out.stop();
        let media = (p1 - p0) as f64;
        let wall = (t1 - t0).as_micros() as f64;
        eprintln!("audio clock advanced {media:.0} µs in {wall:.0} µs wall ({:+.0} µs)", media - wall);
        assert!((media - wall).abs() < 40_000.0, "drift {} µs over 5 s", media - wall);
        assert!(p0 >= 1_000_000);
    }
}
