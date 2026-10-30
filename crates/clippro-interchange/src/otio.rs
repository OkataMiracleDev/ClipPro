//! OpenTimelineIO JSON (`.otio`, Timeline.1) for Resolve 18.5+ and OTIO tools (SPEC §16.2).

use crate::model::{Item, MarkerColor, Timeline};
use crate::xml::file_url;
use clippro_types::FrameRate;
use serde_json::{json, Value};

fn rt(value: i64, rate: FrameRate) -> Value {
    json!({ "OTIO_SCHEMA": "RationalTime.1", "rate": rate.fps(), "value": value as f64 })
}

fn range(start: i64, dur: i64, rate: FrameRate) -> Value {
    json!({ "OTIO_SCHEMA": "TimeRange.1", "start_time": rt(start, rate), "duration": rt(dur, rate) })
}

fn color(c: MarkerColor) -> &'static str {
    match c {
        MarkerColor::Red => "RED",
        MarkerColor::Orange => "ORANGE",
        MarkerColor::Yellow => "YELLOW",
        MarkerColor::Green => "GREEN",
        MarkerColor::Cyan => "CYAN",
        MarkerColor::Blue => "BLUE",
        MarkerColor::Purple => "PURPLE",
        MarkerColor::Magenta => "MAGENTA",
        MarkerColor::White => "WHITE",
    }
}

pub fn export(tl: &Timeline) -> String {
    let mut tracks = Vec::new();
    for track in &tl.video {
        let children: Vec<Value> = track
            .items
            .iter()
            .map(|it| match it {
                Item::Gap(n) => json!({ "OTIO_SCHEMA": "Gap.1", "name": "", "source_range": range(0, *n, tl.rate), "effects": [], "markers": [], "metadata": {} }),
                Item::Slug { len, title } => json!({ "OTIO_SCHEMA": "Gap.1", "name": title, "source_range": range(0, *len, tl.rate), "effects": [],
                    "markers": [{ "OTIO_SCHEMA": "Marker.2", "name": title, "comment": "", "color": "WHITE", "marked_range": range(0, 1, tl.rate), "metadata": {} }],
                    "metadata": { "clippro": { "slug": true } } }),
                Item::Clip(c) => {
                    let m = &tl.media[c.media];
                    let start = m.start_tc_frames + c.src_in;
                    let markers: Vec<Value> = c
                        .markers
                        .iter()
                        .map(|mk| {
                            json!({ "OTIO_SCHEMA": "Marker.2", "name": mk.name, "comment": mk.comment, "color": color(mk.color),
                                    "marked_range": range(start + mk.at, mk.duration, m.rate), "metadata": {} })
                        })
                        .collect();
                    json!({
                        "OTIO_SCHEMA": "Clip.2",
                        "name": c.name,
                        "source_range": range(start, c.len(), m.rate),
                        "media_references": { "DEFAULT_MEDIA": {
                            "OTIO_SCHEMA": "ExternalReference.1",
                            "name": m.name,
                            "target_url": file_url(&m.path),
                            "available_range": range(m.start_tc_frames, m.duration_frames, m.rate),
                            "metadata": { "clippro": { "id": m.id, "width": m.width, "height": m.height, "audio_channels": m.audio_channels,
                                                       "reel": m.reel, "drop_frame": m.drop_frame } }
                        }},
                        "active_media_reference_key": "DEFAULT_MEDIA",
                        "effects": [],
                        "markers": markers,
                        "enabled": true,
                        "metadata": { "clippro": { "score": c.score, "select": c.select_index } }
                    })
                }
            })
            .collect();
        tracks.push(json!({ "OTIO_SCHEMA": "Track.1", "name": track.name, "kind": "Video", "children": children,
                            "source_range": null, "effects": [], "markers": [], "enabled": true, "metadata": {} }));
    }
    let doc = json!({
        "OTIO_SCHEMA": "Timeline.1",
        "name": tl.name,
        "global_start_time": rt(tl.start_tc_frames, tl.rate),
        "tracks": { "OTIO_SCHEMA": "Stack.1", "name": "tracks", "children": tracks, "source_range": null,
                    "effects": [], "markers": [], "enabled": true, "metadata": {} },
        "metadata": { "clippro": { "version": env!("CARGO_PKG_VERSION"), "width": tl.width, "height": tl.height, "drop_frame": tl.drop_frame } }
    });
    serde_json::to_string_pretty(&doc).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{media, selects};
    use crate::model::{build, ExportOptions};

    #[test]
    fn structure() {
        let tl = build(media(2), &selects(), &ExportOptions { gap_s: 1.0, ..ExportOptions::default() });
        let s = export(&tl);
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["OTIO_SCHEMA"], "Timeline.1");
        let kids = v["tracks"]["children"][0]["children"].as_array().unwrap();
        assert_eq!(kids.len(), 5);
        assert_eq!(kids[0]["OTIO_SCHEMA"], "Clip.2");
        assert_eq!(kids[1]["OTIO_SCHEMA"], "Gap.1");
        assert_eq!(kids[0]["source_range"]["start_time"]["value"], 900_006.0);
        assert_eq!(kids[0]["source_range"]["duration"]["value"], 113.0);
        assert_eq!(kids[0]["markers"][0]["color"], "GREEN");
        assert_eq!(kids[0]["media_references"]["DEFAULT_MEDIA"]["target_url"], "file:///D:/Footage/Day%201/A001C003.mov");
    }
}
