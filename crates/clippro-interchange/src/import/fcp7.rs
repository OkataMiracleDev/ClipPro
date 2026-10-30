//! FCP7 XML (`xmeml`) reader: Premiere Pro, Resolve and FCP7 sequences.

use super::dom::{parse, Node};
use super::{intern, non_empty, place, stem, url_to_path};
use crate::model::{convert_frames, ClipItem, Marker, MarkerColor, MediaRef, Timeline, Track};
use clippro_types::{FrameRate, Timecode};

/// `<rate><timebase>30</timebase><ntsc>TRUE</ntsc></rate>` under `n`.
pub fn rate_of(n: &Node) -> Option<FrameRate> {
    let r = n.child("rate")?;
    let base = r.int_at("timebase")?.max(1);
    let ntsc = r.text_at("ntsc").is_some_and(|s| s.eq_ignore_ascii_case("true"));
    Some(if ntsc { FrameRate::new(base * 1000, 1001) } else { FrameRate::new(base, 1) })
}

/// (frames, drop) of the `<timecode>` under `n`.
fn timecode(n: &Node, rate: FrameRate) -> (i64, bool) {
    let Some(tc) = n.child("timecode") else { return (0, false) };
    let drop = tc.text_at("displayformat").is_some_and(|d| d.eq_ignore_ascii_case("DF"));
    let frames = tc
        .int_at("frame")
        .or_else(|| Timecode::parse(tc.text_at("string")?, rate.timecode_base()).ok().map(|t| t.frames))
        .unwrap_or(0);
    (frames, drop && rate.supports_drop_frame())
}

fn media_ref(def: Option<&Node>, ci: &Node, fallback_rate: FrameRate, index: usize) -> MediaRef {
    let f = def.unwrap_or(ci);
    let rate = rate_of(f).or_else(|| rate_of(ci)).unwrap_or(fallback_rate);
    let (start_tc_frames, drop_frame) = timecode(f, rate);
    let path = f.text_at("pathurl").map(url_to_path).unwrap_or_default();
    let name = f
        .text_at("name")
        .filter(|n| !n.is_empty())
        .map(stem)
        .or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .or_else(|| ci.text_at("name").map(str::to_string))
        .unwrap_or_default();
    let id = f.attr("id").and_then(|id| id.trim_start_matches(|c: char| !c.is_ascii_digit()).parse().ok()).unwrap_or(index as i64 + 1);
    let sc = f.at("media/video/samplecharacteristics");
    MediaRef {
        id,
        name,
        path,
        rate,
        duration_frames: f.int_at("duration").or_else(|| ci.int_at("duration")).unwrap_or(0),
        start_tc_frames,
        drop_frame,
        width: sc.and_then(|s| s.int_at("width")).unwrap_or(1920) as u32,
        height: sc.and_then(|s| s.int_at("height")).unwrap_or(1080) as u32,
        audio_channels: f.at("media/audio").map(|a| a.int_at("channelcount").unwrap_or(2)).unwrap_or(0) as u32,
        reel: f.text_at("timecode/reel/name").filter(|r| !r.is_empty()).map(str::to_string),
    }
}

fn markers(ci: &Node, src_in: i64) -> Vec<Marker> {
    ci.all("marker")
        .filter_map(|m| {
            let at = m.int_at("in")?;
            let out = m.int_at("out").unwrap_or(-1);
            Some(Marker {
                at: (at - src_in).max(0),
                duration: if out > at { out - at } else { 0 },
                name: m.text_at("name").unwrap_or("").to_string(),
                comment: m.text_at("comment").unwrap_or("").to_string(),
                color: m.text_at("color").and_then(MarkerColor::parse).unwrap_or(MarkerColor::Green),
            })
        })
        .collect()
}

pub fn import(xml: &str) -> Result<Timeline, String> {
    let root = parse(xml)?;
    if root.name != "xmeml" {
        return Err("not an FCP7 XML (xmeml) document".into());
    }
    let mut seqs = Vec::new();
    root.find("sequence", &mut seqs);
    let seq = *seqs.first().ok_or("the XML has no sequence")?;
    let rate = rate_of(seq).unwrap_or_default();
    let (start_tc_frames, drop_frame) = timecode(seq, rate);
    let fmt = seq.at("media/video/format/samplecharacteristics");
    // Files are described in full once (anywhere in the document) and referenced by id after.
    let mut files = Vec::new();
    root.find("file", &mut files);
    let defs: Vec<(&str, &Node)> = files.iter().filter(|f| !f.children.is_empty()).filter_map(|f| Some((f.attr("id")?, *f))).collect();
    let mut tl = Timeline {
        name: seq.text_at("name").unwrap_or("Imported sequence").to_string(),
        rate,
        drop_frame,
        start_tc_frames,
        width: fmt.and_then(|f| f.int_at("width")).unwrap_or(1920) as u32,
        height: fmt.and_then(|f| f.int_at("height")).unwrap_or(1080) as u32,
        media: Vec::new(),
        video: Vec::new(),
        with_audio: seq.at("media/audio").is_some_and(|a| a.all("track").next().is_some()),
    };
    let mut by_id: Vec<(String, usize)> = Vec::new();
    let mut lanes: Vec<Vec<(i64, ClipItem)>> = Vec::new();
    for tr in seq.at("media/video").map(|v| v.all("track").collect::<Vec<_>>()).unwrap_or_default() {
        let mut clips = Vec::new();
        for ci in tr.all("clipitem") {
            if ci.text_at("enabled").is_some_and(|e| e.eq_ignore_ascii_case("false")) {
                continue;
            }
            let file = ci.child("file");
            let fid = file.and_then(|f| f.attr("id")).unwrap_or("").to_string();
            let mi = match by_id.iter().find(|x| !fid.is_empty() && x.0 == fid) {
                Some(x) => x.1,
                None => {
                    let def = defs.iter().find(|d| d.0 == fid).map(|d| d.1).or(file);
                    let m = media_ref(def, ci, rate, tl.media.len());
                    let i = intern(&mut tl.media, m);
                    by_id.push((fid, i));
                    i
                }
            };
            let mrate = tl.media[mi].rate;
            let (Some(src_in), Some(src_out)) = (ci.int_at("in"), ci.int_at("out")) else { continue };
            if src_out <= src_in {
                continue;
            }
            // start = -1 means the edit sits inside a transition; take it from the end instead.
            let rec = match ci.int_at("start") {
                Some(s) if s >= 0 => s,
                _ => ci.int_at("end").map(|e| e - convert_frames(src_out - src_in, mrate, rate)).unwrap_or(0).max(0),
            };
            let name = ci.text_at("name").unwrap_or(&tl.media[mi].name).to_string();
            clips.push((rec, ClipItem { media: mi, name, src_in, src_out, markers: markers(ci, src_in), select_index: None, score: None }));
        }
        lanes.push(clips);
    }
    let video: Vec<Track> = lanes.into_iter().enumerate().map(|(k, c)| Track { name: format!("V{}", k + 1), items: place(&tl, c) }).collect();
    tl.video = non_empty(video);
    Ok(tl)
}
