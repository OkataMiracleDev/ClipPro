//! FCP7 XML (`xmeml` version 5) for Premiere Pro and DaVinci Resolve (SPEC §16.2).

use crate::model::{ClipItem, Item, MediaRef, Timeline};
use crate::xml::{file_url_localhost, Xml};
use clippro_types::FrameRate;

fn rate(x: &mut Xml, r: FrameRate) {
    x.open("rate", &[]).text("timebase", r.timecode_base().to_string()).text("ntsc", if r.is_ntsc() { "TRUE" } else { "FALSE" }).close();
}

fn file_el(x: &mut Xml, m: &MediaRef, first_use: bool) {
    let id = format!("file-{}", m.id);
    if !first_use {
        x.empty("file", &[("id", id)]);
        return;
    }
    x.open("file", &[("id", id)]);
    x.text("name", format!("{}{}", m.name, m.path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default()));
    x.text("pathurl", file_url_localhost(&m.path));
    rate(x, m.rate);
    x.text("duration", m.duration_frames.to_string());
    x.open("timecode", &[]);
    rate(x, m.rate);
    x.text("string", m.start_tc().to_string());
    x.text("frame", m.start_tc_frames.to_string());
    x.text("displayformat", if m.drop_frame { "DF" } else { "NDF" });
    if let Some(reel) = &m.reel {
        x.open("reel", &[]).text("name", reel).close();
    }
    x.close();
    x.open("media", &[]);
    x.open("video", &[]).open("samplecharacteristics", &[]);
    rate(x, m.rate);
    x.text("width", m.width.to_string()).text("height", m.height.to_string());
    x.close().close();
    if m.audio_channels > 0 {
        x.open("audio", &[]).open("samplecharacteristics", &[]).text("depth", "16").text("samplerate", "48000").close();
        x.text("channelcount", m.audio_channels.to_string()).close();
    }
    x.close();
    x.close();
}

fn markers(x: &mut Xml, c: &ClipItem) {
    for mk in &c.markers {
        x.open("marker", &[]);
        x.text("name", &mk.name).text("comment", &mk.comment);
        // Marker positions are in source frames of the clip.
        x.text("in", (c.src_in + mk.at).to_string());
        x.text("out", (c.src_in + mk.at + mk.duration).to_string());
        x.text("color", mk.color.name());
        x.close();
    }
}

/// A Slug generator (black filler) titled with the stringout group.
fn slug(x: &mut Xml, t: &Timeline, id: usize, title: &str, len: i64, rec_in: i64, rec_out: i64) {
    x.open("generatoritem", &[("id", format!("clipitem-{id}"))]);
    x.text("name", title);
    x.text("enabled", "TRUE");
    x.text("duration", len.to_string());
    rate(x, t.rate);
    x.text("start", rec_in.to_string()).text("end", rec_out.to_string());
    x.text("in", "0").text("out", len.to_string());
    x.open("effect", &[]);
    x.text("name", "Slug").text("effectid", "Slug").text("effectcategory", "Matte").text("effecttype", "generator").text("mediatype", "video");
    x.close();
    x.open("marker", &[]).text("name", title).text("comment", "").text("in", "0").text("out", "-1").close();
    x.close();
}

/// Renders the timeline as FCP7 XML.
pub fn export(t: &Timeline) -> String {
    let mut x = Xml::new(Some("<!DOCTYPE xmeml>"));
    let mut seen_files: Vec<i64> = Vec::new();
    x.open("xmeml", &[("version", "5".into())]);
    x.open("sequence", &[("id", "sequence-1".into())]);
    x.text("name", &t.name);
    x.text("duration", t.duration().to_string());
    rate(&mut x, t.rate);
    x.open("timecode", &[]);
    rate(&mut x, t.rate);
    x.text("string", clippro_types::Timecode::from_frames(t.start_tc_frames, t.rate, t.drop_frame).to_string());
    x.text("frame", t.start_tc_frames.to_string());
    x.text("displayformat", if t.drop_frame { "DF" } else { "NDF" });
    x.close();
    x.open("media", &[]);
    x.open("video", &[]);
    x.open("format", &[]).open("samplecharacteristics", &[]);
    rate(&mut x, t.rate);
    x.text("width", t.width.to_string()).text("height", t.height.to_string()).text("pixelaspectratio", "square");
    x.close().close();
    let mut clip_ids = 0usize;
    for track in &t.video {
        x.open("track", &[]);
        for (it, (rec_in, rec_out)) in track.items.iter().zip(t.positions(track)) {
            if let Item::Slug { len, title } = it {
                clip_ids += 1;
                slug(&mut x, t, clip_ids, title, *len, rec_in, rec_out);
                continue;
            }
            let Item::Clip(c) = it else { continue };
            clip_ids += 1;
            let m = &t.media[c.media];
            x.open("clipitem", &[("id", format!("clipitem-{clip_ids}"))]);
            x.text("name", &c.name);
            x.text("enabled", "TRUE");
            x.text("duration", m.duration_frames.to_string());
            rate(&mut x, m.rate);
            x.text("start", rec_in.to_string()).text("end", rec_out.to_string());
            x.text("in", c.src_in.to_string()).text("out", c.src_out.to_string());
            let first = !seen_files.contains(&m.id);
            if first {
                seen_files.push(m.id);
            }
            file_el(&mut x, m, first);
            markers(&mut x, c);
            x.close();
        }
        x.close();
    }
    x.close(); // video
    if t.with_audio {
        x.open("audio", &[]);
        let channels = t.media.iter().map(|m| m.audio_channels).max().unwrap_or(0).min(8);
        if let Some(track) = t.video.first() {
            for ch in 1..=channels {
                x.open("track", &[]);
                for (it, (rec_in, rec_out)) in track.items.iter().zip(t.positions(track)) {
                    let Item::Clip(c) = it else { continue };
                    let m = &t.media[c.media];
                    if m.audio_channels < ch {
                        continue;
                    }
                    clip_ids += 1;
                    x.open("clipitem", &[("id", format!("clipitem-{clip_ids}"))]);
                    x.text("name", &c.name);
                    x.text("enabled", "TRUE");
                    x.text("duration", m.duration_frames.to_string());
                    rate(&mut x, m.rate);
                    x.text("start", rec_in.to_string()).text("end", rec_out.to_string());
                    x.text("in", c.src_in.to_string()).text("out", c.src_out.to_string());
                    x.empty("file", &[("id", format!("file-{}", m.id))]);
                    x.open("sourcetrack", &[]).text("mediatype", "audio").text("trackindex", ch.to_string()).close();
                    x.close();
                }
                x.close();
            }
        }
        x.close();
    }
    x.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{media, selects};
    use crate::model::{build, ExportOptions};

    #[test]
    fn structure() {
        let t = build(media(2), &selects(), &ExportOptions::default());
        let x = export(&t);
        assert!(x.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE xmeml>"));
        assert_eq!(x.matches("<clipitem ").count(), 3 + 3 * 2, "3 video + 3 × 2 audio channels");
        // Each file is described once, then referenced.
        assert_eq!(x.matches("<pathurl>").count(), 2);
        assert!(x.contains("<pathurl>file://localhost/D:/Footage/Day%201/A001C003.mov</pathurl>"));
        assert!(x.contains("<string>10:00:00:00</string>"));
        assert!(x.contains("<start>0</start>") && x.contains("<end>113</end>"));
        assert!(x.contains("<in>6</in>") && x.contains("<out>119</out>"));
        assert!(x.contains("<name>Select 1 · 82</name>"));
        crate::validate::well_formed(&x).unwrap();
    }

    #[test]
    fn ntsc_rate_flags() {
        let mut m = media(1);
        m[0].rate = FrameRate::new(30000, 1001);
        m[0].drop_frame = true;
        let t = build(m, &selects()[..2], &ExportOptions::default());
        let x = export(&t);
        assert!(x.contains("<timebase>30</timebase>") && x.contains("<ntsc>TRUE</ntsc>"));
        assert!(x.contains("<displayformat>DF</displayformat>"));
        assert!(x.contains("01:00:00;00"));
    }
}
