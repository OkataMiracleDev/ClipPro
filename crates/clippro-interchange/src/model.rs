//! NLE-neutral timeline model (SPEC §16.1) and the builder from selects (P7.1).

use clippro_types::{FrameRate, Timecode};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A source media file referenced by clip items.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MediaRef {
    pub id: i64,
    pub name: String,
    pub path: PathBuf,
    pub rate: FrameRate,
    /// Media length in frames.
    pub duration_frames: i64,
    /// Start timecode of the media (frames since midnight at `rate`); 0 if none.
    pub start_tc_frames: i64,
    pub drop_frame: bool,
    pub width: u32,
    pub height: u32,
    pub audio_channels: u32,
    pub reel: Option<String>,
}

impl MediaRef {
    pub fn start_tc(&self) -> Timecode {
        Timecode::from_frames(self.start_tc_frames, self.rate, self.drop_frame)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkerColor {
    Red,
    Orange,
    Yellow,
    Green,
    Cyan,
    Blue,
    Purple,
    Magenta,
    White,
}

impl MarkerColor {
    pub fn name(self) -> &'static str {
        match self {
            MarkerColor::Red => "red",
            MarkerColor::Orange => "orange",
            MarkerColor::Yellow => "yellow",
            MarkerColor::Green => "green",
            MarkerColor::Cyan => "cyan",
            MarkerColor::Blue => "blue",
            MarkerColor::Purple => "purple",
            MarkerColor::Magenta => "magenta",
            MarkerColor::White => "white",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    /// Offset from the start of the clip item (frames at the media rate).
    pub at: i64,
    pub duration: i64,
    pub name: String,
    pub comment: String,
    pub color: MarkerColor,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClipItem {
    pub media: usize,
    pub name: String,
    /// Source range in media frames (0 = first frame of the file).
    pub src_in: i64,
    pub src_out: i64,
    pub markers: Vec<Marker>,
    /// Which select this came from (for reports), if any.
    pub select_index: Option<usize>,
    pub score: Option<f32>,
}

impl ClipItem {
    pub fn len(&self) -> i64 {
        self.src_out - self.src_in
    }
    pub fn is_empty(&self) -> bool {
        self.len() <= 0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Item {
    Clip(ClipItem),
    /// A gap in timeline frames.
    Gap(i64),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub name: String,
    pub items: Vec<Item>,
}

/// A sequence. Track positions are in timeline frames at `rate`; clip items keep their media
/// rate and exporters convert where formats need it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub name: String,
    pub rate: FrameRate,
    pub drop_frame: bool,
    /// Record start timecode (frames at `rate`), conventionally 01:00:00:00.
    pub start_tc_frames: i64,
    pub width: u32,
    pub height: u32,
    pub media: Vec<MediaRef>,
    pub video: Vec<Track>,
    /// Audio channel count per clip follows the media (mirrors V1 when `with_audio`).
    pub with_audio: bool,
}

/// Converts `frames` at `from` rate to frames at `to` rate (rounded).
pub fn convert_frames(frames: i64, from: FrameRate, to: FrameRate) -> i64 {
    if from == to {
        return frames;
    }
    let us = from.frames_to_us(frames);
    to.us_to_frames_round(us)
}

impl Timeline {
    /// Timeline length of an item in timeline frames.
    pub fn item_len(&self, item: &Item) -> i64 {
        match item {
            Item::Gap(n) => *n,
            Item::Clip(c) => convert_frames(c.len(), self.media[c.media].rate, self.rate),
        }
    }

    /// (record_in, record_out) of every item on a track, in timeline frames from 0.
    pub fn positions(&self, track: &Track) -> Vec<(i64, i64)> {
        let mut at = 0;
        track
            .items
            .iter()
            .map(|it| {
                let len = self.item_len(it);
                let p = (at, at + len);
                at += len;
                p
            })
            .collect()
    }

    pub fn duration(&self) -> i64 {
        self.video.iter().map(|t| self.positions(t).last().map(|p| p.1).unwrap_or(0)).max().unwrap_or(0)
    }
}

/// Track layout for exports (SPEC EXP-7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// One track with the selects, one after another.
    #[default]
    SelectsOnly,
    /// Whole clips in order: selects on V1, the rest on V2 underneath.
    SelectsOverRejects,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportOptions {
    pub name: String,
    pub layout: Layout,
    /// Include the handles around each select.
    pub handles: bool,
    /// Gap between selects, seconds.
    pub gap_s: f64,
    pub with_audio: bool,
    /// Marker name and comment templates; tokens: {clip} {index} {score} {flags} {duration} {tc_in} {tc_out}.
    pub marker_name: String,
    pub marker_comment: String,
    pub markers: bool,
    /// Force a timeline rate (else the most common clip rate).
    pub rate: Option<FrameRate>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            name: "ClipPro Selects".into(),
            layout: Layout::SelectsOnly,
            handles: true,
            gap_s: 0.0,
            with_audio: true,
            marker_name: "Select {index} · {score}".into(),
            marker_comment: "{flags}".into(),
            markers: true,
            rate: None,
        }
    }
}

/// One select to place on the timeline (µs relative to the media start).
#[derive(Clone, Debug, PartialEq)]
pub struct SelectInput {
    pub media: usize,
    pub start_us: i64,
    pub end_us: i64,
    pub in_us: i64,
    pub out_us: i64,
    pub score: f32,
    pub flags: Vec<String>,
}

pub fn score_color(score: f32) -> MarkerColor {
    if score >= 0.75 {
        MarkerColor::Green
    } else if score >= 0.55 {
        MarkerColor::Yellow
    } else {
        MarkerColor::Red
    }
}

fn fill(tpl: &str, clip: &str, index: usize, s: &SelectInput, m: &MediaRef, from: i64, to: i64) -> String {
    let tc = |f: i64| Timecode::from_frames(m.start_tc_frames + f, m.rate, m.drop_frame).to_string();
    tpl.replace("{clip}", clip)
        .replace("{index}", &(index + 1).to_string())
        .replace("{score}", &format!("{}", (s.score * 100.0).round() as i64))
        .replace("{flags}", &if s.flags.is_empty() { "No flags".to_string() } else { s.flags.join(", ") })
        .replace("{duration}", &format!("{:.1}s", (s.end_us - s.start_us) as f64 / 1e6))
        .replace("{tc_in}", &tc(from))
        .replace("{tc_out}", &tc(to))
}

/// Builds a timeline from selects (already in the desired order).
pub fn build(media: Vec<MediaRef>, selects: &[SelectInput], opts: &ExportOptions) -> Timeline {
    let rate = opts.rate.unwrap_or_else(|| {
        let mut counts: Vec<(FrameRate, usize)> = Vec::new();
        for s in selects {
            let r = media[s.media].rate;
            match counts.iter_mut().find(|c| c.0 == r) {
                Some(c) => c.1 += 1,
                None => counts.push((r, 1)),
            }
        }
        counts.iter().max_by_key(|c| c.1).map(|c| c.0).or_else(|| media.first().map(|m| m.rate)).unwrap_or_default()
    });
    let drop = rate.supports_drop_frame() && media.iter().any(|m| m.drop_frame);
    let (width, height) = media.first().map(|m| (m.width, m.height)).unwrap_or((1920, 1080));
    let item_for = |i: usize, s: &SelectInput| -> ClipItem {
        let m = &media[s.media];
        let (a, b) = if opts.handles { (s.in_us, s.out_us) } else { (s.start_us, s.end_us) };
        let src_in = m.rate.us_to_frames_round(a).clamp(0, m.duration_frames);
        let src_out = m.rate.us_to_frames_round(b).clamp(src_in + 1, m.duration_frames.max(src_in + 1));
        let core_in = m.rate.us_to_frames_round(s.start_us) - src_in;
        let core_len = m.rate.us_to_frames_round(s.end_us) - m.rate.us_to_frames_round(s.start_us);
        let markers = if opts.markers {
            vec![Marker {
                at: core_in.max(0),
                duration: core_len.max(1),
                name: fill(&opts.marker_name, &m.name, i, s, m, src_in, src_out),
                comment: fill(&opts.marker_comment, &m.name, i, s, m, src_in, src_out),
                color: score_color(s.score),
            }]
        } else {
            Vec::new()
        };
        ClipItem { media: s.media, name: m.name.clone(), src_in, src_out, markers, select_index: Some(i), score: Some(s.score) }
    };
    let mut video = Vec::new();
    match opts.layout {
        Layout::SelectsOnly => {
            let gap = (opts.gap_s * rate.fps()).round() as i64;
            let mut items = Vec::new();
            for (i, s) in selects.iter().enumerate() {
                if i > 0 && gap > 0 {
                    items.push(Item::Gap(gap));
                }
                items.push(Item::Clip(item_for(i, s)));
            }
            video.push(Track { name: "Selects".into(), items });
        }
        Layout::SelectsOverRejects => {
            // Whole clips in media order; V1 holds selects at their positions, V2 the rest.
            let mut v1 = Vec::new();
            let mut v2 = Vec::new();
            let mut order: Vec<usize> = Vec::new();
            for s in selects {
                if !order.contains(&s.media) {
                    order.push(s.media);
                }
            }
            for mi in order {
                let m = &media[mi];
                let mut sel: Vec<(usize, &SelectInput)> = selects.iter().enumerate().filter(|(_, s)| s.media == mi).collect();
                sel.sort_by_key(|(_, s)| s.start_us);
                let mut cursor = 0i64; // media frames
                for (i, s) in sel {
                    let it = item_for(i, s);
                    if it.src_in > cursor {
                        let gap = convert_frames(it.src_in - cursor, m.rate, rate);
                        v1.push(Item::Gap(gap));
                        v2.push(Item::Clip(ClipItem {
                            media: mi,
                            name: m.name.clone(),
                            src_in: cursor,
                            src_out: it.src_in,
                            markers: vec![],
                            select_index: None,
                            score: None,
                        }));
                    }
                    let len = convert_frames(it.len(), m.rate, rate);
                    v2.push(Item::Gap(len));
                    cursor = it.src_out.max(cursor);
                    v1.push(Item::Clip(it));
                }
                if cursor < m.duration_frames {
                    let gap = convert_frames(m.duration_frames - cursor, m.rate, rate);
                    v1.push(Item::Gap(gap));
                    v2.push(Item::Clip(ClipItem {
                        media: mi,
                        name: m.name.clone(),
                        src_in: cursor,
                        src_out: m.duration_frames,
                        markers: vec![],
                        select_index: None,
                        score: None,
                    }));
                }
            }
            video.push(Track { name: "Selects".into(), items: v1 });
            video.push(Track { name: "Rejects".into(), items: v2 });
        }
    }
    let start_tc_frames = Timecode::parse(if drop { "01:00:00;00" } else { "01:00:00:00" }, rate.timecode_base()).map(|t| t.frames).unwrap_or(0);
    Timeline { name: opts.name.clone(), rate, drop_frame: drop, start_tc_frames, width, height, media, video, with_audio: opts.with_audio }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn media(n: usize) -> Vec<MediaRef> {
        (0..n)
            .map(|i| MediaRef {
                id: i as i64 + 1,
                name: format!("A00{}C00{}", i + 1, i + 3),
                path: PathBuf::from(format!("D:/Footage/Day 1/A00{}C00{}.mov", i + 1, i + 3)),
                rate: FrameRate::new(25, 1),
                duration_frames: 250,
                start_tc_frames: 25 * 3600 * 10 + i as i64 * 1000,
                drop_frame: false,
                width: 1920,
                height: 1080,
                audio_channels: 2,
                reel: Some(format!("A00{}", i + 1)),
            })
            .collect()
    }

    pub fn selects() -> Vec<SelectInput> {
        vec![
            SelectInput { media: 0, start_us: 1_000_000, end_us: 4_000_000, in_us: 250_000, out_us: 4_750_000, score: 0.82, flags: vec![] },
            SelectInput { media: 0, start_us: 6_000_000, end_us: 8_000_000, in_us: 5_250_000, out_us: 8_750_000, score: 0.61, flags: vec!["Shaky".into()] },
            SelectInput { media: 1, start_us: 0, end_us: 5_000_000, in_us: 0, out_us: 5_750_000, score: 0.9, flags: vec![] },
        ]
    }

    #[test]
    fn selects_only_layout() {
        let t = build(media(2), &selects(), &ExportOptions { gap_s: 1.0, ..ExportOptions::default() });
        assert_eq!(t.rate, FrameRate::new(25, 1));
        let v1 = &t.video[0];
        // 3 clips + 2 gaps of 25 frames
        assert_eq!(v1.items.len(), 5);
        let Item::Clip(c) = &v1.items[0] else { panic!() };
        assert_eq!((c.src_in, c.src_out), (6, 119));
        assert_eq!(c.markers[0].at, 19);
        assert_eq!(c.markers[0].duration, 75);
        assert_eq!(c.markers[0].name, "Select 1 · 82");
        assert_eq!(c.markers[0].color, MarkerColor::Green);
        assert_eq!(t.duration(), 113 + 25 + 88 + 25 + 144);
        assert_eq!(t.start_tc_frames, 90_000);
    }

    #[test]
    fn selects_over_rejects_covers_whole_clips() {
        let t = build(media(2), &selects(), &ExportOptions { layout: Layout::SelectsOverRejects, ..ExportOptions::default() });
        assert_eq!(t.video.len(), 2);
        let p1 = t.positions(&t.video[0]);
        let p2 = t.positions(&t.video[1]);
        assert_eq!(p1.last().unwrap().1, 500, "both clips in full on V1 (with gaps)");
        assert_eq!(p2.last().unwrap().1, 500);
        // V1 clips and V2 clips never overlap in time.
        for (i, it) in t.video[0].items.iter().enumerate() {
            if let Item::Clip(_) = it {
                for (j, it2) in t.video[1].items.iter().enumerate() {
                    if let Item::Clip(_) = it2 {
                        assert!(p1[i].1 <= p2[j].0 || p2[j].1 <= p1[i].0);
                    }
                }
            }
        }
    }

    #[test]
    fn without_handles_uses_core_span() {
        let t = build(media(2), &selects(), &ExportOptions { handles: false, ..ExportOptions::default() });
        let Item::Clip(c) = &t.video[0].items[0] else { panic!() };
        assert_eq!((c.src_in, c.src_out), (25, 100));
        assert_eq!(c.markers[0].at, 0);
    }

    #[test]
    fn mixed_rates_convert() {
        let mut m = media(2);
        m[1].rate = FrameRate::new(50, 1);
        m[1].duration_frames = 500;
        let t = build(m, &selects(), &ExportOptions::default());
        assert_eq!(t.rate, FrameRate::new(25, 1), "most common rate wins");
        let Item::Clip(c) = &t.video[0].items[2] else { panic!() };
        assert_eq!(c.len(), 288);
        assert_eq!(t.item_len(&t.video[0].items[2]), 144);
    }
}
