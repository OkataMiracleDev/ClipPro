//! Binary encoding of measurements (SPEC §12.3): columnar, u16-quantised, zstd-compressed.
//! Column keys are stored so newer readers can skip unknown columns and older data simply
//! lacks new ones (they decode as NaN).

use crate::calibration::Calibration;
use crate::columns::{dequantise, quantise, Col, MeasurementSet, NCOLS};
use crate::image::Rect;
use crate::sample::SampleRaw;
use clippro_types::GroupMask;

const MAGIC: &[u8; 4] = b"CPMS";
const VERSION: u16 = 1;
const TILE_RANGE: [f32; 2] = [-7.0, 0.0];

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("not a ClipPro measurement blob")]
    Magic,
    #[error("unsupported measurement blob version {0}")]
    Version(u16),
    #[error("truncated measurement blob")]
    Truncated,
    #[error("compression: {0}")]
    Zstd(String),
    #[error("sample chunk: {0}")]
    Chunk(String),
}

struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.0.extend_from_slice(b);
    }
}

struct R<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], CodecError> {
        if self.p + n > self.b.len() {
            return Err(CodecError::Truncated);
        }
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, CodecError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, CodecError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().map_err(|_| CodecError::Truncated)?))
    }
    fn u32(&mut self) -> Result<u32, CodecError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().map_err(|_| CodecError::Truncated)?))
    }
    fn f32(&mut self) -> Result<f32, CodecError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().map_err(|_| CodecError::Truncated)?))
    }
    fn f64(&mut self) -> Result<f64, CodecError> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().map_err(|_| CodecError::Truncated)?))
    }
}

/// Encodes a (quantised) measurement set.
pub fn encode(ms: &MeasurementSet, cal: &Calibration) -> Result<Vec<u8>, CodecError> {
    let n = ms.len();
    let mut w = W(Vec::with_capacity(n * (NCOLS * 2 + 100) + 256));
    w.bytes(MAGIC);
    w.u16(VERSION);
    w.u32(n as u32);
    w.f64(ms.rate_hz);
    w.f64(ms.fps);
    w.u8(ms.pair_sampling as u8);
    for v in [ms.active.x, ms.active.y, ms.active.w, ms.active.h] {
        w.u32(v as u32);
    }
    w.u32(ms.working.0);
    w.u32(ms.working.1);
    w.u8(ms.groups.0);
    // Times (delta-coded) and flags.
    let mut prev = 0i64;
    for t in &ms.time_us {
        w.i64(t - prev);
        prev = *t;
    }
    for f in &ms.flags {
        w.u32(*f);
    }
    let mut bits = vec![0u8; n.div_ceil(8)];
    for (k, c) in ms.cut_before.iter().enumerate() {
        if *c {
            bits[k / 8] |= 1 << (k % 8);
        }
    }
    w.bytes(&bits);
    // Columns.
    w.u16(NCOLS as u16);
    for &c in Col::ALL {
        let key = c.key().as_bytes();
        w.u8(key.len() as u8);
        w.bytes(key);
        let r = cal.range(c);
        w.f32(r[0]);
        w.f32(r[1]);
        for v in ms.col(c) {
            w.u16(quantise(*v, r));
        }
    }
    // Tiles, motion vectors, pHash, histograms.
    w.u8(ms.tile_dims.0);
    w.u8(ms.tile_dims.1);
    w.u32(ms.tiles.len() as u32);
    for v in &ms.tiles {
        w.u16(quantise(*v, TILE_RANGE));
    }
    w.u8(ms.mv_dims.0);
    w.u8(ms.mv_dims.1);
    w.u32(ms.mvs.len() as u32);
    for v in &ms.mvs {
        w.u8(v[0] as u8);
        w.u8(v[1] as u8);
    }
    for h in &ms.phash {
        w.u64(*h);
    }
    w.u32(ms.hist64.len() as u32);
    w.bytes(&ms.hist64);
    zstd::bulk::compress(&w.0, 3).map_err(|e| CodecError::Zstd(e.to_string()))
}

/// Decodes a measurement set. Unknown columns are skipped; missing ones decode as NaN.
pub fn decode(blob: &[u8]) -> Result<MeasurementSet, CodecError> {
    let raw = zstd::stream::decode_all(blob).map_err(|e| CodecError::Zstd(e.to_string()))?;
    let mut r = R { b: &raw, p: 0 };
    if r.take(4)? != MAGIC {
        return Err(CodecError::Magic);
    }
    let ver = r.u16()?;
    if ver != VERSION {
        return Err(CodecError::Version(ver));
    }
    let n = r.u32()? as usize;
    let mut ms = MeasurementSet::new(n);
    ms.cols = vec![vec![f32::NAN; n]; NCOLS];
    ms.rate_hz = r.f64()?;
    ms.fps = r.f64()?;
    ms.pair_sampling = r.u8()? != 0;
    let (x, y, aw, ah) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?);
    ms.active = Rect { x: x as usize, y: y as usize, w: aw as usize, h: ah as usize };
    ms.working = (r.u32()?, r.u32()?);
    ms.groups = GroupMask(r.u8()?);
    let mut t = 0i64;
    for c in r.take(8 * n)?.chunks_exact(8) {
        t += i64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]);
        ms.time_us.push(t);
    }
    ms.flags.extend(r.take(4 * n)?.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])));
    let bits = r.take(n.div_ceil(8))?;
    ms.cut_before = (0..n).map(|k| bits[k / 8] & (1 << (k % 8)) != 0).collect();
    let ncols = r.u16()? as usize;
    for _ in 0..ncols {
        let klen = r.u8()? as usize;
        let key = String::from_utf8_lossy(r.take(klen)?).into_owned();
        let range = [r.f32()?, r.f32()?];
        let bytes = r.take(2 * n)?;
        if let Some(c) = Col::from_key(&key) {
            ms.cols[c as usize] = bytes.chunks_exact(2).map(|b| dequantise(u16::from_le_bytes([b[0], b[1]]), range)).collect();
        }
    }
    ms.tile_dims = (r.u8()?, r.u8()?);
    let nt = r.u32()? as usize;
    ms.tiles = r.take(2 * nt)?.chunks_exact(2).map(|b| dequantise(u16::from_le_bytes([b[0], b[1]]), TILE_RANGE)).collect();
    ms.mv_dims = (r.u8()?, r.u8()?);
    let nm = r.u32()? as usize;
    ms.mvs = r.take(2 * nm)?.chunks_exact(2).map(|b| [b[0] as i8, b[1] as i8]).collect();
    ms.phash = r
        .take(8 * n)?
        .chunks_exact(8)
        .map(|c| u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
        .collect();
    let nh = r.u32()? as usize;
    ms.hist64 = r.take(nh)?.to_vec();
    Ok(ms)
}

/// Encodes a chunk of raw samples (checkpoints) as MessagePack + zstd.
pub fn encode_samples(samples: &[SampleRaw]) -> Result<Vec<u8>, CodecError> {
    let bytes = rmp_serde::to_vec(samples).map_err(|e| CodecError::Chunk(e.to_string()))?;
    zstd::bulk::compress(&bytes, 3).map_err(|e| CodecError::Zstd(e.to_string()))
}

pub fn decode_samples(blob: &[u8]) -> Result<Vec<SampleRaw>, CodecError> {
    let raw = zstd::stream::decode_all(blob).map_err(|e| CodecError::Zstd(e.to_string()))?;
    rmp_serde::from_slice(&raw).map_err(|e| CodecError::Chunk(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clippro_types::FlagKind;

    fn sample_set(n: usize) -> MeasurementSet {
        let cal = Calibration::embedded();
        let mut ms = MeasurementSet::new(n);
        ms.time_us = (0..n as i64).map(|k| k * 250_000 + 17).collect();
        ms.cols = (0..NCOLS)
            .map(|c| {
                let r = cal.range(Col::ALL[c]);
                (0..n).map(|k| if k % 13 == 5 { f32::NAN } else { r[0] + (r[1] - r[0]) * ((k * 7 + c) % 100) as f32 / 100.0 }).collect()
            })
            .collect();
        ms.flags = (0..n).map(|k| if k % 3 == 0 { FlagKind::Shaky.bit() } else { 0 }).collect();
        ms.cut_before = (0..n).map(|k| k % 50 == 49).collect();
        ms.tile_dims = (8, 5);
        ms.tiles = (0..n * 40).map(|i| -7.0 + (i % 70) as f32 / 10.0).collect();
        ms.mv_dims = (16, 9);
        ms.mvs = (0..n * 144).map(|i| [(i % 200) as i8, i8::MIN]).collect();
        ms.phash = (0..n as u64).map(|k| k * 0x9E37_79B9).collect();
        ms.hist64 = (0..n * 64).map(|i| (i % 256) as u8).collect();
        ms.rate_hz = 4.0;
        ms.fps = 25.0;
        ms.pair_sampling = true;
        ms.active = Rect { x: 0, y: 70, w: 960, h: 400 };
        ms.working = (960, 540);
        ms.groups = GroupMask(0b1011);
        ms.quantise_in_place(cal);
        ms.tiles.iter_mut().for_each(|v| *v = dequantise(quantise(*v, TILE_RANGE), TILE_RANGE));
        ms
    }

    fn same(a: &MeasurementSet, b: &MeasurementSet) {
        assert_eq!(a.time_us, b.time_us);
        assert_eq!(a.flags, b.flags);
        assert_eq!(a.cut_before, b.cut_before);
        for c in 0..NCOLS {
            for (x, y) in a.cols[c].iter().zip(&b.cols[c]) {
                assert!(x.to_bits() == y.to_bits() || (x.is_nan() && y.is_nan()), "col {c}: {x} vs {y}");
            }
        }
        assert_eq!(a.tiles, b.tiles);
        assert_eq!(a.mvs, b.mvs);
        assert_eq!(a.phash, b.phash);
        assert_eq!(a.hist64, b.hist64);
        assert_eq!((a.rate_hz, a.fps, a.pair_sampling, a.active, a.working, a.groups), (b.rate_hz, b.fps, b.pair_sampling, b.active, b.working, b.groups));
    }

    #[test]
    fn roundtrip_is_exact() {
        let ms = sample_set(1000);
        let blob = encode(&ms, Calibration::embedded()).unwrap();
        same(&ms, &decode(&blob).unwrap());
    }

    #[test]
    fn one_hour_fits_budget() {
        // ADR-001: 1 h at 4 Hz (with tiles, motion vectors and histograms) encodes to ≤ 4 MB.
        let ms = sample_set(14_400);
        let blob = encode(&ms, Calibration::embedded()).unwrap();
        assert!(blob.len() <= 4_000_000, "{} bytes", blob.len());
        eprintln!("1 h synthetic: {} KB", blob.len() / 1024);
    }

    #[test]
    fn decode_100h_columns_fast() {
        // P3.2: decode 1.44 M samples (100 h at 4 Hz) into SoA columns in ≤ 300 ms (release).
        let cal = Calibration::embedded();
        let n = 1_440_000;
        let mut ms = MeasurementSet::new(n);
        ms.time_us = (0..n as i64).map(|k| k * 250_000).collect();
        ms.cols = (0..NCOLS).map(|c| (0..n).map(|k| ((k * 31 + c * 7) % 1000) as f32 / 1000.0).collect()).collect();
        ms.flags = vec![0; n];
        ms.cut_before = vec![false; n];
        ms.phash = vec![0; n];
        ms.quantise_in_place(cal);
        let blob = encode(&ms, cal).unwrap();
        let t = std::time::Instant::now();
        let back = decode(&blob).unwrap();
        let el = t.elapsed();
        assert_eq!(back.len(), n);
        eprintln!("decode 1.44 M samples: {el:?}");
        // Wall-clock budgets are asserted in performance runs (CLIPPRO_PERF=1, PLAN P8.3/P14.6),
        // on an idle machine at full clock; ordinary test runs only report the time.
        if std::env::var("CLIPPRO_PERF").is_ok() && !cfg!(debug_assertions) {
            assert!(el.as_millis() <= 300, "{el:?}");
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode(b"not zstd").is_err());
        let bad = zstd::bulk::compress(b"XXXX\x01\x00", 3).unwrap();
        assert!(matches!(decode(&bad), Err(CodecError::Magic)));
    }
}
