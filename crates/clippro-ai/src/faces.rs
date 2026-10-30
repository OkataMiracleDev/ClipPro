//! Faces (SPEC AI-2, ALGORITHMS §8.5): YuNet detection with 5-point landmarks, alignment to
//! 112×112, SFace embeddings, per-face focus and size, tracks across samples and People
//! clustering.

use crate::models::installed_path;
use crate::runtime::session;
use ndarray::Array4;
use ort::session::Session;
use ort::value::Tensor;
use serde::{Deserialize, Serialize};

const DET: usize = 640;
const MIN_SCORE: f32 = 0.6;
const NMS_IOU: f32 = 0.4;
/// ArcFace/SFace 112×112 landmark template (eyes, nose, mouth corners).
const TEMPLATE: [[f32; 2]; 5] = [[38.2946, 51.6963], [73.5318, 51.5014], [56.0252, 71.7366], [41.5493, 92.3655], [70.7299, 92.2041]];
/// SFace same-person cosine similarity is ≥ 0.363 (OpenCV); People clustering cuts at
/// cosine distance 0.6 (calibrated for SFace; ALGORITHMS §8.5 uses 0.45 for ArcFace).
pub const CLUSTER_DISTANCE: f32 = 0.6;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Face {
    /// x, y, w, h in frame pixels.
    pub bbox: [f32; 4],
    pub score: f32,
    pub landmarks: [[f32; 2]; 5],
    /// Unit-length SFace embedding (128).
    pub embedding: Vec<f32>,
    /// log10 mean squared Laplacian of luma ([0,1]) inside the box.
    pub sharpness: f32,
    /// Box area / frame area.
    pub size: f32,
}

pub struct FaceEngine {
    det: Session,
    rec: Session,
}

fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let (x1, y1) = (a[0].max(b[0]), a[1].max(b[1]));
    let (x2, y2) = ((a[0] + a[2]).min(b[0] + b[2]), (a[1] + a[3]).min(b[1] + b[3]));
    let inter = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let u = a[2] * a[3] + b[2] * b[3] - inter;
    if u > 0.0 { inter / u } else { 0.0 }
}

/// Similarity transform (scale·rotation + translation) mapping `src` onto `dst`, least squares.
/// Returns [a, b, tx, ty] for x' = a·x − b·y + tx, y' = b·x + a·y + ty.
pub fn similarity(src: &[[f32; 2]; 5], dst: &[[f32; 2]; 5]) -> [f32; 4] {
    let n = src.len() as f32;
    let (mut sx, mut sy, mut dx, mut dy) = (0.0, 0.0, 0.0, 0.0);
    for i in 0..5 {
        sx += src[i][0];
        sy += src[i][1];
        dx += dst[i][0];
        dy += dst[i][1];
    }
    let (sx, sy, dx, dy) = (sx / n, sy / n, dx / n, dy / n);
    let (mut num_a, mut num_b, mut den) = (0.0, 0.0, 0.0);
    for i in 0..5 {
        let (x, y) = (src[i][0] - sx, src[i][1] - sy);
        let (u, v) = (dst[i][0] - dx, dst[i][1] - dy);
        num_a += x * u + y * v;
        num_b += x * v - y * u;
        den += x * x + y * y;
    }
    let (a, b) = if den > 0.0 { (num_a / den, num_b / den) } else { (1.0, 0.0) };
    [a, b, dx - (a * sx - b * sy), dy - (b * sx + a * sy)]
}

/// Warps the face into SFace's 112×112 RGB input (raw 0–255, NCHW).
fn align(rgb: &[u8], w: usize, h: usize, lm: &[[f32; 2]; 5]) -> Vec<f32> {
    let [a, b, tx, ty] = similarity(lm, &TEMPLATE);
    // Inverse: x = (a(u−tx) + b(v−ty)) / (a²+b²), y = (−b(u−tx) + a(v−ty)) / (a²+b²).
    let d = (a * a + b * b).max(1e-9);
    let mut out = vec![0f32; 3 * 112 * 112];
    for v in 0..112 {
        for u in 0..112 {
            let (uu, vv) = (u as f32 - tx, v as f32 - ty);
            let (x, y) = ((a * uu + b * vv) / d, (-b * uu + a * vv) / d);
            if x < 0.0 || y < 0.0 || x >= (w - 1) as f32 || y >= (h - 1) as f32 {
                continue;
            }
            let (x0, y0) = (x as usize, y as usize);
            let (fx, fy) = (x - x0 as f32, y - y0 as f32);
            for c in 0..3 {
                let p = |xx: usize, yy: usize| rgb[(yy * w + xx) * 3 + c] as f32;
                let val = (p(x0, y0) * (1.0 - fx) + p(x0 + 1, y0) * fx) * (1.0 - fy) + (p(x0, y0 + 1) * (1.0 - fx) + p(x0 + 1, y0 + 1) * fx) * fy;
                out[c * 112 * 112 + v * 112 + u] = val;
            }
        }
    }
    out
}

/// log10 of the mean squared 4-neighbour Laplacian of luma inside a box.
fn box_sharpness(rgb: &[u8], w: usize, h: usize, b: &[f32; 4]) -> f32 {
    let x0 = (b[0].max(1.0) as usize).min(w - 2);
    let y0 = (b[1].max(1.0) as usize).min(h - 2);
    let x1 = ((b[0] + b[2]) as usize).clamp(x0 + 1, w - 1);
    let y1 = ((b[1] + b[3]) as usize).clamp(y0 + 1, h - 1);
    let l = |x: usize, y: usize| {
        let i = (y * w + x) * 3;
        (0.299 * rgb[i] as f32 + 0.587 * rgb[i + 1] as f32 + 0.114 * rgb[i + 2] as f32) / 255.0
    };
    let mut acc = 0f64;
    let mut n = 0usize;
    for y in y0..y1 {
        for x in x0..x1 {
            let lap = 4.0 * l(x, y) - l(x - 1, y) - l(x + 1, y) - l(x, y - 1) - l(x, y + 1);
            acc += (lap * lap) as f64;
            n += 1;
        }
    }
    ((acc / n.max(1) as f64).max(1e-12)).log10() as f32
}

impl FaceEngine {
    pub fn load(deterministic: bool) -> Result<FaceEngine, String> {
        let need = |id: &str| installed_path(id).ok_or_else(|| format!("the {id} model isn't installed"));
        Ok(FaceEngine { det: session(&need("yunet-2023mar")?, deterministic)?, rec: session(&need("sface-2021dec")?, deterministic)? })
    }

    /// Detects faces in an RGB8 frame (any size) and embeds each.
    pub fn detect(&mut self, rgb: &[u8], w: usize, h: usize) -> Result<Vec<Face>, String> {
        // Fit into 640×640 (top-left, keeping the aspect), BGR 0–255.
        let s = DET as f32 / w.max(h) as f32;
        let (sw, sh) = (((w as f32 * s) as usize).min(DET), ((h as f32 * s) as usize).min(DET));
        let mut input = vec![0f32; 3 * DET * DET];
        for y in 0..sh {
            let sy = ((y as f32 + 0.5) / s) as usize;
            for x in 0..sw {
                let sx = ((x as f32 + 0.5) / s) as usize;
                let i = (sy.min(h - 1) * w + sx.min(w - 1)) * 3;
                for c in 0..3 {
                    input[c * DET * DET + y * DET + x] = rgb[i + 2 - c] as f32;
                }
            }
        }
        let t = Tensor::from_array(Array4::from_shape_vec((1, 3, DET, DET), input).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let out = self.det.run(ort::inputs!["input" => t]).map_err(|e| e.to_string())?;
        let mut cands: Vec<([f32; 4], f32, [[f32; 2]; 5])> = Vec::new();
        for stride in [8usize, 16, 32] {
            let get = |name: &str| out[format!("{name}_{stride}").as_str()].try_extract_tensor::<f32>().map(|(_, d)| d.to_vec()).map_err(|e| e.to_string());
            let (cls, obj, bbox, kps) = (get("cls")?, get("obj")?, get("bbox")?, get("kps")?);
            let cols = DET / stride;
            for i in 0..cls.len() {
                let score = (cls[i].clamp(0.0, 1.0) * obj[i].clamp(0.0, 1.0)).sqrt();
                if score < MIN_SCORE {
                    continue;
                }
                let (r, c) = ((i / cols) as f32, (i % cols) as f32);
                let st = stride as f32;
                let (cx, cy) = ((c + bbox[4 * i]) * st, (r + bbox[4 * i + 1]) * st);
                let (bw, bh) = (bbox[4 * i + 2].exp() * st, bbox[4 * i + 3].exp() * st);
                let b = [(cx - bw / 2.0) / s, (cy - bh / 2.0) / s, bw / s, bh / s];
                let mut lm = [[0f32; 2]; 5];
                for k in 0..5 {
                    lm[k] = [((kps[10 * i + 2 * k] + c) * st) / s, ((kps[10 * i + 2 * k + 1] + r) * st) / s];
                }
                cands.push((b, score, lm));
            }
        }
        drop(out);
        // NMS.
        cands.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut keep: Vec<([f32; 4], f32, [[f32; 2]; 5])> = Vec::new();
        for c in cands {
            if keep.iter().all(|k| iou(&k.0, &c.0) < NMS_IOU) {
                keep.push(c);
            }
        }
        let mut faces = Vec::new();
        for (b, score, lm) in keep {
            let aligned = align(rgb, w, h, &lm);
            let t = Tensor::from_array(Array4::from_shape_vec((1, 3, 112, 112), aligned).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            let o = self.rec.run(ort::inputs!["data" => t]).map_err(|e| e.to_string())?;
            let (_, emb) = o["fc1"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
            let mut e = emb.to_vec();
            let n = e.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
            e.iter_mut().for_each(|x| *x /= n);
            faces.push(Face { bbox: b, score, landmarks: lm, embedding: e, sharpness: box_sharpness(rgb, w, h, &b), size: (b[2] * b[3]) / (w * h) as f32 });
        }
        Ok(faces)
    }
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// A face followed across samples.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceTrack {
    /// (sample index, face index within that sample).
    pub faces: Vec<(usize, usize)>,
    /// Unit-length mean embedding.
    pub embedding: Vec<f32>,
}

/// Links faces sample to sample by IoU ≥ 0.3 and embedding cosine ≥ 0.5 (greedy, best first).
pub fn tracks(samples: &[Vec<Face>]) -> Vec<FaceTrack> {
    let mut open: Vec<(usize, FaceTrack)> = Vec::new(); // (last sample, track)
    let mut done = Vec::new();
    for (k, faces) in samples.iter().enumerate() {
        let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
        for (ti, (last, t)) in open.iter().enumerate() {
            if *last + 1 != k {
                continue;
            }
            let (ls, lf) = *t.faces.last().unwrap_or(&(0, 0));
            let prev = &samples[ls][lf];
            for (fi, f) in faces.iter().enumerate() {
                let (o, c) = (iou(&prev.bbox, &f.bbox), cosine(&prev.embedding, &f.embedding));
                if o >= 0.3 && c >= 0.5 {
                    pairs.push((o + c, ti, fi));
                }
            }
        }
        pairs.sort_by(|a, b| b.0.total_cmp(&a.0));
        let (mut used_t, mut used_f) = (vec![false; open.len()], vec![false; faces.len()]);
        for (_, ti, fi) in pairs {
            if used_t[ti] || used_f[fi] {
                continue;
            }
            used_t[ti] = true;
            used_f[fi] = true;
            open[ti].0 = k;
            open[ti].1.faces.push((k, fi));
        }
        for fi in (0..faces.len()).filter(|f| !used_f[*f]) {
            open.push((k, FaceTrack { faces: vec![(k, fi)], embedding: Vec::new() }));
        }
        // Tracks not continued here are closed.
        let (keep, closed): (Vec<_>, Vec<_>) = open.into_iter().partition(|(last, _)| *last == k);
        done.extend(closed.into_iter().map(|x| x.1));
        open = keep;
    }
    done.extend(open.into_iter().map(|x| x.1));
    for t in &mut done {
        let d = samples[t.faces[0].0][t.faces[0].1].embedding.len();
        let mut m = vec![0f32; d];
        for &(s, f) in &t.faces {
            for (a, b) in m.iter_mut().zip(&samples[s][f].embedding) {
                *a += b;
            }
        }
        let n = m.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
        m.iter_mut().for_each(|x| *x /= n);
        t.embedding = m;
    }
    done
}

/// Average-linkage agglomerative clustering by cosine distance; returns a label per item.
pub fn cluster(embs: &[Vec<f32>], max_distance: f32) -> Vec<usize> {
    let mut groups: Vec<Vec<usize>> = (0..embs.len()).map(|i| vec![i]).collect();
    loop {
        let mut best: Option<(f32, usize, usize)> = None;
        for i in 0..groups.len() {
            for j in i + 1..groups.len() {
                let mut sum = 0.0;
                for &a in &groups[i] {
                    for &b in &groups[j] {
                        sum += 1.0 - cosine(&embs[a], &embs[b]);
                    }
                }
                let d = sum / (groups[i].len() * groups[j].len()) as f32;
                if d <= max_distance && best.is_none_or(|b| d < b.0) {
                    best = Some((d, i, j));
                }
            }
        }
        let Some((_, i, j)) = best else { break };
        let g = groups.remove(j);
        groups[i].extend(g);
    }
    let mut labels = vec![0; embs.len()];
    for (l, g) in groups.iter().enumerate() {
        for &i in g {
            labels[i] = l;
        }
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    fn face(x: f32, e: [f32; 2]) -> Face {
        let n = (e[0] * e[0] + e[1] * e[1]).sqrt();
        Face { bbox: [x, 10.0, 50.0, 50.0], score: 0.9, landmarks: [[0.0; 2]; 5], embedding: vec![e[0] / n, e[1] / n], sharpness: -2.0, size: 0.05 }
    }

    #[test]
    fn alignment_tracks_and_clusters() {
        // The template maps onto itself.
        let t = similarity(&TEMPLATE, &TEMPLATE);
        assert!((t[0] - 1.0).abs() < 1e-4 && t[1].abs() < 1e-4 && t[2].abs() < 1e-3);
        // Two people side by side for three samples, the second one leaving.
        let s = vec![vec![face(0.0, [1.0, 0.0]), face(200.0, [0.0, 1.0])], vec![face(4.0, [1.0, 0.05]), face(203.0, [0.05, 1.0])], vec![face(8.0, [1.0, 0.1])]];
        let tr = tracks(&s);
        assert_eq!(tr.len(), 2);
        assert_eq!(tr.iter().map(|t| t.faces.len()).max(), Some(3));
        let labels = cluster(&[vec![1.0, 0.0], vec![0.99, 0.1], vec![0.0, 1.0]], 0.3);
        assert_eq!(labels[0], labels[1]);
        assert_ne!(labels[0], labels[2]);
    }

    fn load(name: &str) -> Option<(Vec<u8>, usize, usize)> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/faces").join(name);
        let o = Command::new("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-i"]).arg(&p).args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"]).output().ok()?;
        let probe = Command::new("ffmpeg").args(["-hide_banner", "-i"]).arg(&p).output().ok()?;
        let text = String::from_utf8_lossy(&probe.stderr).to_string();
        let dims = text.split(|c: char| !c.is_ascii_digit() && c != 'x').find(|t| t.contains('x') && t.split('x').all(|n| n.len() >= 2 && n.parse::<usize>().is_ok()))?;
        let (w, h) = dims.split_once('x')?;
        let (w, h) = (w.parse().ok()?, h.parse().ok()?);
        (o.stdout.len() == w * h * 3).then_some((o.stdout, w, h))
    }

    /// With the models installed: one face per portrait; the same face matches itself mirrored
    /// far better than the other person.
    #[test]
    fn real_faces() {
        let Ok(mut eng) = FaceEngine::load(true) else {
            eprintln!("face models not installed: skipping");
            return;
        };
        let (Some(a), Some(b)) = (load("einstein.jpg"), load("lincoln.jpg")) else {
            eprintln!("ffmpeg or fixtures missing: skipping");
            return;
        };
        let fa = eng.detect(&a.0, a.1, a.2).unwrap();
        let fb = eng.detect(&b.0, b.1, b.2).unwrap();
        assert_eq!((fa.len(), fb.len()), (1, 1), "{fa:?} {fb:?}");
        assert!(fa[0].size > 0.05 && fa[0].embedding.len() == 128);
        // Mirror image of the first portrait.
        let (w, h) = (a.1, a.2);
        let mut m = vec![0u8; a.0.len()];
        for y in 0..h {
            for x in 0..w {
                let (s, d) = ((y * w + x) * 3, (y * w + (w - 1 - x)) * 3);
                m[d..d + 3].copy_from_slice(&a.0[s..s + 3]);
            }
        }
        let fm = eng.detect(&m, w, h).unwrap();
        let same = cosine(&fa[0].embedding, &fm[0].embedding);
        let other = cosine(&fa[0].embedding, &fb[0].embedding);
        eprintln!("same person {same:.3}, different {other:.3}");
        assert!(same > 0.6 && other < 0.363, "same {same} other {other}");
        let labels = cluster(&[fa[0].embedding.clone(), fm[0].embedding.clone(), fb[0].embedding.clone()], CLUSTER_DISTANCE);
        assert_eq!(labels[0], labels[1]);
        assert_ne!(labels[0], labels[2]);
    }
}
