//! Stage timing breakdown for one clip (dev diagnostic):
//! `cargo run --release --example profile -- FILE [cpu]`
use clippro_measure::image::working_image;
use clippro_measure::sample::{ClipContext, SampleInput, SampleMeasurer};
use clippro_media::{probe, sample_plan, DecodeOptions, HwMode, VideoDecoder};
use clippro_types::SampleRate;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = std::path::Path::new(&args[1]);
    let hw = if args.get(2).map(|s| s == "cpu").unwrap_or(false) { HwMode::Off } else { HwMode::Auto };
    let info = probe(path).unwrap();
    let plan = sample_plan(info.duration_us, info.fps(), SampleRate::Standard);
    let opts = DecodeOptions { threads: 4, hw };
    let dur = info.duration_us as f64 / 1e6;
    let v = info.video.as_ref().unwrap();
    println!("{} {}x{} {} {:.1}s, {} samples, hw={:?}", path.display(), v.width, v.height, v.codec, dur, plan.len(), hw);

    let t = Instant::now();
    let mut dec = VideoDecoder::open(path, &info, &opts).unwrap();
    let open = t.elapsed();
    let t = Instant::now();
    let st = dec.decode_samples(&plan, 0, true, |_| true).unwrap();
    let decode = t.elapsed();
    println!("open {:>7.1} ms | decode+pairs {:>7.1} ms ({} frames, hw used {})", ms(open), ms(decode), st.frames_decoded, st.hw);

    let mut dec = VideoDecoder::open(path, &info, &opts).unwrap();
    let mut wi = Duration::ZERO;
    let mut meas = Duration::ZERO;
    let (w, h) = clippro_measure::image::working_dims(v.width as usize, v.height as usize, 960);
    let ctx = ClipContext {
        fps: info.fps().fps(),
        active: clippro_measure::image::Rect::full(w, h),
        working: (w, h),
        seed_base: 1,
        pair_sampling: true,
        motion: true,
    };
    let mut m = SampleMeasurer::new(ctx);
    let t0 = Instant::now();
    dec.decode_samples(&plan, 0, true, |s| {
        let t = Instant::now();
        let f = working_image(&s.frame.luma, 960);
        let p = s.pair.as_ref().map(|p| working_image(&p.luma, 960));
        wi += t.elapsed();
        let t = Instant::now();
        m.measure(SampleInput { index: s.index, time_us: plan[s.index], frame: &f, pair: p.as_ref(), chroma: None });
        meas += t.elapsed();
        true
    })
    .unwrap();
    let total = t0.elapsed();
    let n = plan.len() as f64;
    println!(
        "full pass {:>7.1} ms = decode {:>7.1} + working image {:>7.1} + measure {:>7.1} | per sample: measure {:.2} ms | {:.1}x realtime",
        ms(total),
        ms(total - wi - meas),
        ms(wi),
        ms(meas),
        ms(meas) / n,
        dur / total.as_secs_f64()
    );
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}
