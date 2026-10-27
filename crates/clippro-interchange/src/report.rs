//! CSV and JSON reports (SPEC EXP-8). The HTML report and PDF contact sheet build on these.

use crate::model::{Item, Timeline};
use clippro_types::Timecode;
use serde_json::json;

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// One row per select in timeline order.
pub fn export_csv(tl: &Timeline) -> String {
    let mut out = String::from("index,clip,path,source_in_tc,source_out_tc,record_in_tc,duration_s,score,marker,comment\n");
    let Some(track) = tl.video.first() else { return out };
    for (it, (rin, _)) in track.items.iter().zip(tl.positions(track)) {
        let Item::Clip(c) = it else { continue };
        let m = &tl.media[c.media];
        let tci = Timecode::from_frames(m.start_tc_frames + c.src_in, m.rate, m.drop_frame);
        let tco = Timecode::from_frames(m.start_tc_frames + c.src_out, m.rate, m.drop_frame);
        let rec = Timecode::from_frames(tl.start_tc_frames + rin, tl.rate, tl.drop_frame);
        let mk = c.markers.first();
        out.push_str(&format!(
            "{},{},{},{},{},{},{:.3},{},{},{}\n",
            c.select_index.map(|i| i + 1).unwrap_or(0),
            csv_field(&c.name),
            csv_field(&m.path.to_string_lossy()),
            tci,
            tco,
            rec,
            c.len() as f64 / m.rate.fps(),
            c.score.map(|s| format!("{:.1}", s * 100.0)).unwrap_or_default(),
            csv_field(mk.map(|k| k.name.as_str()).unwrap_or("")),
            csv_field(mk.map(|k| k.comment.as_str()).unwrap_or("")),
        ));
    }
    out
}

pub fn export_json(tl: &Timeline) -> String {
    let mut selects = Vec::new();
    if let Some(track) = tl.video.first() {
        for (it, (rin, rout)) in track.items.iter().zip(tl.positions(track)) {
            let Item::Clip(c) = it else { continue };
            let m = &tl.media[c.media];
            selects.push(json!({
                "index": c.select_index.map(|i| i + 1),
                "clip": c.name,
                "path": m.path,
                "source_in_frames": c.src_in,
                "source_out_frames": c.src_out,
                "source_in_tc": Timecode::from_frames(m.start_tc_frames + c.src_in, m.rate, m.drop_frame).to_string(),
                "source_out_tc": Timecode::from_frames(m.start_tc_frames + c.src_out, m.rate, m.drop_frame).to_string(),
                "record_in_frames": rin,
                "record_out_frames": rout,
                "fps": m.rate.fps(),
                "score": c.score,
                "markers": c.markers.iter().map(|k| json!({"name": k.name, "comment": k.comment, "color": k.color.name(), "at_frames": k.at, "duration_frames": k.duration})).collect::<Vec<_>>(),
            }));
        }
    }
    let doc = json!({
        "name": tl.name,
        "timeline_fps": tl.rate.fps(),
        "timeline_frames": tl.duration(),
        "selects": selects,
        "generator": format!("ClipPro {}", env!("CARGO_PKG_VERSION")),
    });
    serde_json::to_string_pretty(&doc).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{media, selects};
    use crate::model::{build, ExportOptions};

    #[test]
    fn csv_rows_and_quoting() {
        let mut m = media(2);
        m[0].name = "Take, \"best\"".into();
        let tl = build(m, &selects(), &ExportOptions::default());
        let c = export_csv(&tl);
        let lines: Vec<&str> = c.lines().collect();
        assert_eq!(lines.len(), 4);
        assert!(lines[1].starts_with("1,\"Take, \"\"best\"\"\",D:/Footage/Day 1/A001C003.mov,10:00:00:06,10:00:04:19,01:00:00:00,4.520,82.0,"));
    }

    #[test]
    fn json_parses() {
        let tl = build(media(2), &selects(), &ExportOptions::default());
        let v: serde_json::Value = serde_json::from_str(&export_json(&tl)).unwrap();
        assert_eq!(v["selects"].as_array().unwrap().len(), 3);
        assert_eq!(v["selects"][2]["source_in_tc"], "10:00:40:00");
    }
}
