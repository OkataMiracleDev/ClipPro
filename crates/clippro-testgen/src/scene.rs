//! Procedural scene texture and a virtual camera renderer.

use rayon::prelude::*;

/// Deterministic hash → [0,1).
fn hash(x: i64, y: i64, seed: u64) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ seed.wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Tileable value noise with period `p` lattice cells.
fn value_noise(x: f32, y: f32, p: i64, seed: u64) -> f32 {
    let (xi, yi) = (x.floor() as i64, y.floor() as i64);
    let (fx, fy) = (smooth(x - xi as f32), smooth(y - yi as f32));
    let g = |a: i64, b: i64| hash(a.rem_euclid(p), b.rem_euclid(p), seed);
    let a = g(xi, yi) + (g(xi + 1, yi) - g(xi, yi)) * fx;
    let b = g(xi, yi + 1) + (g(xi + 1, yi + 1) - g(xi, yi + 1)) * fx;
    a + (b - a) * fy
}

/// A tileable scene: luma plus low-frequency chroma, all in [0,1].
pub struct Texture {
    pub size: usize,
    pub y: Vec<f32>,
    pub cb: Vec<f32>,
    pub cr: Vec<f32>,
    /// Box-filtered mip levels 1.. (level k is size >> k), used to avoid aliasing.
    pub mips: Vec<Texture>,
}

impl Texture {
    /// Fractal noise with sharp-edged shapes so focus and motion are measurable.
    pub fn new(size: usize, seed: u64) -> Texture {
        let s = size as f32;
        let rows: Vec<(Vec<f32>, Vec<f32>, Vec<f32>)> = (0..size)
            .into_par_iter()
            .map(|y| {
                let mut ly = Vec::with_capacity(size);
                let mut lb = Vec::with_capacity(size);
                let mut lr = Vec::with_capacity(size);
                for x in 0..size {
                    let (u, v) = (x as f32, y as f32);
                    let mut n = 0.0;
                    let mut amp = 0.5;
                    let mut cells = 8i64;
                    for o in 0..6 {
                        n += amp * value_noise(u / s * cells as f32, v / s * cells as f32, cells, seed + o);
                        amp *= 0.55;
                        cells *= 2;
                    }
                    // Hard-edged blobs (step of a mid-frequency noise) and fine stripes.
                    let blob = value_noise(u / s * 24.0, v / s * 24.0, 24, seed + 99);
                    let edge = if blob > 0.55 { 0.18 } else { -0.05 };
                    let stripes = 0.06 * ((u / s * 512.0 * std::f32::consts::TAU).sin().signum())
                        * (value_noise(u / s * 16.0, v / s * 16.0, 16, seed + 7) > 0.6) as i32 as f32;
                    ly.push((0.12 + 0.75 * n + edge + stripes).clamp(0.02, 0.97));
                    lb.push(0.5 + 0.12 * (value_noise(u / s * 4.0, v / s * 4.0, 4, seed + 31) - 0.5));
                    lr.push(0.5 + 0.12 * (value_noise(u / s * 4.0, v / s * 4.0, 4, seed + 57) - 0.5));
                }
                (ly, lb, lr)
            })
            .collect();
        let mut t = Texture { size, y: Vec::with_capacity(size * size), cb: Vec::new(), cr: Vec::new(), mips: Vec::new() };
        t.cb.reserve(size * size);
        t.cr.reserve(size * size);
        for (a, b, c) in rows {
            t.y.extend(a);
            t.cb.extend(b);
            t.cr.extend(c);
        }
        let mut cur = t.half();
        while cur.size >= 64 {
            let next = cur.half();
            t.mips.push(cur);
            cur = next;
        }
        t
    }

    fn half(&self) -> Texture {
        let n = self.size / 2;
        let down = |p: &[f32]| -> Vec<f32> {
            let mut o = Vec::with_capacity(n * n);
            for y in 0..n {
                for x in 0..n {
                    let i = 2 * y * self.size + 2 * x;
                    o.push(0.25 * (p[i] + p[i + 1] + p[i + self.size] + p[i + self.size + 1]));
                }
            }
            o
        };
        Texture { size: n, y: down(&self.y), cb: down(&self.cb), cr: down(&self.cr), mips: Vec::new() }
    }

    fn mip(&self, k: usize) -> &Texture {
        if k == 0 {
            self
        } else {
            &self.mips[(k - 1).min(self.mips.len() - 1)]
        }
    }

    /// Trilinear sample of one plane (0 = Y, 1 = Cb, 2 = Cr) at texel coords for `lod`.
    #[inline]
    fn sample_lod(&self, plane: usize, x: f32, y: f32, lod: f32) -> f32 {
        let lod = lod.clamp(0.0, self.mips.len() as f32);
        let k = lod.floor() as usize;
        let f = lod - k as f32;
        let pick = |t: &Texture, d: f32| {
            let p = match plane {
                0 => &t.y,
                1 => &t.cb,
                _ => &t.cr,
            };
            t.sample(p, x / d, y / d)
        };
        let a = pick(self.mip(k), (1u32 << k) as f32);
        if f < 1e-3 || k >= self.mips.len() {
            return a;
        }
        let b = pick(self.mip(k + 1), (1u32 << (k + 1)) as f32);
        a + (b - a) * f
    }

    #[inline]
    fn sample(&self, plane: &[f32], x: f32, y: f32) -> f32 {
        let n = self.size as i64;
        let (xi, yi) = (x.floor() as i64, y.floor() as i64);
        let (fx, fy) = (x - xi as f32, y - yi as f32);
        let at = |a: i64, b: i64| plane[(b.rem_euclid(n) * n + a.rem_euclid(n)) as usize];
        let a = at(xi, yi) + (at(xi + 1, yi) - at(xi, yi)) * fx;
        let b = at(xi, yi + 1) + (at(xi + 1, yi + 1) - at(xi, yi + 1)) * fx;
        a + (b - a) * fy
    }
}

/// Camera pose in texture coordinates.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pose {
    pub cx: f32,
    pub cy: f32,
    /// > 1 = zoomed in (content enlarged).
    pub zoom: f32,
    /// Content rotation in radians, y-down (positive = clockwise on screen).
    pub rot: f32,
}

/// A planar YUV 4:4:4 float frame.
#[derive(Clone)]
pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub y: Vec<f32>,
    pub cb: Vec<f32>,
    pub cr: Vec<f32>,
}

impl Frame {
    pub fn solid(w: usize, h: usize, y: f32, cb: f32, cr: f32) -> Frame {
        Frame { w, h, y: vec![y; w * h], cb: vec![cb; w * h], cr: vec![cr; w * h] }
    }

    /// Renders the texture through a camera with the given pose. `scale` is texture px per
    /// frame px at zoom 1.
    pub fn render(tex: &Texture, w: usize, h: usize, pose: Pose, scale: f32) -> Frame {
        let lod = (scale / pose.zoom).max(1.0).log2();
        let (c, s) = (pose.rot.cos(), pose.rot.sin());
        let rows: Vec<(Vec<f32>, Vec<f32>, Vec<f32>)> = (0..h)
            .into_par_iter()
            .map(|y| {
                let mut ly = Vec::with_capacity(w);
                let mut lb = Vec::with_capacity(w);
                let mut lr = Vec::with_capacity(w);
                for x in 0..w {
                    let (px, py) = (x as f32 - w as f32 / 2.0, y as f32 - h as f32 / 2.0);
                    // Inverse rotate then scale: content appears rotated by +rot.
                    let (rx, ry) = (c * px + s * py, -s * px + c * py);
                    let (tx, ty) = (pose.cx + rx * scale / pose.zoom, pose.cy + ry * scale / pose.zoom);
                    ly.push(tex.sample_lod(0, tx, ty, lod));
                    lb.push(tex.sample_lod(1, tx, ty, lod));
                    lr.push(tex.sample_lod(2, tx, ty, lod));
                }
                (ly, lb, lr)
            })
            .collect();
        let mut f = Frame { w, h, y: Vec::with_capacity(w * h), cb: Vec::with_capacity(w * h), cr: Vec::with_capacity(w * h) };
        for (a, b, cc) in rows {
            f.y.extend(a);
            f.cb.extend(b);
            f.cr.extend(cc);
        }
        f
    }

    /// Averages several frames (motion blur from sub-frame samples).
    pub fn average(frames: &[Frame]) -> Frame {
        let mut out = frames[0].clone();
        let n = frames.len() as f32;
        for f in &frames[1..] {
            for i in 0..out.y.len() {
                out.y[i] += f.y[i];
                out.cb[i] += f.cb[i];
                out.cr[i] += f.cr[i];
            }
        }
        for i in 0..out.y.len() {
            out.y[i] /= n;
            out.cb[i] /= n;
            out.cr[i] /= n;
        }
        out
    }

    /// Separable Gaussian blur of all planes.
    pub fn blur(&self, sigma: f32) -> Frame {
        if sigma <= 0.05 {
            return self.clone();
        }
        let r = (3.0 * sigma).ceil() as i64;
        let k: Vec<f32> = (-r..=r).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
        let ks: f32 = k.iter().sum();
        let (w, h) = (self.w, self.h);
        let pass = |src: &[f32], horiz: bool| -> Vec<f32> {
            (0..h)
                .into_par_iter()
                .flat_map_iter(|y| {
                    let k = &k;
                    (0..w).map(move |x| {
                        let mut acc = 0.0;
                        for (j, kv) in k.iter().enumerate() {
                            let o = j as i64 - r;
                            let (sx, sy) = if horiz {
                                ((x as i64 + o).clamp(0, w as i64 - 1) as usize, y)
                            } else {
                                (x, (y as i64 + o).clamp(0, h as i64 - 1) as usize)
                            };
                            acc += src[sy * w + sx] * kv;
                        }
                        acc / ks
                    })
                })
                .collect()
        };
        let bl = |p: &[f32]| pass(&pass(p, true), false);
        Frame { w, h, y: bl(&self.y), cb: bl(&self.cb), cr: bl(&self.cr) }
    }

    pub fn map_luma(&mut self, f: impl Fn(usize, usize, f32) -> f32 + Sync) {
        let w = self.w;
        self.y.par_iter_mut().enumerate().for_each(|(i, v)| *v = f(i % w, i / w, *v));
    }

    /// Adds Gaussian noise (deterministic per frame seed).
    pub fn add_noise(&mut self, sigma: f32, seed: u64) {
        if sigma <= 0.0 {
            return;
        }
        let w = self.w;
        self.y.par_iter_mut().enumerate().for_each(|(i, v)| {
            let u1 = hash((i % w) as i64, (i / w) as i64, seed).max(1e-7);
            let u2 = hash((i % w) as i64 + 7, (i / w) as i64 + 3, seed ^ 0x5555);
            let g = (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos();
            *v += sigma * g;
        });
    }

    /// 8-bit full-swing planes converted to limited-range YUV 4:4:4 bytes (Y, Cb, Cr).
    pub fn to_yuv444p(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.y.len() * 3);
        out.extend(self.y.iter().map(|v| (16.0 + 219.0 * v.clamp(0.0, 1.0)).round() as u8));
        out.extend(self.cb.iter().map(|v| (16.0 + 224.0 * v.clamp(0.0, 1.0)).round() as u8));
        out.extend(self.cr.iter().map(|v| (16.0 + 224.0 * v.clamp(0.0, 1.0)).round() as u8));
        out
    }

    /// 16-bit little-endian limited-range YUV 4:4:4 (for 10-bit outputs).
    pub fn to_yuv444p16(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.y.len() * 6);
        let mut push = |v: f32, lo: f32, span: f32| {
            let q = ((lo + span * v.clamp(0.0, 1.0)) * 256.0).round() as u16;
            out.extend_from_slice(&q.to_le_bytes());
        };
        for v in &self.y {
            push(*v, 16.0, 219.0);
        }
        for v in &self.cb {
            push(*v, 16.0, 224.0);
        }
        for v in &self.cr {
            push(*v, 16.0, 224.0);
        }
        out
    }
}

/// Rec.709 RGB [0,1] → (Y, Cb, Cr) in [0,1] (Cb/Cr neutral at 0.5).
pub fn rgb_to_ycbcr(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let cb = (b - y) / 1.8556 + 0.5;
    let cr = (r - y) / 1.5748 + 0.5;
    (y, cb, cr)
}
