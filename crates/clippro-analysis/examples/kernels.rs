//! Per-kernel timing on real decoded frames (dev diagnostic): `--example kernels -- FILE`
use clippro_measure::image::{working_image, Rect};
use clippro_measure::motion::{estimate, Pyramid};
use clippro_measure::sample::horizon_tilt;
use clippro_measure::spatial::*;
use clippro_media::{probe, DecodeOptions, VideoDecoder};
use std::time::Instant;

fn main() {
    let p = std::env::args().nth(1).unwrap();
    let path = std::path::Path::new(&p);
    let info = probe(path).unwrap();
    let mut dec = VideoDecoder::open(path, &info, &DecodeOptions::default()).unwrap();
    let mut frames = Vec::new();
    let mut lumas = Vec::new();
    dec.decode_samples(&[1_000_000, 1_040_000], 0, false, |s| {
        let t = Instant::now();
        frames.push(working_image(&s.frame.luma, 960));
        lumas.push(t.elapsed());
        true
    })
    .unwrap();
    println!("working_image: {:?}", lumas);
    let (a, b) = (&frames[0], &frames[1]);
    let n = 20;
    let time = |name: &str, f: &mut dyn FnMut()| {
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        println!("{name:<22} {:>8.3} ms", t.elapsed().as_secs_f64() * 1e3 / n as f64);
    };
    time("spatial (all)", &mut || {
        std::hint::black_box(spatial(a));
    });
    time("exposure", &mut || {
        std::hint::black_box(exposure(a));
    });
    time("row_band", &mut || {
        std::hint::black_box(row_band(a));
    });
    time("row_band_temporal", &mut || {
        std::hint::black_box(row_band_temporal(a, b, 0.0));
    });
    time("thumb 64x36", &mut || {
        std::hint::black_box(a.resize_area(Rect::full(a.w, a.h), 64, 36));
    });
    let pa = Pyramid::build(a);
    let pb = Pyramid::build(b);
    time("pyramid", &mut || {
        std::hint::black_box(Pyramid::build(a));
    });
    time("motion estimate", &mut || {
        std::hint::black_box(estimate(&pa, &pb, 0.003, 1));
    });
    time("horizon_tilt", &mut || {
        std::hint::black_box(horizon_tilt(pa.l0()));
    });
    let th = a.resize_area(Rect::full(a.w, a.h), 64, 36);
    time("phash", &mut || {
        std::hint::black_box(phash(&th));
    });
    time("mad full", &mut || {
        std::hint::black_box(mad(a, b));
    });
    time("crop", &mut || {
        std::hint::black_box(a.crop(Rect::full(a.w, a.h)));
    });
}
