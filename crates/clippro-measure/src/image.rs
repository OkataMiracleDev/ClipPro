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

/// Flattened box-filter weights: output `i` reads source indices `start[i]..start[i]+len[i]`
/// with weights `w[off[i]..off[i]+len[i]]`.
struct FlatWeights {
    start: Vec<usize>,
    len: Vec<usize>,
    off: Vec<usize>,
    w: Vec<f32>,
}

fn flat_weights(start: f64, len: f64, out: usize, limit: usize) -> FlatWeights {
    let nested = area_weights(start, len, out);
    let mut fw = FlatWeights {
        start: Vec::with_capacity(out),
        len: Vec::with_capacity(out),
        off: Vec::with_capacity(out),
        w: Vec::new(),
    };
    for ws in nested {
        let first = ws.first().map(|x| x.0).unwrap_or(0).min(limit.saturating_sub(1));
        fw.start.push(first);
        fw.off.push(fw.w.len());
        let mut n = 0;
        for (i, wt) in ws {
            if i < limit {
                fw.w.push(wt);
                n += 1;
            } else if n > 0 {
                // Past the edge: fold into the last valid sample (edge clamp).
                if let Some(last) = fw.w.last_mut() {
                    *last += wt;
                }
            }
        }
        if n == 0 {
            fw.w.push(1.0);
            n = 1;
        }
        fw.len.push(n);
    }
    fw
}

/// Raw code-value range that maps into [0,1]. Clamping codes is equivalent to clamping
/// normalised values because the mapping is monotonic.
fn code_range(p: &PlaneRef) -> (f32, f32, u32, u32) {
    let (s, o) = p.norm();
    let lo = ((-o / s) - 1e-3).ceil().max(0.0);
    let hi = (((1.0 - o) / s) + 1e-3).floor().min(65535.0);
    (s, o, lo as u32, hi as u32)
}

/// Area-filter resize of a decoded plane straight to f32. `src` is a rectangle in source pixel
/// coordinates (may be fractional for chroma mapping). Works on raw code values and normalises
/// once per output pixel; integer downscale factors take a pure integer box-sum path.
pub fn resize_plane(p: &PlaneRef, src: (f64, f64, f64, f64), ow: usize, oh: usize) -> Image {
    let (sx, sy, sw, sh) = src;
    let (s, o, lo, hi) = code_range(p);
    let step = p.step.max(1);
    let mut out = Image::new(ow, oh);
    if ow == 0 || oh == 0 || p.width == 0 || p.height == 0 {
        return out;
    }
    let integral = sx.fract() == 0.0 && sy.fract() == 0.0 && sw.fract() == 0.0 && sh.fract() == 0.0;
    let (x0, y0, iw, ih) = (sx as usize, sy as usize, sw as usize, sh as usize);
    if integral && iw % ow == 0 && ih % oh == 0 && x0 + iw <= p.width && y0 + ih <= p.height {
        let (kx, ky) = (iw / ow, ih / oh);
        let scale = s / (kx * ky) as f32;
        let mut sums = vec![0u32; ow];
        for oy in 0..oh {
            sums.iter_mut().for_each(|v| *v = 0);
            for r in 0..ky {
                let base = (y0 + oy * ky + r) * p.stride + x0 * step;
                match &p.data {
                    PlaneData::U8(d) => {
                        for (ox, acc) in sums.iter_mut().enumerate() {
                            let b = base + ox * kx * step;
                            let mut t = 0u32;
                            for k in 0..kx {
                                t += (d[b + k * step] as u32).clamp(lo, hi);
                            }
                            *acc += t;
                        }
                    }
                    PlaneData::U16(d) => {
                        for (ox, acc) in sums.iter_mut().enumerate() {
                            let b = base + ox * kx * step;
                            let mut t = 0u32;
                            for k in 0..kx {
                                t += (d[b + k * step] as u32).clamp(lo, hi);
                            }
                            *acc += t;
                        }
                    }
                }
            }
            let row = &mut out.data[oy * ow..(oy + 1) * ow];
            for (dst, sum) in row.iter_mut().zip(&sums) {
                *dst = (*sum as f32 * scale + o).clamp(0.0, 1.0);
            }
        }
        return out;
    }
    // General separable path with flattened weights on raw codes.
    // Each source row's horizontal pass is computed once and scattered to the output rows that
    // use it (non-integer factors share rows between neighbouring outputs).
    let wx = flat_weights(sx, sw, ow, p.width);
    let wy = flat_weights(sy, sh, oh, p.height);
    let mut users: Vec<Vec<(usize, f32)>> = vec![Vec::new(); p.height];
    for oy in 0..oh {
        for j in 0..wy.len[oy] {
            users[wy.start[oy] + j].push((oy, wy.w[wy.off[oy] + j]));
        }
    }
    let mut tmp = vec![0f32; ow];
    for (y, us) in users.iter().enumerate() {
        if us.is_empty() {
            continue;
        }
        let base = y * p.stride;
        match &p.data {
            PlaneData::U8(d) => horizontal(d, base, step, &wx, lo, hi, &mut tmp),
            PlaneData::U16(d) => horizontal(d, base, step, &wx, lo, hi, &mut tmp),
        }
        for &(oy, wyv) in us {
            let dst = &mut out.data[oy * ow..(oy + 1) * ow];
            for (d, t) in dst.iter_mut().zip(&tmp) {
                *d += t * wyv;
            }
        }
    }
    for d in out.data.iter_mut() {
        *d = (*d * s + o).clamp(0.0, 1.0);
    }
    out
}

#[inline]
fn horizontal<T: Copy + Into<u32>>(d: &[T], base: usize, step: usize, wx: &FlatWeights, lo: u32, hi: u32, out: &mut [f32]) {
    for (ox, t) in out.iter_mut().enumerate() {
        let (st, off, n) = (wx.start[ox], wx.off[ox], wx.len[ox]);
        let mut acc = 0f32;
        for i in 0..n {
            let v: u32 = d[base + (st + i) * step].into();
            acc += v.clamp(lo, hi) as f32 * wx.w[off + i];
        }
        *t = acc;
    }
}

/// Reference implementation (normalise every source pixel, then filter); used by tests.
pub fn resize_plane_reference(p: &PlaneRef, src: (f64, f64, f64, f64), ow: usize, oh: usize) -> Image {
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
    fn fast_resize_matches_reference() {
        let (w, h) = (96usize, 54usize);
        let d8: Vec<u8> = (0..w * h).map(|i| ((i * 37 + i / w * 11) % 256) as u8).collect();
        let d16: Vec<u16> = (0..w * h).map(|i| (((i * 97) % 1024) << 6) as u16).collect();
        let mk = |data, width, step, bit_depth, msb, full, chroma| PlaneRef {
            data,
            width,
            height: h,
            stride: w,
            step,
            bit_depth,
            msb_aligned: msb,
            full_range: full,
            chroma,
        };
        let planes = [
            mk(PlaneData::U8(&d8), w, 1, 8, false, false, false),
            mk(PlaneData::U8(&d8), w, 1, 8, false, true, false),
            mk(PlaneData::U16(&d16), w, 1, 10, true, false, false),
            mk(PlaneData::U8(&d8), w / 2, 2, 8, false, false, true),
        ];
        for p in &planes {
            let full = (0.0, 0.0, p.width as f64, p.height as f64);
            for (ow, oh) in [(p.width / 2, p.height / 2), (p.width / 3, p.height / 3), (17, 11), (p.width, p.height)] {
                let a = resize_plane(p, full, ow, oh);
                let b = resize_plane_reference(p, full, ow, oh);
                for (x, y) in a.data.iter().zip(&b.data) {
                    assert!((x - y).abs() < 2e-5, "{ow}x{oh}: {x} vs {y}");
                }
            }
            let sub = (3.5, 2.25, p.width as f64 / 2.0, p.height as f64 / 2.0);
            let a = resize_plane(p, sub, 9, 7);
            let b = resize_plane_reference(p, sub, 9, 7);
            for (x, y) in a.data.iter().zip(&b.data) {
                assert!((x - y).abs() < 2e-5, "sub: {x} vs {y}");
            }
        }
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
