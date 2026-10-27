//! FCPXML 1.10 for Final Cut Pro and DaVinci Resolve (SPEC §16.2).

use crate::model::{Item, MediaRef, Timeline};
use crate::xml::{file_url, Xml};
use clippro_types::{FrameRate, Rational};

/// Rational time string: `frames` at `rate` → "N/Ds" (or "Ns").
pub fn t(frames: i64, rate: FrameRate) -> String {
    let r = Rational::new(frames * rate.0.den, rate.0.num);
    if r.den == 1 {
        format!("{}s", r.num)
    } else {
        format!("{}/{}s", r.num, r.den)
    }
}

fn frame_duration(rate: FrameRate) -> String {
    t(1, rate)
}

fn format_id(formats: &[(FrameRate, u32, u32)], m: &MediaRef) -> String {
    let i = formats.iter().position(|f| *f == (m.rate, m.width, m.height)).unwrap_or(0);
    format!("r{}", i + 1)
}

pub fn export(tl: &Timeline) -> String {
    let mut x = Xml::new(Some("<!DOCTYPE fcpxml>"));
    x.open("fcpxml", &[("version", "1.10".into())]);
    x.open("resources", &[]);
    // Formats: the sequence format first, then one per distinct media format.
    let mut formats: Vec<(FrameRate, u32, u32)> = vec![(tl.rate, tl.width, tl.height)];
    for m in &tl.media {
        if !formats.contains(&(m.rate, m.width, m.height)) {
            formats.push((m.rate, m.width, m.height));
        }
    }
    for (i, (r, w, h)) in formats.iter().enumerate() {
        x.empty(
            "format",
            &[("id", format!("r{}", i + 1)), ("frameDuration", frame_duration(*r)), ("width", w.to_string()), ("height", h.to_string())],
        );
    }
    let asset_id = |i: usize| format!("a{}", i + 1);
    for (i, m) in tl.media.iter().enumerate() {
        let mut attrs = vec![
            ("id", asset_id(i)),
            ("name", m.name.clone()),
            ("start", t(m.start_tc_frames, m.rate)),
            ("duration", t(m.duration_frames, m.rate)),
            ("hasVideo", "1".into()),
            ("format", format_id(&formats, m)),
        ];
        if m.audio_channels > 0 {
            attrs.push(("hasAudio", "1".into()));
            attrs.push(("audioSources", "1".into()));
            attrs.push(("audioChannels", m.audio_channels.to_string()));
            attrs.push(("audioRate", "48000".into()));
        }
        x.open("asset", &attrs);
        x.empty("media-rep", &[("kind", "original-media".into()), ("src", file_url(&m.path))]);
        x.close();
    }
    x.close(); // resources
    x.open("library", &[]);
    x.open("event", &[("name", "ClipPro".into())]);
    x.open("project", &[("name", tl.name.clone())]);
    let tc_format = if tl.drop_frame { "DF" } else { "NDF" };
    x.open(
        "sequence",
        &[
            ("format", "r1".into()),
            ("duration", t(tl.duration(), tl.rate)),
            ("tcStart", t(tl.start_tc_frames, tl.rate)),
            ("tcFormat", tc_format.into()),
            ("audioLayout", "stereo".into()),
            ("audioRate", "48k".into()),
        ],
    );
    x.open("spine", &[]);
    let Some(primary) = tl.video.first() else {
        return x.finish();
    };
    let pos = tl.positions(primary);
    // Connected clips from other tracks, grouped by the spine element they start in.
    let mut connected: Vec<Vec<(i64, usize, usize)>> = vec![Vec::new(); primary.items.len()];
    for (lane, track) in tl.video.iter().enumerate().skip(1) {
        for (j, (it, (rin, _))) in track.items.iter().zip(tl.positions(track)).enumerate() {
            if !matches!(it, Item::Clip(_)) {
                continue;
            }
            let k = pos.iter().position(|(a, b)| rin >= *a && rin < *b).unwrap_or(pos.len().saturating_sub(1));
            if let Some(v) = connected.get_mut(k) {
                v.push((rin, lane, j));
            }
        }
    }
    for (k, (it, (rin, _))) in primary.items.iter().zip(pos.iter().copied()).enumerate() {
        let offset = t(tl.start_tc_frames + rin, tl.rate);
        match it {
            Item::Gap(n) => {
                x.open("gap", &[("name", "Gap".into()), ("offset", offset), ("start", "0s".into()), ("duration", t(*n, tl.rate))]);
                for (crin, lane, j) in &connected[k] {
                    connected_clip(&mut x, tl, *lane, *j, t(crin - rin, tl.rate));
                }
                x.close();
            }
            Item::Clip(c) => {
                let m = &tl.media[c.media];
                let start_f = m.start_tc_frames + c.src_in;
                x.open(
                    "asset-clip",
                    &[
                        ("ref", asset_id(c.media)),
                        ("name", c.name.clone()),
                        ("offset", offset),
                        ("start", t(start_f, m.rate)),
                        ("duration", t(c.len(), m.rate)),
                        ("format", format_id(&formats, m)),
                        ("tcFormat", if m.drop_frame { "DF".into() } else { "NDF".into() }),
                    ],
                );
                for mk in &c.markers {
                    x.empty(
                        "marker",
                        &[
                            ("start", t(start_f + mk.at, m.rate)),
                            ("duration", t(mk.duration, m.rate)),
                            ("value", mk.name.clone()),
                            ("note", mk.comment.clone()),
                        ],
                    );
                }
                for (crin, lane, j) in &connected[k] {
                    // Local time of the parent asset-clip at the connected clip's record position.
                    let local = start_f + crate::model::convert_frames(crin - rin, tl.rate, m.rate);
                    connected_clip(&mut x, tl, *lane, *j, t(local, m.rate));
                }
                x.close();
            }
        }
    }
    x.finish()
}

fn connected_clip(x: &mut Xml, tl: &Timeline, lane: usize, j: usize, offset: String) {
    let Some(Item::Clip(c)) = tl.video.get(lane).and_then(|tr| tr.items.get(j)) else { return };
    let m = &tl.media[c.media];
    let fid = (0..tl.media.len()).find(|&i| tl.media[i].id == m.id).unwrap_or(0);
    x.empty(
        "asset-clip",
        &[
            ("ref", format!("a{}", fid + 1)),
            ("lane", lane.to_string()),
            ("name", c.name.clone()),
            ("offset", offset),
            ("start", t(m.start_tc_frames + c.src_in, m.rate)),
            ("duration", t(c.len(), m.rate)),
        ],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{media, selects};
    use crate::model::{build, ExportOptions, Layout};

    #[test]
    fn rational_times() {
        assert_eq!(t(50, FrameRate::new(25, 1)), "2s");
        assert_eq!(t(13, FrameRate::new(25, 1)), "13/25s");
        assert_eq!(t(1, FrameRate::new(30000, 1001)), "1001/30000s");
        assert_eq!(t(30, FrameRate::new(30000, 1001)), "1001/1000s");
    }

    #[test]
    fn structure() {
        let tl = build(media(2), &selects(), &ExportOptions::default());
        let x = export(&tl);
        crate::validate::well_formed(&x).unwrap();
        assert!(x.contains("<fcpxml version=\"1.10\">"));
        assert_eq!(x.matches("<asset ").count(), 2);
        assert_eq!(x.matches("<asset-clip ").count(), 3);
        assert!(x.contains("tcStart=\"3600s\""));
        // First clip: offset 3600s, start 10:00:00:06 = 900006 frames at 25 fps.
        assert!(x.contains("offset=\"3600s\""), "{x}");
        assert!(x.contains("start=\"900006/25s\""), "{x}");
        assert!(x.contains("src=\"file:///D:/Footage/Day%201/A001C003.mov\""));
        assert!(x.contains("<marker start="));
    }

    #[test]
    fn connected_rejects() {
        let tl = build(media(2), &selects(), &ExportOptions { layout: Layout::SelectsOverRejects, ..ExportOptions::default() });
        let x = export(&tl);
        crate::validate::well_formed(&x).unwrap();
        assert!(x.matches("lane=\"1\"").count() >= 3, "{x}");
    }
}
