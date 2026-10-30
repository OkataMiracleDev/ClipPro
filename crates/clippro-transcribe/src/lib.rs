//! Transcripts (SPEC AI-3, PLAN P12.3): whisper.cpp speech-to-text with word timestamps and
//! language detection, filler and flub detection (ALGORITHMS §8.2), sentence-boundary
//! snapping (§8.1), and SRT/VTT export.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const RATE: usize = 16_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Word {
    pub start_us: i64,
    pub end_us: i64,
    pub text: String,
    pub prob: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub start_us: i64,
    pub end_us: i64,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    pub language: String,
    pub words: Vec<Word>,
    pub segments: Vec<Segment>,
}

impl Transcript {
    pub fn text(&self) -> String {
        self.segments.iter().map(|s| s.text.trim()).collect::<Vec<_>>().join(" ")
    }

    /// Words overlapping [a, b) µs.
    pub fn words_in(&self, a: i64, b: i64) -> impl Iterator<Item = &Word> {
        self.words.iter().filter(move |w| w.end_us > a && w.start_us < b)
    }
}

pub fn version() -> &'static str {
    whisper_rs::get_whisper_version()
}

/// Audio per whisper run; cancellation is checked between chunks. (whisper-rs 0.16's safe
/// abort callback casts its boxed closure to the wrong type, so it isn't used.)
const CHUNK: usize = RATE * 600;

/// Transcribes mono 16 kHz audio (`audio[0]` is time 0). `language` None detects it.
pub fn transcribe(model: &Path, audio: &[f32], language: Option<&str>, threads: usize, cancel: Arc<AtomicBool>) -> Result<Transcript, String> {
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
    let ctx = WhisperContext::new_with_params(model, WhisperContextParameters::default()).map_err(|e| format!("whisper: {e}"))?;
    let mut state = ctx.create_state().map_err(|e| format!("whisper: {e}"))?;
    let mut t = Transcript::default();
    let mut lang = language.map(str::to_string);
    for (ci, chunk) in audio.chunks(CHUNK).enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        if chunk.len() < RATE / 2 {
            break;
        }
        let base = (ci * CHUNK) as i64 * 1_000_000 / RATE as i64;
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        p.set_n_threads(threads.max(1) as i32);
        p.set_language(Some(lang.as_deref().unwrap_or("auto")));
        p.set_token_timestamps(true);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_special(false);
        p.set_print_timestamps(false);
        p.set_suppress_blank(true);
        state.full(p, chunk).map_err(|e| format!("whisper: {e}"))?;
        if lang.is_none() {
            lang = whisper_rs::get_lang_str(state.full_lang_id_from_state()).map(str::to_string);
        }
        for seg in state.as_iter() {
            let text = seg.to_str_lossy().map(|s| s.into_owned()).unwrap_or_default();
            let (s0, s1) = (base + seg.start_timestamp() * 10_000, base + seg.end_timestamp() * 10_000);
            if !text.trim().is_empty() {
                t.segments.push(Segment { start_us: s0, end_us: s1, text: text.trim().to_string() });
            }
            // Tokens → words: a token starting with a space begins a new word.
            let mut first = true;
            for k in 0..seg.n_tokens() {
                let Some(tok) = seg.get_token(k) else { continue };
                let s = tok.to_str_lossy().map(|s| s.into_owned()).unwrap_or_default();
                if s.starts_with("[_") || s.starts_with("<|") || s.trim().is_empty() {
                    continue;
                }
                let d = tok.token_data();
                let (a, b) = (base + d.t0 * 10_000, base + d.t1 * 10_000);
                match t.words.last_mut() {
                    Some(w) if !first && !s.starts_with(' ') => {
                        w.text.push_str(&s);
                        w.end_us = w.end_us.max(b);
                        w.prob = w.prob.min(d.p);
                    }
                    _ => t.words.push(Word { start_us: a, end_us: b.max(a), text: s.trim().to_string(), prob: d.p }),
                }
                first = false;
            }
        }
    }
    t.language = lang.unwrap_or_default();
    Ok(t)
}

fn norm(w: &str) -> String {
    w.chars().filter(|c| c.is_alphanumeric() || *c == '\'').collect::<String>().to_lowercase()
}

/// Filler words for a language (ALGORITHMS §8.2).
fn filler_ngrams(lang: &str) -> &'static [&'static [&'static str]] {
    match lang {
        "en" | "" => &[&["um"], &["umm"], &["uh"], &["uhm"], &["erm"], &["er"], &["ah"], &["you", "know"], &["i", "mean"]],
        "de" => &[&["äh"], &["ähm"], &["öh"]],
        "fr" => &[&["euh"], &["ben"]],
        "es" => &[&["eh"], &["este"], &["o", "sea"]],
        _ => &[&["um"], &["uh"]],
    }
}

/// Indexes of filler words.
pub fn fillers(t: &Transcript) -> Vec<usize> {
    let w: Vec<String> = t.words.iter().map(|w| norm(&w.text)).collect();
    let mut out = Vec::new();
    for g in filler_ngrams(&t.language) {
        for i in 0..w.len() {
            if i + g.len() <= w.len() && g.iter().enumerate().all(|(k, x)| w[i + k] == *x) {
                out.extend(i..i + g.len());
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Flubs as (start_us, end_us): a 2–4-word phrase repeated within 2 s, or a self-correction
/// ("I— I mean", a word cut off with a dash then restarted).
pub fn flubs(t: &Transcript) -> Vec<(i64, i64)> {
    let w: Vec<String> = t.words.iter().map(|w| norm(&w.text)).collect();
    let mut out = Vec::new();
    for n in 2..=4usize {
        for i in 0..w.len().saturating_sub(2 * n - 1) {
            let a = &w[i..i + n];
            if a.iter().any(String::is_empty) {
                continue;
            }
            // The repeat starts right after, or within 2 s.
            for j in i + n..w.len().saturating_sub(n - 1) {
                if t.words[j].start_us - t.words[i + n - 1].end_us > 2_000_000 {
                    break;
                }
                if &w[j..j + n] == a {
                    out.push((t.words[i].start_us, t.words[j + n - 1].end_us));
                    break;
                }
            }
        }
    }
    for i in 0..t.words.len().saturating_sub(1) {
        let cut = t.words[i].text.ends_with('—') || t.words[i].text.ends_with("--") || t.words[i].text.ends_with('-');
        if cut && w[i] == w[i + 1] {
            out.push((t.words[i].start_us, t.words[i + 1].end_us));
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Snap points (ALGORITHMS §8.1): (start-side µs, end-side µs) pairs for each boundary — a
/// boundary after word i is "end at word i's end + 0.1 s" or "start at word i+1's start".
pub fn boundaries(t: &Transcript) -> (Vec<i64>, Vec<i64>) {
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    if let Some(f) = t.words.first() {
        starts.push(f.start_us);
    }
    for i in 0..t.words.len() {
        let w = &t.words[i];
        let sentence_end = w.text.ends_with(['.', '?', '!', '…']);
        let pause = t.words.get(i + 1).is_some_and(|n| n.start_us - w.end_us > 350_000);
        let last = i + 1 == t.words.len();
        if sentence_end || pause || last {
            ends.push(w.end_us + 100_000);
            if let Some(n) = t.words.get(i + 1) {
                starts.push(n.start_us);
            }
        }
    }
    (starts, ends)
}

/// Moves a segment's edges to the nearest boundary within ±1.5 s. `allowed(a, b)` says
/// whether the span between the old and new edge may be crossed (no force-excluded samples,
/// no hard cut); otherwise that edge stays.
pub fn snap(start_us: i64, end_us: i64, t: &Transcript, allowed: impl Fn(i64, i64) -> bool) -> (i64, i64) {
    let (starts, ends) = boundaries(t);
    let pick = |edge: i64, cands: &[i64]| -> i64 {
        cands
            .iter()
            .copied()
            .filter(|c| (c - edge).abs() <= 1_500_000 && allowed(edge.min(*c), edge.max(*c)))
            .min_by_key(|c| (c - edge).abs())
            .unwrap_or(edge)
    };
    let (a, b) = (pick(start_us, &starts), pick(end_us, &ends));
    if b > a { (a, b) } else { (start_us, end_us) }
}

fn ts(us: i64, sep: char) -> String {
    let ms = us.max(0) / 1000;
    format!("{:02}:{:02}:{:02}{sep}{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// SRT subtitles of the segments overlapping [a, b) µs, re-timed to start at `a`.
pub fn srt(t: &Transcript, a: i64, b: i64) -> String {
    let mut s = String::new();
    for (i, seg) in t.segments.iter().filter(|g| g.end_us > a && g.start_us < b).enumerate() {
        s.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, ts(seg.start_us.max(a) - a, ','), ts(seg.end_us.min(b) - a, ','), seg.text.trim()));
    }
    s
}

/// WebVTT of the segments overlapping [a, b) µs, re-timed to start at `a`.
pub fn vtt(t: &Transcript, a: i64, b: i64) -> String {
    let mut s = String::from("WEBVTT\n\n");
    for seg in t.segments.iter().filter(|g| g.end_us > a && g.start_us < b) {
        s.push_str(&format!("{} --> {}\n{}\n\n", ts(seg.start_us.max(a) - a, '.'), ts(seg.end_us.min(b) - a, '.'), seg.text.trim()));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(words: &[(&str, f64, f64)]) -> Transcript {
        let words: Vec<Word> = words.iter().map(|(t, a, b)| Word { start_us: (a * 1e6) as i64, end_us: (b * 1e6) as i64, text: t.to_string(), prob: 0.9 }).collect();
        let segments = vec![Segment { start_us: words[0].start_us, end_us: words.last().unwrap().end_us, text: words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ") }];
        Transcript { language: "en".into(), words, segments }
    }

    #[test]
    fn fillers_and_flubs() {
        let t = tr(&[("So", 0.0, 0.2), ("um,", 0.3, 0.5), ("you", 0.6, 0.7), ("know", 0.7, 0.9), ("the", 1.0, 1.1), ("the", 1.2, 1.3), ("plan", 1.4, 1.6), ("is", 1.7, 1.8), ("the", 1.9, 2.0), ("plan", 2.1, 2.4), ("I—", 3.0, 3.1), ("I", 3.2, 3.3), ("mean", 3.3, 3.5)]);
        let f = fillers(&t);
        assert_eq!(f, vec![1, 2, 3, 11, 12], "um, you know, I mean");
        let fl = flubs(&t);
        assert!(fl.iter().any(|(a, b)| *a == 1_200_000 && *b == 2_400_000), "\"the plan\" repeated: {fl:?}");
        assert!(fl.iter().any(|(a, _)| *a == 3_000_000), "self-correction: {fl:?}");
        assert!(!fl.iter().any(|(a, _)| *a == 0), "no flub at the start");
    }

    #[test]
    fn snapping_and_subtitles() {
        let t = tr(&[("Hello", 0.5, 0.9), ("there.", 1.0, 1.4), ("How", 2.0, 2.2), ("are", 2.3, 2.4), ("you?", 2.5, 2.9), ("Fine", 4.0, 4.3)]);
        // Start near "How" snaps to it; the end near "you?" snaps to its end + 0.1 s.
        assert_eq!(snap(1_700_000, 3_200_000, &t, |_, _| true), (2_000_000, 3_000_000));
        // A blocked span keeps that edge.
        assert_eq!(snap(1_700_000, 3_200_000, &t, |a, _| a >= 2_000_000), (1_700_000, 3_000_000));
        // Nothing within 1.5 s: unchanged.
        assert_eq!(snap(10_000_000, 12_000_000, &t, |_, _| true), (10_000_000, 12_000_000));
        let s = srt(&t, 500_000, 5_000_000);
        assert!(s.starts_with("1\n00:00:00,000 --> 00:00:03,800\nHello there. How are you? Fine"), "{s}");
        assert!(vtt(&t, 0, 5_000_000).starts_with("WEBVTT\n\n00:00:00.500 --> 00:00:04.300\n"));
    }

    /// With a Whisper model in CLIPPRO_MODELS: the corpus speech-like clip gives a transcript
    /// (the synthetic "speech" isn't language, so only the plumbing is checked).
    #[test]
    fn whisper_runs() {
        let Some(dir) = std::env::var_os("CLIPPRO_MODELS") else { return };
        let model = Path::new(&dir).join("ggml-base.bin");
        if std::fs::metadata(&model).map(|m| m.len()).unwrap_or(0) != 147_951_465 {
            eprintln!("whisper model missing: skipping");
            return;
        }
        // A 440 Hz tone with silence: Whisper returns quickly (often nothing).
        let audio: Vec<f32> = (0..RATE * 3).map(|i| if i < RATE { 0.1 * (i as f32 * 440.0 * std::f32::consts::TAU / RATE as f32).sin() } else { 0.0 }).collect();
        let t = transcribe(&model, &audio, Some("en"), 4, Arc::new(AtomicBool::new(false))).unwrap();
        assert_eq!(t.language, "en");
        assert!(t.words.iter().all(|w| w.end_us >= w.start_us));
        // Real speech (Windows TTS, testdata/speech/fox.wav: 16 kHz mono PCM).
        let wav = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/speech/fox.wav")).unwrap();
        let data = wav.windows(4).position(|w| w == b"data").unwrap() + 8;
        let pcm: Vec<f32> = wav[data..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect();
        let t = transcribe(&model, &pcm, None, 4, Arc::new(AtomicBool::new(false))).unwrap();
        let text = t.text().to_lowercase();
        eprintln!("{} [{}]", text, t.language);
        assert_eq!(t.language, "en", "detected");
        for w in ["quick", "brown", "fox", "lazy", "dog", "plan"] {
            assert!(text.contains(w), "{w} in {text:?}");
        }
        let fox = t.words.iter().find(|w| w.text.to_lowercase().contains("fox")).unwrap();
        let dog = t.words.iter().find(|w| w.text.to_lowercase().contains("dog")).unwrap();
        assert!(fox.start_us < dog.start_us && dog.end_us <= (pcm.len() as i64 * 1_000_000 / RATE as i64) + 500_000);
        assert!(!fillers(&t).is_empty() || !text.contains("um"), "the \"um\" is a filler when transcribed");
    }
}
