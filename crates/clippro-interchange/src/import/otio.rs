//! OpenTimelineIO JSON reader (Timeline.1 with Clip.1/Clip.2, Gap, Transition and nested Stacks).
//! Video tracks only; nested stacks and other items keep their length as gaps.

use super::{intern, non_empty, place, url_to_path};
use crate::model::{convert_frames, ClipItem, Marker, MarkerColor, MediaRef, Timeline, Track};
use clippro_types::FrameRate;
use serde_json::Value;

/// A RationalTime as (value, rate).
fn rt(v: &Value) -> Option<(f64, f64)> {
    Some((v.get("value")?.as_f64()?, v.get("rate")?.as_f64().filter(|r| *r > 0.0)?))
}

/// A RationalTime in frames at `rate`.
fn at_rate(v: &Value, rate: FrameRate) -> Option<i64> {
    let (value, r) = rt(v)?;
    Some((value * rate.fps() / r).round() as i64)
}

/// (start, duration) of a TimeRange in frames at `rate`.
fn range(v: &Value, rate: FrameRate) -> Option<(i64, i64)> {
    Some((at_rate(v.get("start_time")?, rate)?, at_rate(v.get("duration")?, rate)?))
}

fn range_rate(v: &Value) -> Option<FrameRate> {
    rt(v.get("start_time")?).or_else(|| rt(v.get("duration")?)).map(|(_, r)| FrameRate::from_f64(r))
}

fn schema(v: &Value) -> &str {
    v.get("OTIO_SCHEMA").and_then(Value::as_str).unwrap_or("")
}

fn colour(s: &str) -> MarkerColor {
    match s.to_ascii_uppercase().as_str() {
        "PINK" => MarkerColor::Magenta,
        "BLACK" => MarkerColor::White,
        s => MarkerColor::parse(s).unwrap_or(MarkerColor::Red),
    }
}

fn media_reference(clip: &Value) -> Option<&Value> {
    if let Some(refs) = clip.get("media_references") {
        let key = clip.get("active_media_reference_key").and_then(Value::as_str).unwrap_or("DEFAULT_MEDIA");
        return refs.get(key).or_else(|| refs.as_object()?.values().next());
    }
    clip.get("media_reference")
}

fn clip(tl: &mut Timeline, v: &Value) -> Option<ClipItem> {
    let sr = v.get("source_range").filter(|s| !s.is_null());
    let mr = media_reference(v).filter(|m| !m.is_null());
    let avail = mr.and_then(|m| m.get("available_range")).filter(|a| !a.is_null());
    let mrate = avail.and_then(range_rate).or_else(|| sr.and_then(range_rate)).unwrap_or(tl.rate);
    let (start, duration) = avail.and_then(|a| range(a, mrate)).unwrap_or((0, 0));
    let (s_in, len) = sr.and_then(|s| range(s, mrate)).or(Some((start, duration))).filter(|r| r.1 > 0)?;
    let meta = mr.and_then(|m| m.pointer("/metadata/clippro"));
    let num = |k: &str| meta.and_then(|m| m.get(k)).and_then(Value::as_i64);
    let path = mr.and_then(|m| m.get("target_url")).and_then(Value::as_str).map(url_to_path).unwrap_or_default();
    let clip_name = v.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let name = mr
        .and_then(|m| m.get("name")).and_then(Value::as_str).filter(|n| !n.is_empty()).map(str::to_string)
        .or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| clip_name.clone());
    let m = MediaRef {
        id: num("id").unwrap_or(tl.media.len() as i64 + 1),
        name,
        path,
        rate: mrate,
        duration_frames: if duration > 0 { duration } else { s_in - start + len },
        start_tc_frames: start,
        drop_frame: meta.and_then(|m| m.get("drop_frame")).and_then(Value::as_bool).unwrap_or(false),
        width: num("width").unwrap_or(0) as u32,
        height: num("height").unwrap_or(0) as u32,
        audio_channels: num("audio_channels").unwrap_or(2) as u32,
        reel: meta.and_then(|m| m.get("reel")).and_then(Value::as_str).map(str::to_string),
    };
    let media = intern(&mut tl.media, m);
    let mstart = tl.media[media].start_tc_frames;
    let markers = v
        .get("markers")
        .and_then(Value::as_array)
        .map(|ms| {
            ms.iter()
                .filter_map(|mk| {
                    let (a, d) = range(mk.get("marked_range")?, mrate)?;
                    Some(Marker {
                        at: (a - s_in).max(0),
                        duration: d,
                        name: mk.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                        comment: mk.get("comment").and_then(Value::as_str).unwrap_or("").to_string(),
                        color: colour(mk.get("color").and_then(Value::as_str).unwrap_or("RED")),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let cp = v.pointer("/metadata/clippro");
    Some(ClipItem {
        media,
        name: if clip_name.is_empty() { tl.media[media].name.clone() } else { clip_name },
        src_in: s_in - mstart,
        src_out: s_in - mstart + len,
        markers,
        select_index: cp.and_then(|c| c.get("select")).and_then(Value::as_u64).map(|s| s as usize),
        score: cp.and_then(|c| c.get("score")).and_then(Value::as_f64).map(|s| s as f32),
    })
}

pub fn import(json: &str) -> Result<Timeline, String> {
    let doc: Value = serde_json::from_str(json).map_err(|e| format!("OTIO: {e}"))?;
    if schema(&doc) != "Timeline.1" {
        return Err("not an OTIO timeline".into());
    }
    let tracks: Vec<&Value> = doc.pointer("/tracks/children").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    let first_rate = || {
        tracks.iter().flat_map(|t| t.get("children").and_then(Value::as_array).into_iter().flatten()).find_map(|c| c.get("source_range").and_then(range_rate))
    };
    let gst = doc.get("global_start_time").and_then(rt);
    let rate = gst.map(|g| FrameRate::from_f64(g.1)).or_else(first_rate).unwrap_or_default();
    let meta = doc.pointer("/metadata/clippro");
    let dim = |k: &str| meta.and_then(|m| m.get(k)).and_then(Value::as_u64).map(|v| v as u32);
    let mut tl = Timeline {
        name: doc.get("name").and_then(Value::as_str).unwrap_or("Imported timeline").to_string(),
        rate,
        drop_frame: meta.and_then(|m| m.get("drop_frame")).and_then(Value::as_bool).unwrap_or(false) && rate.supports_drop_frame(),
        start_tc_frames: gst.map(|(v, r)| (v * rate.fps() / r).round() as i64).unwrap_or(0),
        width: dim("width").unwrap_or(1920),
        height: dim("height").unwrap_or(1080),
        media: Vec::new(),
        video: Vec::new(),
        with_audio: true,
    };
    let mut lanes: Vec<Vec<(i64, ClipItem)>> = Vec::new();
    for t in tracks.iter().filter(|t| schema(t).starts_with("Track") && t.get("kind").and_then(Value::as_str).unwrap_or("Video") == "Video") {
        let mut at = 0i64;
        let mut clips = Vec::new();
        for c in t.get("children").and_then(Value::as_array).into_iter().flatten() {
            let s = schema(c);
            if s.starts_with("Transition") {
                continue; // overlaps its neighbours; takes no track time
            }
            if s.starts_with("Clip") {
                let enabled = c.get("enabled").and_then(Value::as_bool).unwrap_or(true);
                if let Some(ci) = clip(&mut tl, c) {
                    let len = convert_frames(ci.len(), tl.media[ci.media].rate, tl.rate);
                    if enabled {
                        clips.push((at, ci));
                    }
                    at += len;
                    continue;
                }
            }
            at += c.get("source_range").and_then(|r| range(r, tl.rate)).map(|r| r.1).unwrap_or(0);
        }
        lanes.push(clips);
    }
    let video: Vec<Track> = lanes.into_iter().enumerate().map(|(k, c)| Track { name: format!("V{}", k + 1), items: place(&tl, c) }).collect();
    tl.video = non_empty(video);
    Ok(tl)
}
