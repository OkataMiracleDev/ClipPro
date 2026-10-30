//! FCPXML reader (1.8–1.11): Final Cut Pro and Resolve projects. Reads the primary storyline
//! and connected clips (positive lanes become upper tracks); audio lanes and nested
//! storylines are skipped.

use super::dom::{parse, Node};
use super::{intern, non_empty, place, url_to_path};
use crate::model::{ClipItem, Marker, MarkerColor, MediaRef, Timeline, Track};
use clippro_types::{FrameRate, Rational};

/// "1001/30000s", "5s", "0s" or "2.5s" → seconds.
pub fn secs(s: &str) -> Option<Rational> {
    let v = s.trim().strip_suffix('s')?;
    if let Some((n, d)) = v.split_once('/') {
        return Some(Rational::new(n.parse().ok()?, d.parse().ok()?));
    }
    if let Ok(n) = v.parse::<i64>() {
        return Some(Rational::from_int(n));
    }
    let f: f64 = v.parse().ok()?;
    Some(Rational::new((f * 1_000_000.0).round() as i64, 1_000_000))
}

/// Seconds → frames at `rate` (nearest).
pub fn frames(t: Rational, rate: FrameRate) -> i64 {
    let n = t.num as i128 * rate.0.num as i128;
    let d = t.den as i128 * rate.0.den as i128;
    (if n >= 0 { (2 * n + d) / (2 * d) } else { -((-2 * n + d) / (2 * d)) }) as i64
}

fn time(n: &Node, k: &str) -> Rational {
    n.attr(k).and_then(secs).unwrap_or(Rational::ZERO)
}

struct Format {
    id: String,
    rate: Option<FrameRate>,
    width: u32,
    height: u32,
}

struct Asset {
    id: String,
    media: usize,
}

struct Ctx {
    formats: Vec<Format>,
    assets: Vec<Asset>,
}

impl Ctx {
    fn format(&self, id: Option<&str>) -> Option<&Format> {
        self.formats.iter().find(|f| Some(f.id.as_str()) == id)
    }
}

fn markers(el: &Node, clip_start: Rational, rate: FrameRate) -> Vec<Marker> {
    let base = frames(clip_start, rate);
    el.children
        .iter()
        .filter(|c| c.name == "marker" || c.name == "chapter-marker")
        .map(|m| Marker {
            at: (frames(time(m, "start"), rate) - base).max(0),
            duration: frames(time(m, "duration"), rate),
            name: m.attr("value").unwrap_or("").to_string(),
            comment: m.attr("note").unwrap_or("").to_string(),
            color: if m.attr("completed") == Some("1") { MarkerColor::Green } else { MarkerColor::Blue },
        })
        .collect()
}

/// The clip an element stands for, with its source start (seconds, media time).
fn clip_of(tl: &Timeline, ctx: &Ctx, el: &Node) -> Option<ClipItem> {
    let (src, start) = match el.name.as_str() {
        "asset-clip" | "video" => (el, time(el, "start")),
        // A wrapper whose local time runs through a nested asset at that asset's offset.
        "clip" | "sync-clip" => {
            let inner = el.children.iter().find(|c| (c.name == "video" || c.name == "asset-clip") && c.attr("ref").is_some())?;
            (inner, time(el, "start").sub(time(inner, "offset")).add(time(inner, "start")))
        }
        _ => return None,
    };
    let asset = ctx.assets.iter().find(|a| Some(a.id.as_str()) == src.attr("ref"))?;
    let m = &tl.media[asset.media];
    let src_in = frames(start, m.rate) - m.start_tc_frames;
    let len = frames(time(el, "duration"), m.rate);
    if len <= 0 {
        return None;
    }
    let name = el.attr("name").unwrap_or(&m.name).to_string();
    Some(ClipItem { media: asset.media, name, src_in, src_out: src_in + len, markers: markers(el, start, m.rate), select_index: None, score: None })
}

pub fn import(xml: &str) -> Result<Timeline, String> {
    let root = parse(xml)?;
    if root.name != "fcpxml" {
        return Err("not an FCPXML document".into());
    }
    let res = root.child("resources").ok_or("FCPXML without resources")?;
    let mut ctx = Ctx { formats: Vec::new(), assets: Vec::new() };
    for f in res.all("format") {
        let rate = f.attr("frameDuration").and_then(secs).filter(|d| d.num > 0).map(|d| FrameRate(Rational::new(d.den, d.num)));
        let dim = |k| f.attr(k).and_then(|v| v.parse().ok()).unwrap_or(0);
        ctx.formats.push(Format { id: f.attr("id").unwrap_or("").into(), rate, width: dim("width"), height: dim("height") });
    }
    let mut seqs = Vec::new();
    root.find("sequence", &mut seqs);
    let seq = *seqs.first().ok_or("the FCPXML has no sequence")?;
    let sf = ctx.format(seq.attr("format"));
    let rate = sf.and_then(|f| f.rate).unwrap_or_default();
    let (width, height) = sf.map(|f| (f.width, f.height)).filter(|d| d.0 > 0).unwrap_or((1920, 1080));
    let tc_start = time(seq, "tcStart");
    let mut projects = Vec::new();
    root.find("project", &mut projects);
    let name = projects.first().and_then(|p| p.attr("name")).unwrap_or("Imported project").to_string();
    let drop_frame = seq.attr("tcFormat") == Some("DF") && rate.supports_drop_frame();
    let mut tl = Timeline { name, rate, drop_frame, start_tc_frames: frames(tc_start, rate), width, height, media: Vec::new(), video: Vec::new(), with_audio: false };
    for a in res.all("asset") {
        let f = ctx.format(a.attr("format"));
        let mrate = f.and_then(|f| f.rate).unwrap_or(rate);
        let src = a.child("media-rep").and_then(|r| r.attr("src")).or(a.attr("src")).unwrap_or("");
        let m = MediaRef {
            id: ctx.assets.len() as i64 + 1,
            name: a.attr("name").unwrap_or("").to_string(),
            path: url_to_path(src),
            rate: mrate,
            duration_frames: frames(time(a, "duration"), mrate),
            start_tc_frames: frames(time(a, "start"), mrate),
            drop_frame: false,
            width: f.map(|f| f.width).unwrap_or(0),
            height: f.map(|f| f.height).unwrap_or(0),
            audio_channels: if a.attr("hasAudio") == Some("1") { a.attr("audioChannels").and_then(|c| c.parse().ok()).unwrap_or(2) } else { 0 },
            reel: None,
        };
        let media = intern(&mut tl.media, m);
        ctx.assets.push(Asset { id: a.attr("id").unwrap_or("").into(), media });
    }
    tl.with_audio = tl.media.iter().any(|m| m.audio_channels > 0);
    let spine = seq.child("spine").ok_or("the sequence has no spine")?;
    let mut lanes: Vec<Vec<(i64, ClipItem)>> = vec![Vec::new()];
    for el in &spine.children {
        let rec = frames(time(el, "offset").sub(tc_start), rate);
        if let Some(c) = clip_of(&tl, &ctx, el) {
            // Drop-frame flag of the media, as the clip declares it.
            if el.attr("tcFormat") == Some("DF") {
                tl.media[c.media].drop_frame = tl.media[c.media].rate.supports_drop_frame();
            }
            lanes[0].push((rec, c));
        }
        let parent_start = time(el, "start");
        for cc in &el.children {
            let lane: i64 = cc.attr("lane").and_then(|l| l.parse().ok()).unwrap_or(0);
            if lane <= 0 {
                continue;
            }
            let Some(c) = clip_of(&tl, &ctx, cc) else { continue };
            let at = rec + frames(time(cc, "offset").sub(parent_start), rate);
            let lane = lane as usize;
            if lanes.len() <= lane {
                lanes.resize_with(lane + 1, Vec::new);
            }
            lanes[lane].push((at, c));
        }
    }
    let video: Vec<Track> = lanes.into_iter().enumerate().map(|(k, c)| Track { name: format!("V{}", k + 1), items: place(&tl, c) }).collect();
    tl.video = non_empty(video);
    Ok(tl)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_strings() {
        assert_eq!(secs("1001/30000s"), Some(Rational::new(1001, 30000)));
        assert_eq!(secs("3600s"), Some(Rational::from_int(3600)));
        assert_eq!(secs("2.5s"), Some(Rational::new(5, 2)));
        assert_eq!(secs("12"), None);
        assert_eq!(frames(Rational::new(1001, 1000), FrameRate::new(30000, 1001)), 30);
        assert_eq!(frames(Rational::new(13, 25), FrameRate::new(25, 1)), 13);
        assert_eq!(frames(Rational::new(-13, 25), FrameRate::new(25, 1)), -13);
    }
}
