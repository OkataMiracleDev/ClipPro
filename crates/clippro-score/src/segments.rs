//! Stages S5–S7: candidate runs, segments, handles, per-clip cap and manual overrides
//! (ALGORITHMS §7).

use crate::frame::FrameScores;
use crate::preset::LongSegmentMode;
use crate::settings::Settings;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentKind {
    Auto,
    /// Kept because a flag tier forced a sample in.
    Forced,
    /// Added or forced in by the user.
    Manual,
}

/// One select.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    /// Core span (µs from clip start), used for length rules and totals.
    pub start_us: i64,
    pub end_us: i64,
    /// Span including handles, used for exports.
    pub in_us: i64,
    pub out_us: i64,
    pub mean: f32,
    pub first_sample: usize,
    pub last_sample: usize,
    pub kind: SegmentKind,
}

impl Segment {
    pub fn len_us(&self) -> i64 {
        self.end_us - self.start_us
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverrideKind {
    ForceIn,
    ForceOut,
    ManualSegment,
    Trim,
}

/// A manual override anchored to a time range so it survives re-scoring.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Override {
    pub kind: OverrideKind,
    pub start_us: i64,
    pub end_us: i64,
}

/// Sample timing for one clip.
pub struct Timing<'a> {
    pub time_us: &'a [i64],
    pub rate_hz: f64,
    pub duration_us: i64,
}

impl Timing<'_> {
    fn half_us(&self) -> i64 {
        (0.5e6 / self.rate_hz.max(1e-6)).round() as i64
    }
    fn span(&self, a: usize, b: usize) -> (i64, i64) {
        let h = self.half_us();
        ((self.time_us[a] - h).max(0), (self.time_us[b] + h).min(self.duration_us.max(self.time_us[b])))
    }
    /// Boundary time of a hard cut before sample `k`.
    fn cut_time(&self, k: usize) -> i64 {
        (self.time_us[k] - self.half_us()).max(0)
    }
}

/// S5: candidate runs (inclusive sample ranges).
pub fn candidate_runs(fs: &FrameScores, s: &Settings, t: &Timing) -> Vec<(usize, usize)> {
    let n = fs.score.len();
    let cand: Vec<bool> =
        (0..n).map(|k| (fs.score[k] >= s.score_lo || fs.force_in[k]) && !fs.force_out[k]).collect();
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut k = 0;
    while k < n {
        if !cand[k] {
            k += 1;
            continue;
        }
        let a = k;
        while k + 1 < n && cand[k + 1] && !fs.cut_before[k + 1] {
            k += 1;
        }
        runs.push((a, k));
        k += 1;
    }
    // Merge across short gaps without force-excluded samples or cuts.
    let g = (s.gap_s * t.rate_hz).round() as usize;
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for r in runs {
        if let Some(last) = merged.last_mut() {
            let gap = r.0 - last.1 - 1;
            let clean = (last.1 + 1..=r.0).all(|i| !fs.cut_before[i])
                && (last.1 + 1..r.0).all(|i| !fs.force_out[i]);
            if gap <= g && clean {
                last.1 = r.1;
                continue;
            }
        }
        merged.push(r);
    }
    merged
}

fn mean_range(score: &[f32], a: usize, b: usize) -> f32 {
    let mut s = 0f64;
    for v in &score[a..=b] {
        s += *v as f64;
    }
    (s / (b - a + 1) as f64) as f32
}

/// Best window of `m` samples within [a, b] by mean score (ties: earliest).
fn best_window(score: &[f32], a: usize, b: usize, m: usize) -> usize {
    let mut sum: f64 = score[a..a + m].iter().map(|v| *v as f64).sum();
    let (mut best, mut bi) = (sum, a);
    for i in a + 1..=b + 1 - m {
        sum += score[i + m - 1] as f64 - score[i - 1] as f64;
        if sum > best + 1e-9 {
            best = sum;
            bi = i;
        }
    }
    bi
}

/// S6: segments from runs (length rules and score range; handles are added in S7).
pub fn segments_from_runs(runs: &[(usize, usize)], fs: &FrameScores, s: &Settings, t: &Timing) -> Vec<Segment> {
    let min_us = (s.len_min_s * 1e6).round() as i64;
    let max_us = s.len_max_s.map(|m| (m * 1e6).round() as i64);
    let mut out = Vec::new();
    fn process(
        a: usize,
        b: usize,
        fs: &FrameScores,
        s: &Settings,
        t: &Timing,
        min_us: i64,
        max_us: Option<i64>,
        out: &mut Vec<Segment>,
    ) {
        let (st, en) = t.span(a, b);
        if en - st < min_us {
            return;
        }
        if let Some(mx) = max_us {
            if en - st > mx {
                let m = (((mx as f64) / 1e6 * t.rate_hz).floor() as usize).clamp(1, b - a + 1);
                let i = best_window(&fs.score, a, b, m);
                let (ws, we) = t.span(i, i + m - 1);
                if we - ws >= min_us {
                    push(i, i + m - 1, fs, s, t, out);
                }
                if s.long_mode == LongSegmentMode::Split {
                    if i > a {
                        process(a, i - 1, fs, s, t, min_us, max_us, out);
                    }
                    if i + m <= b {
                        process(i + m, b, fs, s, t, min_us, max_us, out);
                    }
                }
                return;
            }
        }
        push(a, b, fs, s, t, out);
    }
    fn push(a: usize, b: usize, fs: &FrameScores, s: &Settings, t: &Timing, out: &mut Vec<Segment>) {
        let mean = mean_range(&fs.score, a, b);
        let forced = fs.force_in[a..=b].iter().any(|x| *x);
        if !forced && (mean < s.score_lo - 1e-6 || mean > s.score_hi + 1e-6) {
            return;
        }
        let (st, en) = t.span(a, b);
        out.push(Segment {
            start_us: st,
            end_us: en,
            in_us: st,
            out_us: en,
            mean,
            first_sample: a,
            last_sample: b,
            kind: if forced { SegmentKind::Forced } else { SegmentKind::Auto },
        });
    }
    for &(a, b) in runs {
        process(a, b, fs, s, t, min_us, max_us, &mut out);
    }
    out.sort_by_key(|x| x.start_us);
    out
}

/// S7: per-clip cap, overrides, handles. Returns segments in timeline order.
pub fn finalise(
    mut segs: Vec<Segment>,
    fs: &FrameScores,
    s: &Settings,
    t: &Timing,
    overrides: &[Override],
) -> Vec<Segment> {
    // Per-clip cap (forced segments are exempt and don't count).
    if let Some(cap) = s.max_per_clip {
        let mut ranked: Vec<usize> = (0..segs.len()).filter(|&i| segs[i].kind == SegmentKind::Auto).collect();
        ranked.sort_by(|&i, &j| {
            segs[j]
                .mean
                .total_cmp(&segs[i].mean)
                .then(segs[j].len_us().cmp(&segs[i].len_us()))
                .then(segs[i].start_us.cmp(&segs[j].start_us))
        });
        let drop: std::collections::HashSet<usize> = ranked.into_iter().skip(cap as usize).collect();
        segs = segs.into_iter().enumerate().filter(|(i, _)| !drop.contains(i)).map(|(_, s)| s).collect();
    }
    apply_overrides(&mut segs, fs, t, overrides);
    segs.sort_by_key(|x| x.start_us);
    add_handles(&mut segs, fs, s, t);
    segs
}

fn samples_in(t: &Timing, a: i64, b: i64) -> Option<(usize, usize)> {
    let h = t.half_us();
    let first = t.time_us.iter().position(|&x| x + h > a)?;
    let last = t.time_us.iter().rposition(|&x| x - h < b)?;
    (first <= last).then_some((first, last))
}

fn apply_overrides(segs: &mut Vec<Segment>, fs: &FrameScores, t: &Timing, overrides: &[Override]) {
    for o in overrides {
        let (a, b) = (o.start_us.min(o.end_us), o.start_us.max(o.end_us).min(t.duration_us.max(o.end_us)));
        if b <= a {
            continue;
        }
        match o.kind {
            OverrideKind::ForceOut => segs.retain(|sg| {
                let ov = (sg.end_us.min(b) - sg.start_us.max(a)).max(0);
                (ov as f64) < 0.5 * sg.len_us() as f64
            }),
            OverrideKind::ForceIn | OverrideKind::ManualSegment => {
                let (mut st, mut en) = (a, b);
                segs.retain(|sg| {
                    let overlaps = sg.start_us < en && sg.end_us > st;
                    if overlaps {
                        st = st.min(sg.start_us);
                        en = en.max(sg.end_us);
                    }
                    !overlaps
                });
                let (fsamp, lsamp, mean) = match samples_in(t, st, en) {
                    Some((f, l)) => (f, l, mean_range(&fs.score, f, l)),
                    None => (0, 0, 0.0),
                };
                segs.push(Segment {
                    start_us: st,
                    end_us: en,
                    in_us: st,
                    out_us: en,
                    mean,
                    first_sample: fsamp,
                    last_sample: lsamp,
                    kind: SegmentKind::Manual,
                });
            }
            OverrideKind::Trim => {
                // Applies to the segment overlapping the trim range the most.
                let best = segs
                    .iter()
                    .enumerate()
                    .map(|(i, sg)| (i, (sg.end_us.min(b) - sg.start_us.max(a)).max(0)))
                    .filter(|(_, ov)| *ov > 0)
                    .max_by_key(|(_, ov)| *ov);
                if let Some((i, _)) = best {
                    let sg = &mut segs[i];
                    sg.start_us = a;
                    sg.end_us = b;
                    if let Some((f, l)) = samples_in(t, a, b) {
                        sg.first_sample = f;
                        sg.last_sample = l;
                        sg.mean = mean_range(&fs.score, f, l);
                    }
                    sg.kind = SegmentKind::Manual;
                }
            }
        }
    }
}

fn add_handles(segs: &mut [Segment], fs: &FrameScores, s: &Settings, t: &Timing) {
    let h = ((s.handles_samples as f64) * 1e6 / t.rate_hz.max(1e-6)).round() as i64;
    let n = t.time_us.len();
    for sg in segs.iter_mut() {
        // Nearest hard cut at or before the first sample, and after the last.
        let left = (0..=sg.first_sample.min(n.saturating_sub(1)))
            .rev()
            .find(|&k| fs.cut_before[k] && k > 0)
            .map(|k| t.cut_time(k))
            .unwrap_or(0);
        let right = (sg.last_sample + 1..n).find(|&k| fs.cut_before[k]).map(|k| t.cut_time(k)).unwrap_or(t.duration_us);
        sg.in_us = (sg.start_us - h).max(left).max(0).min(sg.start_us);
        sg.out_us = (sg.end_us + h).min(right).min(t.duration_us.max(sg.end_us)).max(sg.end_us);
    }
    for i in 1..segs.len() {
        let (l, r) = segs.split_at_mut(i);
        let (a, b) = (&mut l[i - 1], &mut r[0]);
        if a.out_us > b.in_us {
            let mid = ((a.end_us + b.start_us) / 2).clamp(a.end_us, b.start_us.max(a.end_us));
            a.out_us = a.out_us.min(mid).max(a.end_us);
            b.in_us = b.in_us.max(mid).min(b.start_us);
        }
    }
}
