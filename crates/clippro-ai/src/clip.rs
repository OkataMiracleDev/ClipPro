//! CLIP ViT-B/32 (SPEC AI-4/5/6, ALGORITHMS §8.6–8.7): image and text embeddings, zero-shot
//! shot type and setting, and a prompt-contrast aesthetic score.

use crate::models::installed_path;
use crate::runtime::session;
use ndarray::{Array2, Array4};
use ort::session::Session;
use ort::value::Tensor;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DIM: usize = 512;
const SIZE: usize = 224;
const MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
/// Image batch size (fixed, see `embed_images`).
pub const BATCH: usize = 8;
const STD: [f32; 3] = [0.268_629_54, 0.261_302_58, 0.275_777_1];

/// Shot classes (ALGORITHMS §8.6), in the order of `SHOT_PROMPTS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShotType {
    Wide,
    Medium,
    CloseUp,
    ExtremeCloseUp,
    Insert,
    Aerial,
    OverTheShoulder,
}

impl ShotType {
    pub const ALL: [ShotType; 7] = [ShotType::Wide, ShotType::Medium, ShotType::CloseUp, ShotType::ExtremeCloseUp, ShotType::Insert, ShotType::Aerial, ShotType::OverTheShoulder];
    pub fn label(self) -> &'static str {
        match self {
            ShotType::Wide => "Wide",
            ShotType::Medium => "Medium",
            ShotType::CloseUp => "Close-up",
            ShotType::ExtremeCloseUp => "Extreme close-up",
            ShotType::Insert => "Insert",
            ShotType::Aerial => "Aerial",
            ShotType::OverTheShoulder => "Over the shoulder",
        }
    }
}

const SHOT_PROMPTS: [&str; 7] = [
    "a wide shot of a scene",
    "a medium shot of a person",
    "a close-up shot of a face",
    "an extreme close-up of an eye or detail",
    "an insert shot of an object",
    "an aerial drone shot",
    "an over-the-shoulder shot",
];
const SETTING_PROMPTS: [&str; 4] = ["a photo taken indoors", "a photo taken outdoors", "a photo taken during the day", "a photo taken at night"];
const AESTHETIC_PROMPTS: [&str; 2] = ["a beautiful, well-composed photograph", "a dull, poorly composed photograph"];

// ---------- Tokenizer (CLIP BPE, from tokenizer.json) ----------

pub struct Tokenizer {
    vocab: HashMap<String, i64>,
    ranks: HashMap<(String, String), usize>,
    byte_enc: [char; 256],
    bos: i64,
    eos: i64,
}

/// GPT-2/CLIP byte → printable-char table.
fn bytes_to_unicode() -> [char; 256] {
    let mut bs: Vec<u32> = (b'!' as u32..=b'~' as u32).chain(0xA1..=0xAC).chain(0xAE..=0xFF).collect();
    let mut cs = bs.clone();
    let mut n = 0;
    for b in 0..256u32 {
        if !bs.contains(&b) {
            bs.push(b);
            cs.push(256 + n);
            n += 1;
        }
    }
    let mut out = [' '; 256];
    for (b, c) in bs.iter().zip(cs) {
        out[*b as usize] = char::from_u32(c).unwrap_or(' ');
    }
    out
}

impl Tokenizer {
    pub fn from_json(text: &str) -> Result<Tokenizer, String> {
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("tokenizer: {e}"))?;
        let model = &v["model"];
        let vocab: HashMap<String, i64> = model["vocab"].as_object().ok_or("tokenizer: no vocab")?.iter().filter_map(|(k, v)| Some((k.clone(), v.as_i64()?))).collect();
        let mut ranks = HashMap::new();
        for (i, m) in model["merges"].as_array().ok_or("tokenizer: no merges")?.iter().enumerate() {
            let pair = match m {
                serde_json::Value::String(s) => s.split_once(' ').map(|(a, b)| (a.to_string(), b.to_string())),
                serde_json::Value::Array(a) => match (a.first().and_then(|x| x.as_str()), a.get(1).and_then(|x| x.as_str())) {
                    (Some(x), Some(y)) => Some((x.to_string(), y.to_string())),
                    _ => None,
                },
                _ => None,
            };
            if let Some(p) = pair {
                ranks.insert(p, i);
            }
        }
        let bos = *vocab.get("<|startoftext|>").ok_or("tokenizer: no BOS")?;
        let eos = *vocab.get("<|endoftext|>").ok_or("tokenizer: no EOS")?;
        Ok(Tokenizer { vocab, ranks, byte_enc: bytes_to_unicode(), bos, eos })
    }

    /// Words as CLIP splits them: letter runs, single digits, punctuation runs; lowercased.
    fn words(text: &str) -> Vec<String> {
        let t = text.to_lowercase();
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut kind = 0u8; // 1 letters, 3 other
        let flush = |cur: &mut String, out: &mut Vec<String>| {
            if !cur.is_empty() {
                out.push(std::mem::take(cur));
            }
        };
        for c in t.chars() {
            let k = if c.is_alphabetic() { 1 } else if c.is_numeric() { 2 } else if c.is_whitespace() { 0 } else { 3 };
            if k == 0 || k == 2 || k != kind {
                flush(&mut cur, &mut out);
            }
            if k == 2 {
                out.push(c.to_string());
            } else if k != 0 {
                cur.push(c);
            }
            kind = k;
        }
        flush(&mut cur, &mut out);
        out
    }

    fn bpe(&self, word: &str) -> Vec<String> {
        let chars: Vec<char> = word.bytes().map(|b| self.byte_enc[b as usize]).collect();
        let mut parts: Vec<String> = chars.iter().map(|c| c.to_string()).collect();
        if let Some(last) = parts.last_mut() {
            last.push_str("</w>");
        }
        loop {
            let best = parts.windows(2).enumerate().filter_map(|(i, w)| self.ranks.get(&(w[0].clone(), w[1].clone())).map(|r| (*r, i))).min();
            let Some((_, i)) = best else { break };
            let merged = format!("{}{}", parts[i], parts[i + 1]);
            parts.splice(i..i + 2, [merged]);
        }
        parts
    }

    /// BOS + tokens + EOS, truncated to 77.
    pub fn encode(&self, text: &str) -> Vec<i64> {
        let mut ids = vec![self.bos];
        for w in Self::words(text) {
            for p in self.bpe(&w) {
                if let Some(id) = self.vocab.get(&p) {
                    ids.push(*id);
                }
            }
        }
        ids.truncate(76);
        ids.push(self.eos);
        ids
    }
}

// ---------- Image preprocessing ----------

/// RGB8 frame → CLIP input (3×224×224, normalised): shorter side to 224 (bilinear), centre crop.
pub fn preprocess(rgb: &[u8], w: usize, h: usize) -> Vec<f32> {
    let scale = SIZE as f32 / w.min(h).max(1) as f32;
    let (sw, sh) = ((w as f32 * scale).round() as usize, (h as f32 * scale).round() as usize);
    let (ox, oy) = ((sw.saturating_sub(SIZE)) / 2, (sh.saturating_sub(SIZE)) / 2);
    let mut out = vec![0f32; 3 * SIZE * SIZE];
    for y in 0..SIZE {
        let fy = ((y + oy) as f32 + 0.5) / scale - 0.5;
        let y0 = (fy.floor().max(0.0) as usize).min(h - 1);
        let y1 = (y0 + 1).min(h - 1);
        let ty = (fy - y0 as f32).clamp(0.0, 1.0);
        for x in 0..SIZE {
            let fx = ((x + ox) as f32 + 0.5) / scale - 0.5;
            let x0 = (fx.floor().max(0.0) as usize).min(w - 1);
            let x1 = (x0 + 1).min(w - 1);
            let tx = (fx - x0 as f32).clamp(0.0, 1.0);
            for c in 0..3 {
                let p = |xx: usize, yy: usize| rgb[(yy * w + xx) * 3 + c] as f32;
                let v = (p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx) * (1.0 - ty) + (p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx) * ty;
                out[c * SIZE * SIZE + y * SIZE + x] = (v / 255.0 - MEAN[c]) / STD[c];
            }
        }
    }
    out
}

fn l2(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    v.iter_mut().for_each(|x| *x /= n);
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn softmax100(sims: &[f32]) -> Vec<f32> {
    let m = sims.iter().fold(f32::MIN, |a, b| a.max(*b));
    let e: Vec<f32> = sims.iter().map(|s| ((s - m) * 100.0).exp()).collect();
    let z: f32 = e.iter().sum();
    e.iter().map(|x| x / z).collect()
}

/// Per-image zero-shot results.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    /// Probability per `ShotType::ALL`.
    pub shot: Vec<f32>,
    pub outdoor: f32,
    pub night: f32,
    /// 0–10 (prompt contrast; ADR-005).
    pub aesthetic: f32,
}

pub struct Clip {
    vision: Session,
    text: Session,
    tok: Tokenizer,
    prompts: Vec<Vec<f32>>,
}

impl Clip {
    /// Loads the installed CLIP models.
    pub fn load(deterministic: bool) -> Result<Clip, String> {
        let need = |id: &str| installed_path(id).ok_or_else(|| format!("the {id} model isn't installed"));
        let vision = session(&need("clip-b32-vision")?, deterministic)?;
        let text = session(&need("clip-b32-text")?, deterministic)?;
        let tok = Tokenizer::from_json(&std::fs::read_to_string(need("clip-b32-tokenizer")?).map_err(|e| e.to_string())?)?;
        let mut c = Clip { vision, text, tok, prompts: Vec::new() };
        let all: Vec<&str> = SHOT_PROMPTS.iter().chain(&SETTING_PROMPTS).chain(&AESTHETIC_PROMPTS).copied().collect();
        c.prompts = c.embed_texts(&all)?;
        Ok(c)
    }

    /// Unit-length image embeddings for preprocessed images (`preprocess`).
    pub fn embed_images(&mut self, images: &[Vec<f32>]) -> Result<Vec<Vec<f32>>, String> {
        if images.is_empty() {
            return Ok(Vec::new());
        }
        if images.len() > BATCH {
            let mut out = Vec::with_capacity(images.len());
            for c in images.chunks(BATCH) {
                out.extend(self.embed_images(c)?);
            }
            return Ok(out);
        }
        // Always a full batch: a new input shape makes the GPU provider recompile (seconds).
        let n = images.len();
        let mut flat: Vec<f32> = Vec::with_capacity(BATCH * 3 * SIZE * SIZE);
        for k in 0..BATCH {
            flat.extend_from_slice(&images[k.min(n - 1)]);
        }
        let arr = Array4::from_shape_vec((BATCH, 3, SIZE, SIZE), flat).map_err(|e| e.to_string())?;
        let input = Tensor::from_array(arr).map_err(|e| e.to_string())?;
        let out = self.vision.run(ort::inputs!["pixel_values" => input]).map_err(|e| e.to_string())?;
        let (shape, data) = out["image_embeds"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        let d = *shape.last().unwrap_or(&(DIM as i64)) as usize;
        Ok(data.chunks(d).take(n).map(|c| {
            let mut v = c.to_vec();
            l2(&mut v);
            v
        }).collect())
    }

    /// Unit-length text embeddings.
    pub fn embed_texts(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
        let ids: Vec<Vec<i64>> = texts.iter().map(|t| self.tok.encode(t)).collect();
        let len = ids.iter().map(Vec::len).max().unwrap_or(2);
        let mut flat = Vec::with_capacity(ids.len() * len);
        let mut mask = Vec::with_capacity(ids.len() * len);
        for v in &ids {
            flat.extend(v.iter().copied().chain(std::iter::repeat(self.tok.eos).take(len - v.len())));
            mask.extend(std::iter::repeat(1i64).take(v.len()).chain(std::iter::repeat(0).take(len - v.len())));
        }
        let input = Tensor::from_array(Array2::from_shape_vec((ids.len(), len), flat).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let wants_mask = self.text.inputs().iter().any(|i| i.name() == "attention_mask");
        let out = if wants_mask {
            let m = Tensor::from_array(Array2::from_shape_vec((ids.len(), len), mask).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            self.text.run(ort::inputs!["input_ids" => input, "attention_mask" => m])
        } else {
            self.text.run(ort::inputs!["input_ids" => input])
        }
        .map_err(|e| e.to_string())?;
        let (shape, data) = out["text_embeds"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        let d = *shape.last().unwrap_or(&(DIM as i64)) as usize;
        Ok(data.chunks(d).map(|c| {
            let mut v = c.to_vec();
            l2(&mut v);
            v
        }).collect())
    }

    /// Zero-shot shot type, setting and aesthetic for an image embedding.
    pub fn scene(&self, emb: &[f32]) -> Scene {
        let sims: Vec<f32> = self.prompts.iter().map(|p| cosine(emb, p)).collect();
        let shot = softmax100(&sims[..7]);
        let io = softmax100(&sims[7..9]);
        let dn = softmax100(&sims[9..11]);
        let ae = softmax100(&sims[11..13]);
        Scene { shot, outdoor: io[1], night: dn[1], aesthetic: 10.0 * ae[0] }
    }
}

/// Fuses face size into shot probabilities (ALGORITHMS §8.6) and renormalises.
pub fn fuse_face_size(shot: &[f32], largest_face: f32) -> Vec<f32> {
    let mut v = shot.to_vec();
    if largest_face > 0.12 {
        v[ShotType::CloseUp as usize] += 0.2;
    }
    if largest_face > 0.35 {
        v[ShotType::ExtremeCloseUp as usize] += 0.2;
    }
    let z: f32 = v.iter().sum();
    v.iter_mut().for_each(|x| *x /= z.max(1e-9));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_split_like_clip() {
        assert_eq!(Tokenizer::words("A Sunset, over 2 lakes!"), ["a", "sunset", ",", "over", "2", "lakes", "!"]);
        let b = bytes_to_unicode();
        assert_eq!(b[b'a' as usize], 'a');
        assert_eq!(b[b' ' as usize], 'Ġ');
    }

    #[test]
    fn preprocess_shape_and_normalisation() {
        let (w, h) = (320, 180);
        let rgb: Vec<u8> = (0..w * h).flat_map(|_| [255u8, 0, 128]).collect();
        let p = preprocess(&rgb, w, h);
        assert_eq!(p.len(), 3 * 224 * 224);
        assert!((p[0] - (1.0 - MEAN[0]) / STD[0]).abs() < 1e-3);
        assert!((p[224 * 224] - (0.0 - MEAN[1]) / STD[1]).abs() < 1e-3);
        let f = fuse_face_size(&[0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0], 0.2);
        assert!(f[2] > 0.15 && (f.iter().sum::<f32>() - 1.0).abs() < 1e-5);
    }

    /// With the models installed (CLIPPRO_MODELS), text and image embeddings agree on content.
    #[test]
    fn zero_shot_with_installed_models() {
        let Ok(mut c) = Clip::load(true) else {
            eprintln!("CLIP models not installed: skipping");
            return;
        };
        assert_eq!(c.tok.encode("a photo of a cat")[0], 49406);
        // A flat blue image matches "the sky" better than "a dog".
        let (w, h) = (300, 200);
        let sky: Vec<u8> = (0..w * h).flat_map(|i| { let y = (i / w) as u8; [90u8.saturating_add(y / 4), 150, 235] }).collect();
        let img = c.embed_images(&[preprocess(&sky, w, h)]).unwrap();
        let t = c.embed_texts(&["a photo of a clear blue sky", "a photo of a dog"]).unwrap();
        assert!(cosine(&img[0], &t[0]) > cosine(&img[0], &t[1]), "{} vs {}", cosine(&img[0], &t[0]), cosine(&img[0], &t[1]));
        let s = c.scene(&img[0]);
        assert_eq!(s.shot.len(), 7);
        assert!((s.shot.iter().sum::<f32>() - 1.0).abs() < 1e-4);
        assert!((0.0..=10.0).contains(&s.aesthetic));
    }
}
