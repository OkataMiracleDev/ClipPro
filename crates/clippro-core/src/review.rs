//! Imported-timeline review (SPEC EXP-10, PLAN P11.3/P11.4): relink an NLE timeline to the
//! project, score every clip item over the range it uses, flag weak items, list better
//! alternatives from the same take group (ALGORITHMS §8.4, no-AI variant) or the same clip, and
//! export the edit back with score markers and any accepted swaps.

use clippro_interchange::import::{relink, RelinkReport};
use clippro_interchange::model::{convert_frames, score_color, ClipItem, Item, Marker, MarkerColor, MediaRef, Timeline};
use clippro_measure::columns::MeasurementSet;
use clippro_score::frame::FrameScores;
use clippro_score::Segment;
use clippro_types::{ClipId, FlagKind, Tier};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// One analysed project clip, as the review sees it.
#[derive(Clone)]
pub struct ClipData {
    pub clip: ClipId,
    pub name: String,
    pub media: MediaRef,
    pub ms: Arc<MeasurementSet>,
    pub scores: Arc<FrameScores>,
    pub segments: Vec<Segment>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewOptions {
    /// Items whose mean score is below this are weak.
    pub weak_below: f32,
    /// Alternatives listed per item.
    pub alternatives: usize,
    /// Take-group similarity threshold (ALGORITHMS §8.4, calibrated).
    pub similarity: f32,
    /// An alternative must beat the item's mean by at least this much.
    pub min_gain: f32,
}

impl Default for ReviewOptions {
    fn default() -> Self {
        ReviewOptions { weak_below: 0.55, alternatives: 3, similarity: 0.80, min_gain: 0.02 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AltKind {
    SameClip,
    TakeGroup,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Alternative {
    pub clip: ClipId,
    pub clip_name: String,
    pub segment: Segment,
    pub kind: AltKind,
    /// Take similarity to the item (1 for the same clip).
    pub similarity: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemReview {
    pub track: usize,
    /// Index into the track's items.
    pub item: usize,
    pub name: String,
    pub record_in: i64,
    pub record_out: i64,
    /// The linked, analysed project clip (None: offline or not analysed).
    pub clip: Option<ClipId>,
    /// Used source range, µs from the media start.
    pub src_in_us: i64,
    pub src_out_us: i64,
    pub mean: Option<f32>,
    /// Score bar: up to 48 buckets over the used range.
    pub bars: Vec<f32>,
    /// Flags inside the range that affect scoring.
    pub flags: Vec<FlagKind>,
    /// Contains force-excluded samples.
    pub excluded: bool,
    pub weak: bool,
    pub alternatives: Vec<Alternative>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimelineReview {
    /// The timeline, relinked to project media.
    pub timeline: Timeline,
    pub relink: RelinkReport,
    pub items: Vec<ItemReview>,
}

impl TimelineReview {
    pub fn weak_count(&self) -> usize {
        self.items.iter().filter(|i| i.weak).count()
    }
}

/// Samples whose time falls in [a, b), or the nearest one.
fn sample_range(time_us: &[i64], a: i64, b: i64) -> Option<(usize, usize)> {
    if time_us.is_empty() {
        return None;
    }
    let lo = time_us.partition_point(|t| *t < a);
    let hi = time_us.partition_point(|t| *t < b);
    if hi > lo {
        return Some((lo, hi));
    }
    let k = lo.min(time_us.len() - 1);
    Some((k, k + 1))
}

/// Take descriptor: pHash of the middle sample and the mean 64-bin histogram (sums to 1).
fn descriptor(ms: &MeasurementSet, (a, b): (usize, usize)) -> Option<(u64, [f32; 64])> {
    let mid = (a + b) / 2;
    let ph = *ms.phash.get(mid)?;
    let mut h = [0f32; 64];
    let mut n = 0;
    for k in a..b {
        if let Some(row) = ms.hist64.get(k * 64..k * 64 + 64) {
            for (i, v) in row.iter().enumerate() {
                h[i] += *v as f32;
            }
            n += 1;
        }
    }
    let total: f32 = h.iter().sum();
    if n == 0 || total <= 0.0 {
        return None;
    }
    h.iter_mut().for_each(|v| *v /= total);
    Some((ph, h))
}

/// No-AI take similarity: 0.6·(1 − hamming/64) + 0.4·(1 − χ²).
pub fn similarity(a: &(u64, [f32; 64]), b: &(u64, [f32; 64])) -> f32 {
    let ham = (a.0 ^ b.0).count_ones() as f32;
    let mut chi = 0f32;
    for i in 0..64 {
        let d = a.1[i] - b.1[i];
        chi += d * d / (a.1[i] + b.1[i] + 1e-9);
    }
    0.6 * (1.0 - ham / 64.0) + 0.4 * (1.0 - (0.5 * chi).clamp(0.0, 1.0))
}

fn bars(v: &[f32], n: usize) -> Vec<f32> {
    if v.len() <= n {
        return v.to_vec();
    }
    (0..n)
        .map(|i| {
            let (a, b) = (i * v.len() / n, ((i + 1) * v.len() / n).max(i * v.len() / n + 1));
            v[a..b].iter().sum::<f32>() / (b - a) as f32
        })
        .collect()
}

/// Relinks `tl` to the project clips and reviews every clip item.
pub fn review(mut tl: Timeline, clips: &[ClipData], tier: impl Fn(FlagKind) -> Tier, opts: &ReviewOptions) -> TimelineReview {
    let pool: Vec<MediaRef> = clips.iter().map(|c| c.media.clone()).collect();
    let rep = relink(&mut tl, &pool);
    let linked = |mi: usize| rep.linked.iter().find(|l| l.0 == mi).map(|l| &clips[l.1]);
    // Segment descriptors, computed once.
    let seg_desc: Vec<Vec<Option<(u64, [f32; 64])>>> = clips
        .iter()
        .map(|c| c.segments.iter().map(|s| descriptor(&c.ms, (s.first_sample, s.last_sample + 1))).collect())
        .collect();
    let mut items = Vec::new();
    for (k, track) in tl.video.iter().enumerate() {
        for (j, (it, (rin, rout))) in track.items.iter().zip(tl.positions(track)).enumerate() {
            let Item::Clip(c) = it else { continue };
            let m = &tl.media[c.media];
            let (src_in_us, src_out_us) = (m.rate.frames_to_us(c.src_in), m.rate.frames_to_us(c.src_out));
            let mut r = ItemReview {
                track: k,
                item: j,
                name: c.name.clone(),
                record_in: rin,
                record_out: rout,
                clip: None,
                src_in_us,
                src_out_us,
                mean: None,
                bars: Vec::new(),
                flags: Vec::new(),
                excluded: false,
                weak: false,
                alternatives: Vec::new(),
            };
            let Some(cd) = linked(c.media) else {
                items.push(r);
                continue;
            };
            let Some((a, b)) = sample_range(&cd.ms.time_us, src_in_us, src_out_us) else {
                items.push(r);
                continue;
            };
            let sc = &cd.scores;
            let b = b.min(sc.score.len());
            if a >= b {
                items.push(r);
                continue;
            }
            r.clip = Some(cd.clip);
            let mean = sc.score[a..b].iter().sum::<f32>() / (b - a) as f32;
            r.mean = Some(mean);
            r.bars = bars(&sc.score[a..b], 48);
            let bits = sc.flags[a..b.min(sc.flags.len())].iter().fold(0u32, |x, f| x | f);
            r.flags = FlagKind::ALL.iter().copied().filter(|f| bits & f.bit() != 0 && tier(*f) != Tier::Neutral).collect();
            r.excluded = sc.force_out[a..b.min(sc.force_out.len())].iter().any(|x| *x);
            r.weak = mean < opts.weak_below || r.excluded;
            // Alternatives: better segments of the same clip (elsewhere), or of the same take.
            let me = descriptor(&cd.ms, (a, b));
            let mut alts: Vec<Alternative> = Vec::new();
            for (ci, other) in clips.iter().enumerate() {
                for (si, s) in other.segments.iter().enumerate() {
                    if s.mean < mean + opts.min_gain {
                        continue;
                    }
                    let same_clip = other.clip == cd.clip;
                    if same_clip {
                        // Not the same material: less than half of the shorter span overlaps.
                        let ov = (s.end_us.min(src_out_us) - s.start_us.max(src_in_us)).max(0);
                        if ov * 2 >= (s.end_us - s.start_us).min(src_out_us - src_in_us) {
                            continue;
                        }
                    }
                    let sim = match (&me, &seg_desc[ci][si]) {
                        (Some(x), Some(y)) => similarity(x, y),
                        _ => 0.0,
                    };
                    // Same-clip takes more than 30 s apart also count as a take group.
                    let far = same_clip && (s.start_us - src_in_us).abs() > 30_000_000;
                    let kind = if same_clip && !(far && sim >= opts.similarity) {
                        AltKind::SameClip
                    } else if sim >= opts.similarity {
                        AltKind::TakeGroup
                    } else {
                        continue;
                    };
                    alts.push(Alternative {
                        clip: other.clip,
                        clip_name: other.name.clone(),
                        segment: s.clone(),
                        kind,
                        similarity: if kind == AltKind::SameClip { 1.0 } else { sim },
                    });
                }
            }
            alts.sort_by(|x, y| y.segment.mean.total_cmp(&x.segment.mean).then(y.similarity.total_cmp(&x.similarity)));
            alts.truncate(opts.alternatives);
            r.alternatives = alts;
            items.push(r);
        }
    }
    TimelineReview { timeline: tl, relink: rep, items }
}

impl crate::session::Session {
    /// Every analysed project clip with its measurements, scores and selects under `settings`.
    pub fn review_data(&self, settings: &clippro_score::Settings) -> crate::session::Result<Vec<ClipData>> {
        let inputs = self.clip_inputs()?;
        let sel = self.score(settings)?;
        let db = crate::store::lock(&self.db);
        let rows = db.clips()?;
        let mut out = Vec::new();
        for ci in inputs {
            let Some(row) = rows.iter().find(|r| r.id == ci.id) else { continue };
            let Some(m) = db.media(row.media_id)? else { continue };
            let Ok(info) = serde_json::from_str::<clippro_media::MediaInfo>(&m.probe_json) else { continue };
            let Some(cs) = sel.clips.iter().find(|c| c.clip == ci.id) else { continue };
            out.push(ClipData {
                clip: ci.id,
                name: row.name.clone(),
                media: crate::session::media_ref(&info, m.id.0, &row.name, std::path::PathBuf::from(&m.path)),
                ms: ci.ms.clone(),
                scores: cs.scores.clone(),
                segments: cs.segments.clone(),
            });
        }
        Ok(out)
    }

    /// Relinks and reviews an imported timeline against the project (SPEC EXP-10).
    pub fn review_timeline(&self, tl: Timeline, settings: &clippro_score::Settings, opts: &ReviewOptions) -> crate::session::Result<(TimelineReview, Vec<ClipData>)> {
        let clips = self.review_data(settings)?;
        let rv = review(tl, &clips, |f| settings.tier(f), opts);
        Ok((rv, clips))
    }
}

/// An accepted alternative: review item index → alternative index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Swap {
    pub item: usize,
    pub alternative: usize,
}

const MARKER_PREFIX: &str = "ClipPro ";

/// The edit to send back (P11.4): track structure kept, accepted alternatives swapped in at
/// the item's length (centred on the alternative; a short source leaves a gap so later items
/// keep their sync), and optionally one score marker per reviewed item.
pub fn export_back(rv: &TimelineReview, clips: &[ClipData], swaps: &[Swap], markers: bool) -> Timeline {
    let mut tl = rv.timeline.clone();
    // Apply from the last item of each track so earlier indexes stay valid.
    let mut order: Vec<(usize, Option<&Swap>)> = (0..rv.items.len()).map(|i| (i, swaps.iter().find(|s| s.item == i))).collect();
    order.sort_by_key(|(i, _)| std::cmp::Reverse((rv.items[*i].track, rv.items[*i].item)));
    for (i, swap) in order {
        let r = &rv.items[i];
        let alt = swap.and_then(|s| r.alternatives.get(s.alternative));
        let tl_rate = tl.rate;
        let mut extra_gap = 0;
        if let Some(alt) = alt {
            let Some(cd) = clips.iter().find(|c| c.clip == alt.clip) else { continue };
            let mi = match tl.media.iter().position(|m| m.id == cd.media.id && m.path == cd.media.path) {
                Some(x) => x,
                None => {
                    tl.media.push(cd.media.clone());
                    tl.media.len() - 1
                }
            };
            let m = &tl.media[mi];
            let want = convert_frames(r.record_out - r.record_in, tl_rate, m.rate).max(1);
            let len = want.min(m.duration_frames.max(1));
            let centre = m.rate.us_to_frames_round((alt.segment.start_us + alt.segment.end_us) / 2);
            let src_in = (centre - len / 2).clamp(0, (m.duration_frames - len).max(0));
            if len < want {
                extra_gap = convert_frames(want - len, m.rate, tl_rate);
            }
            let item = ClipItem { media: mi, name: cd.name.clone(), src_in, src_out: src_in + len, markers: Vec::new(), select_index: None, score: Some(alt.segment.mean) };
            if let Some(slot) = tl.video.get_mut(r.track).and_then(|t| t.items.get_mut(r.item)) {
                *slot = Item::Clip(item);
            }
        }
        if markers {
            let mean = alt.map(|a| Some(a.segment.mean)).unwrap_or(r.mean);
            if let Some(Item::Clip(c)) = tl.video.get_mut(r.track).and_then(|t| t.items.get_mut(r.item)) {
                c.markers.retain(|mk| !mk.name.starts_with(MARKER_PREFIX));
                if let Some(mean) = mean {
                    let weak = alt.is_none() && r.weak;
                    let flags: Vec<&str> = if alt.is_none() { r.flags.iter().map(|f| f.label()).collect() } else { Vec::new() };
                    let mut comment = if flags.is_empty() { "No flags".to_string() } else { flags.join(", ") };
                    if let Some(a) = alt {
                        comment = format!("Swapped in from {} ({:?})", a.clip_name, a.kind);
                    } else if weak {
                        comment = format!("Weak: {comment}");
                    }
                    c.markers.push(Marker {
                        at: 0,
                        duration: c.len(),
                        name: format!("{MARKER_PREFIX}{}", (mean * 100.0).round() as i64),
                        comment,
                        color: if weak { MarkerColor::Red } else { score_color(mean) },
                    });
                }
            }
        }
        if extra_gap > 0 {
            if let Some(t) = tl.video.get_mut(r.track) {
                t.items.insert(r.item + 1, Item::Gap(extra_gap));
            }
        }
    }
    tl
}

#[cfg(test)]
mod tests {
    use super::*;
    use clippro_interchange::model::Track;
    use clippro_score::segments::SegmentKind;
    use clippro_types::FrameRate;
    use std::path::PathBuf;

    /// A 20 s clip sampled at 4 Hz with a given score per sample and a flat-ish histogram.
    fn clip(id: i64, name: &str, score: impl Fn(usize) -> f32, ph: u64, tone: usize) -> ClipData {
        let n = 80;
        let mut ms = MeasurementSet::default();
        ms.time_us = (0..n as i64).map(|k| k * 250_000).collect();
        ms.phash = vec![ph; n];
        ms.hist64 = (0..n).flat_map(|_| (0..64).map(move |i| if i / 8 == tone { 30u8 } else { 1 })).collect();
        let s: Vec<f32> = (0..n).map(&score).collect();
        let mut fs = FrameScores { score: s.clone(), overall: s, force_in: vec![false; n], force_out: vec![false; n], flags: vec![0; n], cut_before: vec![false; n] };
        fs.flags[10] = FlagKind::Shaky.bit();
        fs.force_out[60] = true;
        let seg = |a: usize, b: usize| {
            let mean = fs.score[a..=b].iter().sum::<f32>() / (b - a + 1) as f32;
            Segment {
                start_us: a as i64 * 250_000,
                end_us: (b as i64 + 1) * 250_000,
                in_us: a as i64 * 250_000,
                out_us: (b as i64 + 1) * 250_000,
                mean,
                first_sample: a,
                last_sample: b,
                kind: SegmentKind::Auto,
            }
        };
        let segments = vec![seg(20, 39), seg(64, 79)];
        let media = MediaRef {
            id,
            name: name.into(),
            path: PathBuf::from(format!("D:/m/{name}.mov")),
            rate: FrameRate::new(25, 1),
            duration_frames: 500,
            start_tc_frames: 0,
            drop_frame: false,
            width: 1920,
            height: 1080,
            audio_channels: 2,
            reel: None,
        };
        ClipData { clip: ClipId(id), name: name.into(), media, ms: Arc::new(ms), scores: Arc::new(fs), segments }
    }

    fn timeline(clips: &[ClipData], items: Vec<(usize, i64, i64)>) -> Timeline {
        Timeline {
            name: "Cut".into(),
            rate: FrameRate::new(25, 1),
            drop_frame: false,
            start_tc_frames: 90_000,
            width: 1920,
            height: 1080,
            media: clips.iter().map(|c| c.media.clone()).collect(),
            video: vec![Track {
                name: "V1".into(),
                items: items
                    .into_iter()
                    .map(|(m, a, b)| Item::Clip(ClipItem { media: m, name: clips[m].name.clone(), src_in: a, src_out: b, markers: vec![], select_index: None, score: None }))
                    .collect(),
            }],
            with_audio: true,
        }
    }

    fn fixture() -> Vec<ClipData> {
        vec![
            // Take 1: weak start (0.3), good middle (0.8), excluded sample at 15 s.
            clip(1, "take1", |k| if k < 20 { 0.3 } else { 0.8 }, 0xF0F0, 2),
            // Take 2 of the same set-up (same look): better everywhere.
            clip(2, "take2", |_| 0.9, 0xF0F1, 2),
            // A different shot.
            clip(3, "wide", |_| 0.95, 0x0F0F_0F0F_0F0F, 6),
        ]
    }

    #[test]
    fn scores_weak_items_and_finds_alternatives() {
        let clips = fixture();
        // take1 0–2 s (weak, Shaky at 2.5 s? no: sample 10 = 2.5 s is inside 0–4 s), take1 15–16 s (excluded).
        let tl = timeline(&clips, vec![(0, 0, 100), (0, 375, 400), (2, 0, 50)]);
        let rv = review(tl, &clips, |f| if f == FlagKind::Shaky { Tier::Disfavour } else { Tier::Neutral }, &ReviewOptions::default());
        assert!(rv.relink.missing.is_empty());
        assert_eq!(rv.items.len(), 3);
        let a = &rv.items[0];
        assert_eq!(a.clip, Some(ClipId(1)));
        assert!((a.mean.unwrap() - 0.3).abs() < 1e-6 && a.weak && !a.excluded);
        assert_eq!(a.flags, vec![FlagKind::Shaky]);
        assert_eq!(a.bars.len(), 16);
        // Best first: take2 (take group), then take1's own good middle.
        let kinds: Vec<(i64, AltKind)> = a.alternatives.iter().map(|x| (x.clip.0, x.kind)).collect();
        assert_eq!(kinds, vec![(2, AltKind::TakeGroup), (2, AltKind::TakeGroup), (1, AltKind::SameClip)]);
        assert!(!kinds.iter().any(|k| k.0 == 3), "a different shot is not an alternative");
        let b = &rv.items[1];
        assert!(b.excluded && b.weak, "force-excluded sample inside");
        let c = &rv.items[2];
        assert!(!c.weak && c.alternatives.is_empty());
        assert_eq!(rv.weak_count(), 2);
    }

    #[test]
    fn offline_items_are_listed_without_scores() {
        let clips = fixture();
        let mut tl = timeline(&clips, vec![(0, 0, 100)]);
        tl.media[0].path = PathBuf::from("X:/gone.mov");
        tl.media[0].name = "gone".into();
        let rv = review(tl, &clips[1..], |_| Tier::Neutral, &ReviewOptions::default());
        assert_eq!(rv.relink.missing, vec![0]);
        assert_eq!((rv.items[0].clip, rv.items[0].mean), (None, None));
    }

    /// Export back keeps the structure, swaps at the same length and marks the items; the
    /// result survives an XML round trip (SPEC P11.4).
    #[test]
    fn export_back_swaps_and_marks() {
        let clips = fixture();
        let tl = timeline(&clips, vec![(0, 0, 100), (2, 0, 50)]);
        let rv = review(tl, &clips, |_| Tier::Neutral, &ReviewOptions::default());
        let out = export_back(&rv, &clips, &[Swap { item: 0, alternative: 0 }], true);
        assert_eq!(out.video.len(), 1);
        assert_eq!(out.positions(&out.video[0]), rv.timeline.positions(&rv.timeline.video[0]), "record positions unchanged");
        let Item::Clip(c) = &out.video[0].items[0] else { panic!() };
        assert_eq!(out.media[c.media].name, "take2");
        assert_eq!(c.len(), 100);
        assert_eq!(c.markers[0].name, "ClipPro 90");
        assert!(c.markers[0].comment.starts_with("Swapped in from take2"));
        let Item::Clip(w) = &out.video[0].items[1] else { panic!() };
        assert_eq!((w.markers[0].name.as_str(), w.markers[0].color), ("ClipPro 95", MarkerColor::Green));
        // Idempotent markers: exporting the re-imported edit again keeps one ClipPro marker.
        let xml = clippro_interchange::Format::Xmeml.render(&out);
        let back = clippro_interchange::import::import(&xml, FrameRate::new(25, 1)).unwrap();
        let rv2 = review(back, &clips, |_| Tier::Neutral, &ReviewOptions::default());
        let again = export_back(&rv2, &clips, &[], true);
        let Item::Clip(c2) = &again.video[0].items[0] else { panic!() };
        assert_eq!(c2.markers.len(), 1);
        assert_eq!(clippro_interchange::import::edit_signature(&again), clippro_interchange::import::edit_signature(&out));
    }

    #[test]
    fn short_alternative_leaves_a_gap() {
        let mut clips = fixture();
        let mut m = clips[1].media.clone();
        m.duration_frames = 40;
        clips[1].media = m;
        let tl = timeline(&clips, vec![(0, 0, 100), (2, 0, 50)]);
        let rv = review(tl, &clips, |_| Tier::Neutral, &ReviewOptions::default());
        let out = export_back(&rv, &clips, &[Swap { item: 0, alternative: 0 }], false);
        assert_eq!(out.video[0].items.len(), 3);
        assert_eq!(out.video[0].items[1], Item::Gap(60));
        assert_eq!(out.duration(), rv.timeline.duration());
    }
}
