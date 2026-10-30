//! Per-clip analysis: decode along the sampling plan, measure each sample, stitch sections,
//! run the temporal pass (SPEC §10, ALGORITHMS §0–§2).

use clippro_measure::calibration::Calibration;
use clippro_measure::columns::{Col, MeasurementSet};
use clippro_measure::image::{resize_plane, working_dims, working_image, Image, Rect};
use clippro_measure::sample::{detect_active_area, ClipContext, SampleInput, SampleMeasurer, SampleRaw};
use clippro_measure::temporal::temporal_pass;
use clippro_media::video::FrameView;
use clippro_media::{sample_plan, DecodeOptions, MediaInfo, VideoDecoder};
use clippro_types::{GroupMask, MetricGroup, SampleRate};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

pub const WORKING_LONG_EDGE: usize = 960;
const ACTIVE_SAMPLES: usize = 16;

#[derive(Clone, Debug)]
pub struct AnalyzeOptions {
    pub rate: SampleRate,
    pub pair_sampling: bool,
    pub groups: GroupMask,
    pub decode: DecodeOptions,
    /// Audio stream to analyse (container index); `None` = the best one.
    pub audio_stream: Option<usize>,
}

impl Default for AnalyzeOptions {
    fn default() -> Self {
        AnalyzeOptions { rate: SampleRate::Standard, pair_sampling: true, groups: GroupMask::ALL_COMPUTE, decode: DecodeOptions::default(), audio_stream: None }
    }
}

/// Audio measurements for a clip's first audio stream (`None` if it can't be decoded).
pub fn analyze_audio(path: &std::path::Path, info: &clippro_media::MediaInfo, stream: Option<usize>) -> Option<clippro_audio::AudioSet> {
    let mut a: Option<clippro_audio::AudioAnalyzer> = None;
    let st = clippro_media::audio::decode_audio(path, stream, info.start_us, |block| {
        a.get_or_insert_with(|| clippro_audio::AudioAnalyzer::new(block.len(), 0)).push(block);
        true
    })
    .ok()?;
    let mut set = a?.finish();
    set.start_us = st.start_us;
    Some(set)
}

#[derive(Debug, thiserror::Error)]
pub enum AnalyzeError {
    #[error(transparent)]
    Media(#[from] clippro_media::MediaError),
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, AnalyzeError>;

/// Effective sample rate (Hz) for a plan.
pub fn effective_rate(rate: SampleRate, info: &MediaInfo) -> f64 {
    rate.hz().unwrap_or_else(|| info.fps().fps())
}

/// RANSAC seed base from a hex fingerprint.
pub fn seed_from_fingerprint(fp: &str) -> u64 {
    u64::from_str_radix(&fp.chars().take(16).collect::<String>(), 16).unwrap_or(0x5EED)
}

/// Cb/Cr thumbnails (32×18) of the active area, straight from the decoder planes.
fn chroma_thumbs(f: &FrameView, active: Rect, working: (usize, usize)) -> Option<(Image, Image)> {
    let (cb, cr) = f.chroma.as_ref()?;
    let sx = cb.width as f64 / working.0 as f64;
    let sy = cb.height as f64 / working.1 as f64;
    let rect = (active.x as f64 * sx, active.y as f64 * sy, active.w as f64 * sx, active.h as f64 * sy);
    Some((resize_plane(cb, rect, 32, 18), resize_plane(cr, rect, 32, 18)))
}

/// Detects the active area from the clip's first samples (ALGORITHMS §1.0 step 3).
pub fn detect_active(path: &Path, info: &MediaInfo, plan: &[i64], opts: &AnalyzeOptions) -> Result<Rect> {
    let mut dec = VideoDecoder::open(path, info, &opts.decode)?;
    let mut frames: Vec<Image> = Vec::new();
    let n = plan.len().min(ACTIVE_SAMPLES);
    dec.decode_samples(&plan[..n], 0, false, |s| {
        frames.push(working_image(&s.frame.luma, WORKING_LONG_EDGE));
        true
    })?;
    let refs: Vec<&Image> = frames.iter().collect();
    Ok(if refs.is_empty() { Rect::default() } else { detect_active_area(&refs) })
}

/// Measures plan samples `[start, end)` (a section). The section starts one sample early
/// when `start > 0` so the first real sample has a previous sample (ALGORITHMS §0.3).
pub fn measure_section(
    path: &Path,
    info: &MediaInfo,
    plan: &[i64],
    start: usize,
    end: usize,
    ctx: &ClipContext,
    opts: &AnalyzeOptions,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize),
) -> Result<Vec<SampleRaw>> {
    let mut out: Vec<SampleRaw> = Vec::with_capacity(end.saturating_sub(start));
    measure_section_each(path, info, plan, start, end, ctx, opts, cancel, |raw| {
        progress(raw.index);
        out.push(raw);
    })?;
    Ok(out)
}

/// As `measure_section`, delivering each measured sample (in order) to `on_sample` instead of
/// collecting them. Returns `Cancelled` if `cancel` was set; samples already delivered stand.
pub fn measure_section_each(
    path: &Path,
    info: &MediaInfo,
    plan: &[i64],
    start: usize,
    end: usize,
    ctx: &ClipContext,
    opts: &AnalyzeOptions,
    cancel: &AtomicBool,
    mut on_sample: impl FnMut(SampleRaw),
) -> Result<()> {
    if start >= end {
        return Ok(());
    }
    let mut dec = VideoDecoder::open(path, info, &opts.decode)?;
    let mut measurer = SampleMeasurer::new(ctx.clone());
    let from = start.saturating_sub(1);
    let mut cancelled = false;
    let working = ctx.working;
    dec.decode_samples(&plan[from..end], from, opts.pair_sampling, |s| {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            return false;
        }
        let frame = working_image(&s.frame.luma, WORKING_LONG_EDGE);
        let pair = s.pair.as_ref().map(|p| working_image(&p.luma, WORKING_LONG_EDGE));
        let chroma = chroma_thumbs(&s.frame, ctx.active, working);
        let raw = measurer.measure(SampleInput {
            index: s.index,
            time_us: plan[s.index],
            frame: &frame,
            pair: pair.as_ref(),
            chroma,
        });
        if s.index >= start {
            on_sample(raw);
        }
        true
    })?;
    if cancelled {
        return Err(AnalyzeError::Cancelled);
    }
    Ok(())
}

/// Clip-level context for a file.
pub fn clip_context(info: &MediaInfo, active: Rect, seed_base: u64, opts: &AnalyzeOptions) -> ClipContext {
    let v = info.video.as_ref();
    let (w, h) = v.map(|v| (v.width as usize, v.height as usize)).unwrap_or((2, 2));
    let working = working_dims(w, h, WORKING_LONG_EDGE);
    let active = if active.w == 0 || active.h == 0 { Rect::full(working.0, working.1) } else { active };
    ClipContext {
        fps: info.fps().fps(),
        active,
        working,
        seed_base,
        pair_sampling: opts.pair_sampling,
        motion: opts.groups.contains(MetricGroup::Motion),
    }
}

/// Stitches sections (already in order), runs the temporal pass and masks disabled groups.
pub fn finish(samples: Vec<SampleRaw>, ctx: &ClipContext, rate_hz: f64, opts: &AnalyzeOptions, cal: &Calibration) -> MeasurementSet {
    let mut ms = temporal_pass(&samples, ctx, rate_hz, cal);
    ms.groups = opts.groups;
    if !opts.groups.contains(MetricGroup::Focus) {
        for c in [Col::SharpFull, Col::SharpCentre, Col::DetailRaw, Col::FocusStabRaw, Col::Tenengrad] {
            ms.col_mut(c).iter_mut().for_each(|v| *v = f32::NAN);
        }
    }
    ms
}

/// Analyses a whole clip in the calling thread (sections run sequentially). The scheduler in
/// `jobs.rs` runs sections in parallel instead.
pub fn analyze_clip(
    path: &Path,
    info: &MediaInfo,
    opts: &AnalyzeOptions,
    seed_base: u64,
    cal: &Calibration,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<MeasurementSet> {
    let plan = sample_plan(info.duration_us, info.fps(), opts.rate);
    let active = detect_active(path, info, &plan, opts)?;
    let ctx = clip_context(info, active, seed_base, opts);
    let total = plan.len();
    let samples = measure_section(path, info, &plan, 0, total, &ctx, opts, cancel, |i| progress(i + 1, total))?;
    Ok(finish(samples, &ctx, effective_rate(opts.rate, info), opts, cal))
}
