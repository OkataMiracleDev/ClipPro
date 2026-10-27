//! Frame inspector (SPEC VIE-2, VIE-4, VIE-5): everything known about one sample, and why it
//! is or isn't in a select.

use crate::session::{Result, Session, SessionError};
use crate::store::lock;
use clippro_measure::columns::Col;
use clippro_score::frame::SampleBreakdown;
use clippro_score::{Segment, Settings};
use clippro_types::{ClipId, FlagKind, Tier};
use std::sync::atomic::AtomicBool;

#[derive(Clone, Debug)]
pub struct Inspection {
    pub sample: usize,
    pub time_us: i64,
    pub breakdown: SampleBreakdown,
    /// Raw measurement columns for the sample (key, value; NaN = not measured).
    pub raw: Vec<(&'static str, f32)>,
    /// Index of the select containing the sample, within this clip's segments.
    pub select: Option<usize>,
    pub segment: Option<Segment>,
    /// The sample is in a select.
    pub pass: bool,
    /// Plain-language reasons, most important first ("Force-excluded by Shaky").
    pub reasons: Vec<String>,
    /// Tile sharpness grid (cols, rows) and log10 values, row-major.
    pub tile_dims: (u8, u8),
    pub tiles: Vec<f32>,
    /// Motion vector grid (cols, rows) and (dx, dy) per cell in working-image pixels.
    pub mv_dims: (u8, u8),
    pub mvs: Vec<[i8; 2]>,
    /// 64-bin luma histogram (sum ≈ 255).
    pub hist: Vec<u8>,
    /// Working image size the grids refer to.
    pub working: (u32, u32),
}

fn clock(us: i64) -> String {
    let s = us.max(0) as f64 / 1e6;
    format!("{}:{:05.2}", (s / 60.0).floor() as i64, s % 60.0)
}

fn names(flags: &[FlagKind]) -> String {
    flags.iter().map(|f| f.label()).collect::<Vec<_>>().join(", ")
}

impl Session {
    /// Inspector data for `sample` of `clip`, scored with `settings`.
    pub fn inspect(&self, clip: ClipId, settings: &Settings, sample: usize) -> Result<Inspection> {
        let clips = self.clip_inputs()?;
        let input = clips.iter().find(|c| c.id == clip).ok_or_else(|| SessionError::Other("This clip has no finished analysis.".into()))?;
        let ms = &input.ms;
        if sample >= ms.len() {
            return Err(SessionError::Other(format!("Sample {sample} is out of range (clip has {}).", ms.len())));
        }
        let overrides = self.overrides()?;
        let mut engine = lock(&self.engine);
        let sel = engine
            .score(&clips, settings, &overrides, &AtomicBool::new(false))
            .ok_or_else(|| SessionError::Other("scoring was cancelled".into()))?;
        let breakdown = engine.explain(input, settings, sample);
        drop(engine);
        let cs = sel.clips.iter().find(|c| c.clip == clip);
        let select = cs.and_then(|c| c.segments.iter().position(|s| (s.first_sample..=s.last_sample).contains(&sample)));
        let segment = select.and_then(|i| cs.map(|c| c.segments[i].clone()));

        let tier = |f: FlagKind| settings.tiers.get(f as usize).copied().unwrap_or_default();
        let excluded: Vec<FlagKind> = breakdown.flags.iter().copied().filter(|f| tier(*f) == Tier::ForceExclude).collect();
        let included: Vec<FlagKind> = breakdown.flags.iter().copied().filter(|f| tier(*f) == Tier::ForceInclude).collect();
        let pct = |x: f32| (x * 100.0).round() as i32;
        let candidate = (breakdown.score >= settings.score_lo || breakdown.force_in) && !breakdown.force_out;
        let mut reasons = Vec::new();
        match (&segment, select) {
            (Some(seg), Some(i)) => {
                reasons.push(format!("In select {} ({}–{})", i + 1, clock(seg.start_us), clock(seg.end_us)));
                if !candidate {
                    reasons.push(format!("Below the threshold ({} < {}) but bridged by the gap rule", pct(breakdown.score), pct(settings.score_lo)));
                }
                if breakdown.force_in && !included.is_empty() {
                    reasons.push(format!("Force-included by {}", names(&included)));
                }
            }
            _ if breakdown.force_out => {
                reasons.push(if excluded.is_empty() { "Force-excluded".to_string() } else { format!("Force-excluded by {}", names(&excluded)) });
            }
            _ if breakdown.score.is_nan() => reasons.push("No measurements for this sample".into()),
            _ if !candidate => reasons.push(format!("Score {} is below the threshold ({})", pct(breakdown.score), pct(settings.score_lo))),
            _ => reasons.push("Passes the score, but its run was dropped by the length, score-range or per-clip rules".into()),
        }
        if !breakdown.flags.is_empty() && segment.is_some() && excluded.is_empty() {
            reasons.push(format!("Flags: {}", names(&breakdown.flags)));
        }

        let raw = Col::ALL.iter().map(|&c| (c.key(), ms.cols[c as usize].get(sample).copied().unwrap_or(f32::NAN))).collect();
        let grid = |dims: (u8, u8)| dims.0 as usize * dims.1 as usize;
        let slice = |n: usize| sample * n..(sample + 1) * n;
        let tiles = ms.tiles.get(slice(grid(ms.tile_dims))).map(<[f32]>::to_vec).unwrap_or_default();
        let mvs = ms.mvs.get(slice(grid(ms.mv_dims))).map(<[[i8; 2]]>::to_vec).unwrap_or_default();
        let hist = ms.hist64.get(slice(64)).map(<[u8]>::to_vec).unwrap_or_default();
        Ok(Inspection {
            sample,
            time_us: ms.time_us[sample],
            pass: segment.is_some(),
            breakdown,
            raw,
            select,
            segment,
            reasons,
            tile_dims: ms.tile_dims,
            tiles,
            mv_dims: ms.mv_dims,
            mvs,
            hist,
            working: ms.working,
        })
    }
}

impl Session {
    /// The clips that best match the camera-movement filter (SPEC TUN-6 "live best 5 matches"):
    /// mean camera factor per clip, scoring with metrics and flags switched off so the filter
    /// alone decides. Best first.
    pub fn camera_matches(&self, settings: &Settings, n: usize) -> Result<Vec<(ClipId, f32)>> {
        let mut s = settings.clone();
        s.switches.metrics = false;
        s.switches.flags = false;
        s.switches.camera_filter = true;
        s.camera.enabled = true;
        let clips = self.clip_inputs()?;
        let overrides = self.overrides()?;
        let sel = lock(&self.engine)
            .score(&clips, &s, &overrides, &AtomicBool::new(false))
            .ok_or_else(|| SessionError::Other("scoring was cancelled".into()))?;
        let mut out: Vec<(ClipId, f32)> = sel
            .clips
            .iter()
            .filter_map(|c| {
                let v: Vec<f32> = c.scores.score.iter().copied().filter(|x| x.is_finite()).collect();
                (!v.is_empty()).then(|| (c.clip, v.iter().sum::<f32>() / v.len() as f32))
            })
            .collect();
        out.sort_by(|a, b| b.1.total_cmp(&a.1));
        out.truncate(n);
        Ok(out)
    }
}

