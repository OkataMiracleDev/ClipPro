//! Image planes, area-filter resampling and the working image (ALGORITHMS §1.0).

/// Raw samples of a decoded plane.
#[derive(Clone, Copy, Debug)]
pub enum PlaneData<'a> {
    U8(&'a [u8]),
    U16(&'a [u16]),
}

/// A borrowed decoded plane (luma or chroma) straight from the decoder.
#[derive(Clone, Copy, Debug)]
pub struct PlaneRef<'a> {
    pub data: PlaneData<'a>,
    pub width: usize,
    pub height: usize,
    /// Row stride in elements (not bytes).
    pub stride: usize,
    /// Elements between consecutive samples in a row (1 for planar, 2 for interleaved NV12 chroma).
    pub step: usize,
    /// Significant bits (8, 10, 12, 16).
    pub bit_depth: u8,
    /// Value stored in the most significant bits of each u16 (P010/P016 layout).
    pub msb_aligned: bool,
    /// Full-range (PC) rather than limited-range (TV) coding.
    pub full_range: bool,
    /// True for chroma planes (limited range is 16..240 instead of 16..235, neutral at 0.5).
    pub chroma: bool,
}

impl<'a> PlaneRef<'a> {
    /// Returns the (scale, offset) mapping a raw code value to [0,1].
    fn norm(&self) -> (f32, f32) {
        let b = self.bit_depth.clamp(8, 16) as i32;
        let shift = if self.msb_aligned { 16 - b } else { 0 };
        let unit = 2f32.powi(b - 8);
        let pre = 1.0 / 2f32.powi(shift);
        if self.full_range {
            let max = 2f32.powi(b) - 1.0;
            (pre / max, 0.0)
        } else if self.chroma {
            // 16..240 → 0..1
            (pre / (224.0 * unit), -16.0 * unit / (224.0 * unit))
        } else {
            (pre / (219.0 * unit), -16.0 * unit / (219.0 * unit))
        }
    }

    /// Reads row `y` as normalised f32 into `out` (len = width).
    pub fn row_f32(&self, y: usize, out: &mut [f32]) {
        let (s, o) = self.norm();
        let start = y * self.stride;
        let step = self.step.max(1);
        let end = start + (self.width - 1) * step + 1;
        match self.data {
            PlaneData::U8(d) => {
                for (dst, &v) in out.iter_mut().zip(d[start..end].iter().step_by(step)) {
                    *dst = (v as f32 * s + o).clamp(0.0, 1.0);
                }
            }
            PlaneData::U16(d) => {
                for (dst, &v) in out.iter_mut().zip(d[start..end].iter().step_by(step)) {
                    *dst = (v as f32 * s + o).clamp(0.0, 1.0);
                }
            }
        }
    }
}

/// An owned single-channel f32 image.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub data: Vec<f32>,
}

impl Image {
    pub fn new(w: usize, h: usize) -> Self {
        Image { w, h, data: vec![0.0; w * h] }
    }

    pub fn from_fn(w: usize, h: usize, mut f: impl FnMut(usize, usize) -> f32) -> Self {
        let mut data = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                data.push(f(x, y));
            }
        }
        Image { w, h, data }
    }

    #[inline]
    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.data[y * self.w + x]
    }

    pub fn row(&self, y: usize) -> &[f32] {
        &self.data[y * self.w..(y + 1) * self.w]
    }

    /// Copies a sub-rectangle.
    pub fn crop(&self, r: Rect) -> Image {
        let mut out = Image::new(r.w, r.h);
        for y in 0..r.h {
            out.data[y * r.w..(y + 1) * r.w]
                .copy_from_slice(&self.data[(r.y + y) * self.w + r.x..(r.y + y) * self.w + r.x + r.w]);
        }
        out
    }

    /// Area-filter resize of a sub-rectangle.
    pub fn resize_area(&self, r: Rect, ow: usize, oh: usize) -> Image {
        let wx = area_weights(r.x as f64, r.w as f64, ow);
        let wy = area_weights(r.y as f64, r.h as f64, oh);
        let mut tmp = vec![0f32; ow * r.h];
        for y in 0..r.h {
            let row = self.row(r.y + y);
            let dst = &mut tmp[y * ow..(y + 1) * ow];
            apply_weights(row, &wx, dst);
        }
        vertical(&tmp, ow, r.h, &wy, r.y)
    }

    /// 2×2 box downscale (floor of odd sizes).
    pub fn half(&self) -> Image {
        let (w, h) = ((self.w / 2).max(1), (self.h / 2).max(1));
        let mut out = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let x0 = (2 * x).min(self.w - 1);
                let x1 = (2 * x + 1).min(self.w - 1);
                let y0 = (2 * y).min(self.h - 1);
                let y1 = (2 * y + 1).min(self.h - 1);
                out.data[y * w + x] =
                    0.25 * (self.at(x0, y0) + self.at(x1, y0) + self.at(x0, y1) + self.at(x1, y1));
            }
        }
        out
    }

    pub fn mean(&self) -> f64 {
        if self.data.is_empty() {
            return 0.0;
        }
        pairwise_sum(&self.data) / self.data.len() as f64
    }
}

/// Integer rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl Rect {
    pub fn full(w: usize, h: usize) -> Rect {
        Rect { x: 0, y: 0, w, h }
    }
    pub fn area(&self) -> usize {
        self.w * self.h
    }
}

/// Deterministic pairwise summation in f64.
pub fn pairwise_sum(v: &[f32]) -> f64 {
    if v.len() <= 256 {
        let mut s = 0f64;
        for &x in v {
            s += x as f64;
        }
        return s;
    }
    let m = v.len() / 2;
    pairwise_sum(&v[..m]) + pairwise_sum(&v[m..])
}

/// Contribution weights of source samples for each output sample of a box filter.
/// Source span is `[start, start+len)`; returns per output: list of (src index, weight).
fn area_weights(start: f64, len: f64, out: usize) -> Vec<Vec<(usize, f32)>> {
    let scale = len / out as f64;
    (0..out)
        .map(|o| {
            let a = start + o as f64 * scale;
            let b = a + scale;
            let mut ws = Vec::new();
            let mut i = a.floor() as usize;
            while (i as f64) < b - 1e-12 {
                let lo = a.max(i as f64);
                let hi = b.min(i as f64 + 1.0);
                if hi > lo {
                    ws.push((i, ((hi - lo) / scale) as f32));
                }
                i += 1;
            }
            ws
        })
        .collect()
}

#[inline]
fn apply_weights(row: &[f32], w: &[Vec<(usize, f32)>], dst: &mut [f32]) {
    for (d, ws) in dst.iter_mut().zip(w) {
        let mut s = 0f32;
        for &(i, wt) in ws {
            s += row[i.min(row.len() - 1)] * wt;
        }
        *d = s;
    }
}

fn vertical(tmp: &[f32], ow: usize, rows: usize, wy: &[Vec<(usize, f32)>], y0: usize) -> Image {
    let oh = wy.len();
    let mut out = Image::new(ow, oh);
    for (oy, ws) in wy.iter().enumerate() {
        let dst = &mut out.data[oy * ow..(oy + 1) * ow];
        for &(sy, wt) in ws {
            let ry = (sy - y0).min(rows - 1);
            let src = &tmp[ry * ow..(ry + 1) * ow];
            for (d, &s) in dst.iter_mut().zip(src) {
                *d += s * wt;
            }
        }
    }
    out
}

/// Working-image dimensions: long edge ≤ `max_long`, never upscaled, even, ≥ 2.
pub fn working_dims(w: usize, h: usize, max_long: usize) -> (usize, usize) {
    let long = w.max(h).max(1);
    let scale = (max_long as f64 / long as f64).min(1.0);
    let even = |v: f64| (((v / 2.0).round() as usize) * 2).max(2);
    (even(w as f64 * scale), even(h as f64 * scale))
}

/// Area-filter resize of a decoded plane straight to f32 (one pass over source rows).
/// `src` is a rectangle in source pixel coordinates (may be fractional for chroma mapping).
pub fn resize_plane(p: &PlaneRef, src: (f64, f64, f64, f64), ow: usize, oh: usize) -> Image {
    let (sx, sy, sw, sh) = src;
    let wx = area_weights(sx, sw, ow);
    let wy = area_weights(sy, sh, oh);
    let y_first = sy.floor() as usize;
    let y_last = ((sy + sh).ceil() as usize).min(p.height);
    let rows = y_last.saturating_sub(y_first).max(1);
    let mut tmp = vec![0f32; ow * rows];
    let mut row = vec![0f32; p.width];
    for r in 0..rows {
        let y = (y_first + r).min(p.height - 1);
        p.row_f32(y, &mut row);
        apply_weights(&row, &wx, &mut tmp[r * ow..(r + 1) * ow]);
    }
    vertical(&tmp, ow, rows, &wy, y_first)
}

/// Builds the working image (≤ 960 px long edge) from a luma plane.
pub fn working_image(luma: &PlaneRef, max_long: usize) -> Image {
    let (w, h) = working_dims(luma.width, luma.height, max_long);
    resize_plane(luma, (0.0, 0.0, luma.width as f64, luma.height as f64), w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dims() {
        assert_eq!(working_dims(3840, 2160, 960), (960, 540));
        assert_eq!(working_dims(1920, 1080, 960), (960, 540));
        assert_eq!(working_dims(640, 360, 960), (640, 360));
        assert_eq!(working_dims(1080, 1920, 960), (540, 960));
        assert_eq!(working_dims(4096, 1716, 960), (960, 402));
        let (w, h) = working_dims(1001, 333, 960);
        assert!(w % 2 == 0 && h % 2 == 0 && w <= 960);
    }

    #[test]
    fn area_filter_checkerboard_is_mean() {
        let src = Image::from_fn(64, 64, |x, y| if (x + y) % 2 == 0 { 1.0 } else { 0.0 });
        let out = src.resize_area(Rect::full(64, 64), 16, 16);
        for v in &out.data {
            assert!((v - 0.5).abs() < 1e-5, "{v}");
        }
        // Non-integer ratio preserves the mean.
        let out = src.resize_area(Rect::full(64, 64), 10, 7);
        assert!((out.mean() - 0.5).abs() < 0.02);
    }

    #[test]
    fn plane_normalisation() {
        let data: Vec<u8> = vec![16, 235, 0, 255];
        let p = PlaneRef {
            data: PlaneData::U8(&data),
            width: 4,
            height: 1,
            stride: 4,
            step: 1,
            bit_depth: 8,
            msb_aligned: false,
            full_range: false,
            chroma: false,
        };
        let mut row = vec![0f32; 4];
        p.row_f32(0, &mut row);
        for (got, want) in row.iter().zip([0.0, 1.0, 0.0, 1.0]) {
            assert!((got - want).abs() < 1e-6, "{row:?}");
        }
        // P010: 10-bit 940 (white) in the MSBs.
        let d16: Vec<u16> = vec![64 << 6, 940 << 6];
        let p = PlaneRef {
            data: PlaneData::U16(&d16),
            width: 2,
            height: 1,
            stride: 2,
            step: 1,
            bit_depth: 10,
            msb_aligned: true,
            full_range: false,
            chroma: false,
        };
        let mut row = vec![0f32; 2];
        p.row_f32(0, &mut row);
        assert!(row[0].abs() < 1e-6 && (row[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn interleaved_chroma_step() {
        // NV12 UV row: U0 V0 U1 V1 …
        let data: Vec<u8> = vec![10, 200, 20, 210, 30, 220, 40, 230];
        let mk = |off: usize| PlaneRef {
            data: PlaneData::U8(&data[off..]),
            width: 4,
            height: 1,
            stride: 8,
            step: 2,
            bit_depth: 8,
            msb_aligned: false,
            full_range: true,
            chroma: true,
        };
        let mut u = vec![0f32; 4];
        let mut v = vec![0f32; 4];
        mk(0).row_f32(0, &mut u);
        mk(1).row_f32(0, &mut v);
        assert!((u[3] * 255.0 - 40.0).abs() < 1e-3 && (v[0] * 255.0 - 200.0).abs() < 1e-3);
    }

    #[test]
    fn resize_plane_matches_image_resize() {
        let data: Vec<u8> = (0..(40 * 30)).map(|i| (i * 7 % 256) as u8).collect();
        let p = PlaneRef {
            data: PlaneData::U8(&data),
            width: 40,
            height: 30,
            stride: 40,
            step: 1,
            bit_depth: 8,
            msb_aligned: false,
            full_range: true,
            chroma: false,
        };
        let img = resize_plane(&p, (0.0, 0.0, 40.0, 30.0), 40, 30);
        let small = resize_plane(&p, (0.0, 0.0, 40.0, 30.0), 13, 9);
        let small2 = img.resize_area(Rect::full(40, 30), 13, 9);
        for (a, b) in small.data.iter().zip(&small2.data) {
            assert!((a - b).abs() < 1e-5);
        }
    }
}
