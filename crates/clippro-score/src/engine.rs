//! The scoring engine: staged, cached, parallel and cancellable (SPEC §11).

use crate::frame::{explain_sample, frame_scores, FrameScores, SampleBreakdown};
use crate::normalize::{normalise, ClipInput, NormCols};
use crate::segments::{candidate_runs, finalise, segments_from_runs, Override, Segment, Timing};
use crate::settings::{KeyHasher, Settings};
use clippro_measure::calibration::Calibration;
use clippro_types::{ClipId, FlagKind};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// Per-stage execution counters (for tests and diagnostics).
#[derive(Default, Debug)]
pub struct StageCounters {
    pub normalise: AtomicUsize,
    pub frame: AtomicUsize,
    pub runs: AtomicUsize,
    pub segments: AtomicUsize,
    pub finalise: AtomicUsize,
}

impl StageCounters {
    pub fn snapshot(&self) -> [usize; 5] {
        [
            self.normalise.load(Ordering::Relaxed),
            self.frame.load(Ordering::Relaxed),
            self.runs.load(Ordering::Relaxed),
            self.segments.load(Ordering::Relaxed),
            self.finalise.load(Ordering::Relaxed),
        ]
    }
}

#[derive(Clone, Default)]
struct ClipCache {
    key_norm: u64,
    norm: Option<Arc<NormCols>>,
    key_frame: u64,
    frame: Option<Arc<FrameScores>>,
    key_runs: u64,
    runs: Option<Arc<Vec<(usize, usize)>>>,
    key_segs: u64,
    segs: Option<Arc<Vec<Segment>>>,
    key_final: u64,
    fin: Option<Arc<Vec<Segment>>>,
}

/// Selects for one clip.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClipSelects {
    pub clip: ClipId,
    pub segments: Vec<Segment>,
    /// Final per-sample scores F (shared, for score bars).
    #[serde(skip)]
    pub scores: Arc<FrameScores>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Summary {
    pub segment_count: usize,
    pub selects_us: i64,
    pub footage_us: i64,
    pub percent: f32,
    /// Runs of consecutive flagged samples across the project, per flag.
    pub flag_runs: Vec<(FlagKind, usize)>,
}

#[derive(Clone, Debug)]
pub struct Selects {
    pub clips: Vec<ClipSelects>,
    pub summary: Summary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    #[default]
    Timeline,
    ScoreDesc,
    ScoreAsc,
    DurationDesc,
    DurationAsc,
}

/// A flattened select reference for selects-only view and exports.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelectRef {
    pub clip_index: usize,
    pub clip: ClipId,
    pub segment: Segment,
}

impl Selects {
    /// All selects, ordered. Timeline order = clip order then start.
    pub fn sorted(&self, order: SortOrder) -> Vec<SelectRef> {
        let mut v: Vec<SelectRef> = self
            .clips
            .iter()
            .enumerate()
            .flat_map(|(ci, c)| {
                c.segments.iter().map(move |s| SelectRef { clip_index: ci, clip: c.clip, segment: s.clone() })
            })
            .collect();
        let tl = |a: &SelectRef, b: &SelectRef| {
            a.clip_index.cmp(&b.clip_index).then(a.segment.start_us.cmp(&b.segment.start_us))
        };
        match order {
            SortOrder::Timeline => v.sort_by(tl),
            SortOrder::ScoreDesc => v.sort_by(|a, b| b.segment.mean.total_cmp(&a.segment.mean).then(tl(a, b))),
            SortOrder::ScoreAsc => v.sort_by(|a, b| a.segment.mean.total_cmp(&b.segment.mean).then(tl(a, b))),
            SortOrder::DurationDesc => v.sort_by(|a, b| b.segment.len_us().cmp(&a.segment.len_us()).then(tl(a, b))),
            SortOrder::DurationAsc => v.sort_by(|a, b| a.segment.len_us().cmp(&b.segment.len_us()).then(tl(a, b))),
        }
        v
    }
}

pub struct Engine {
    cal: Calibration,
    cache: HashMap<ClipId, ClipCache>,
    pub counters: Arc<StageCounters>,
}

fn hash_of<T: Hash>(v: T) -> u64 {
    let mut h = KeyHasher::default();
    v.hash(&mut h);
    h.finish()
}

fn overrides_key(o: &[Override]) -> u64 {
    hash_of(o.iter().map(|x| (x.kind as u8, x.start_us, x.end_us)).collect::<Vec<_>>())
}

impl Engine {
    pub fn new(cal: Calibration) -> Self {
        Engine { cal, cache: HashMap::new(), counters: Arc::new(StageCounters::default()) }
    }

    pub fn calibration(&self) -> &Calibration {
        &self.cal
    }

    /// Drops cached stages for clips no longer present.
    pub fn retain(&mut self, ids: &[ClipId]) {
        let keep: std::collections::HashSet<_> = ids.iter().collect();
        self.cache.retain(|k, _| keep.contains(k));
    }

    /// Scores every clip. Returns `None` if `cancel` became true (state stays consistent).
    pub fn score(
        &mut self,
        clips: &[ClipInput],
        s: &Settings,
        overrides: &HashMap<ClipId, Vec<Override>>,
        cancel: &AtomicBool,
    ) -> Option<Selects> {
        let cal = &self.cal;
        let counters = &self.counters;
        let empty: Vec<Override> = Vec::new();
        let results: Vec<Option<(ClipCache, ClipSelects)>> = clips
            .par_iter()
            .map(|clip| {
                if cancel.load(Ordering::Relaxed) {
                    return None;
                }
                let mut c = self.cache.get(&clip.id).cloned().unwrap_or_default();
                let key_norm = hash_of((clip.version, s.centre_weighting.to_bits(), clip.profile as u8, cal.version));
                if c.norm.is_none() || c.key_norm != key_norm {
                    counters.normalise.fetch_add(1, Ordering::Relaxed);
                    c.norm = Some(Arc::new(normalise(clip, s.centre_weighting, cal)));
                    c.key_norm = key_norm;
                    c.frame = None;
                }
                let norm = c.norm.clone()?;
                let key_frame = hash_of((key_norm, s.key_frame()));
                if c.frame.is_none() || c.key_frame != key_frame {
                    counters.frame.fetch_add(1, Ordering::Relaxed);
                    c.frame = Some(Arc::new(frame_scores(&norm, s)));
                    c.key_frame = key_frame;
                    c.runs = None;
                }
                if cancel.load(Ordering::Relaxed) {
                    return None;
                }
                let fs = c.frame.clone()?;
                let t = Timing { time_us: &clip.ms.time_us, rate_hz: clip.ms.rate_hz, duration_us: clip.duration_us };
                let key_runs = hash_of((key_frame, s.key_runs()));
                if c.runs.is_none() || c.key_runs != key_runs {
                    counters.runs.fetch_add(1, Ordering::Relaxed);
                    c.runs = Some(Arc::new(candidate_runs(&fs, s, &t)));
                    c.key_runs = key_runs;
                    c.segs = None;
                }
                let runs = c.runs.clone()?;
                let key_segs = hash_of((key_runs, s.key_segments()));
                if c.segs.is_none() || c.key_segs != key_segs {
                    counters.segments.fetch_add(1, Ordering::Relaxed);
                    c.segs = Some(Arc::new(segments_from_runs(&runs, &fs, s, &t)));
                    c.key_segs = key_segs;
                    c.fin = None;
                }
                let segs = c.segs.clone()?;
                let ovs = overrides.get(&clip.id).unwrap_or(&empty);
                let key_final = hash_of((key_segs, s.max_per_clip, s.handles_samples, overrides_key(ovs), clip.duration_us));
                if c.fin.is_none() || c.key_final != key_final {
                    counters.finalise.fetch_add(1, Ordering::Relaxed);
                    c.fin = Some(Arc::new(finalise((*segs).clone(), &fs, s, &t, ovs)));
                    c.key_final = key_final;
                }
                let fin = c.fin.clone()?;
                Some((c, ClipSelects { clip: clip.id, segments: (*fin).clone(), scores: fs }))
            })
            .collect();
        if cancel.load(Ordering::Relaxed) || results.iter().any(|r| r.is_none()) {
            return None;
        }
        let mut out = Vec::with_capacity(results.len());
        for (cache, sel) in results.into_iter().flatten() {
            self.cache.insert(sel.clip, cache);
            out.push(sel);
        }
        let summary = summarise(clips, &out);
        Some(Selects { clips: out, summary })
    }

    /// Inspector breakdown for one sample (replays S1–S4 with the current settings).
    pub fn explain(&self, clip: &ClipInput, s: &Settings, sample: usize) -> SampleBreakdown {
        let norm = match self.cache.get(&clip.id).and_then(|c| c.norm.clone()) {
            Some(n) => n,
            None => Arc::new(normalise(clip, s.centre_weighting, &self.cal)),
        };
        explain_sample(&norm, s, sample)
    }
}

fn summarise(clips: &[ClipInput], sel: &[ClipSelects]) -> Summary {
    let mut sm = Summary::default();
    for (c, s) in clips.iter().zip(sel) {
        sm.footage_us += c.duration_us;
        sm.segment_count += s.segments.len();
        sm.selects_us += s.segments.iter().map(|x| x.len_us()).sum::<i64>();
    }
    sm.percent = if sm.footage_us > 0 { 100.0 * sm.selects_us as f32 / sm.footage_us as f32 } else { 0.0 };
    sm.flag_runs = FlagKind::ALL
        .iter()
        .map(|f| {
            let runs: usize = sel
                .iter()
                .map(|s| {
                    let fl = &s.scores.flags;
                    (0..fl.len()).filter(|&k| fl[k] & f.bit() != 0 && (k == 0 || fl[k - 1] & f.bit() == 0)).count()
                })
                .sum();
            (*f, runs)
        })
        .filter(|(_, n)| *n > 0)
        .collect();
    sm
}
