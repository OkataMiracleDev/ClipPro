//! CMX3600 EDL reader. EDLs have no frame rate, media start timecode or frame size: the rate is
//! given, each source starts at its earliest used timecode until [`super::relink`] rebases it.

use super::{place, stem};
use crate::model::{ClipItem, Marker, MarkerColor, MediaRef, Timeline, Track};
use clippro_types::{FrameRate, Timecode};
use std::path::PathBuf;

fn tc_shaped(s: &str) -> bool {
    let p: Vec<&str> = s.split([':', ';', '.']).collect();
    p.len() == 4 && p.iter().all(|x| !x.is_empty() && x.len() <= 3 && x.bytes().all(|b| b.is_ascii_digit()))
}

/// `001  REEL  AA/V  C  [dur]  src_in src_out rec_in rec_out`.
pub fn is_event_line(l: &str) -> bool {
    let t: Vec<&str> = l.split_whitespace().collect();
    t.len() >= 8 && t[0].len() >= 3 && t[0].bytes().all(|b| b.is_ascii_digit()) && t[t.len() - 4..].iter().all(|x| tc_shaped(x))
}

struct Event {
    reel: String,
    video: bool,
    audio: bool,
    tc: [i64; 4],
    clip: Option<String>,
    file: Option<String>,
    locs: Vec<(i64, MarkerColor, String)>,
}

pub fn import(text: &str, rate: FrameRate) -> Result<Timeline, String> {
    let base = rate.timecode_base();
    let mut name = String::from("Imported EDL");
    let mut fcm_drop = false;
    let tc = |s: &str, drop: bool| -> Option<i64> {
        // "FCM: DROP FRAME" with colon labels still counts drop-frame.
        let s = if drop && !s.contains(';') { format!("{};{}", &s[..s.len() - 3], &s[s.len() - 2..]) } else { s.to_string() };
        Timecode::parse(&s, base).ok().map(|t| t.frames)
    };
    let mut events: Vec<Event> = Vec::new();
    for line in text.lines() {
        let l = line.trim_end();
        if let Some(t) = l.strip_prefix("TITLE:") {
            name = t.trim().to_string();
        } else if let Some(f) = l.strip_prefix("FCM:") {
            fcm_drop = f.trim().eq_ignore_ascii_case("DROP FRAME") && rate.supports_drop_frame();
        } else if is_event_line(l) {
            let t: Vec<&str> = l.split_whitespace().collect();
            let n = t.len();
            let mut v = [0i64; 4];
            for (k, s) in t[n - 4..].iter().enumerate() {
                v[k] = tc(s, fcm_drop).ok_or_else(|| format!("bad timecode {s:?} in: {l}"))?;
            }
            let track = t[2].to_ascii_uppercase();
            events.push(Event {
                reel: t[1].to_string(),
                video: track.contains('V') || track == "B",
                audio: track.contains('A') || track == "B",
                tc: v,
                clip: None,
                file: None,
                locs: Vec::new(),
            });
        } else if let (Some(ev), Some(c)) = (events.last_mut(), l.strip_prefix('*')) {
            let c = c.trim();
            if let Some(n) = c.strip_prefix("FROM CLIP NAME:") {
                ev.clip = Some(n.trim().to_string());
            } else if let Some(f) = c.strip_prefix("SOURCE FILE:") {
                ev.file = Some(f.trim().to_string());
            } else if let Some(loc) = c.strip_prefix("LOC:") {
                let mut it = loc.split_whitespace();
                let (Some(at), Some(col)) = (it.next(), it.next()) else { continue };
                let Some(at) = tc(at, fcm_drop) else { continue };
                let rest: Vec<&str> = it.collect();
                ev.locs.push((at, MarkerColor::parse(col).unwrap_or(MarkerColor::Red), rest.join(" ")));
            }
        }
    }
    let events: Vec<Event> = events.into_iter().filter(|e| e.video && !e.reel.eq_ignore_ascii_case("BL") && e.tc[1] > e.tc[0]).collect();
    // Record start: the hour the first event falls in (conventionally 01:00:00:00).
    let first = events.iter().map(|e| e.tc[2]).min().unwrap_or(0);
    let hh = Timecode::from_frames(first, rate, fcm_drop).components().0.max(0);
    let start_tc_frames = Timecode::from_components(hh, 0, 0, 0, base, fcm_drop).map(|t| t.frames).unwrap_or(0);
    let mut tl = Timeline { name, rate, drop_frame: fcm_drop, start_tc_frames, width: 1920, height: 1080, media: Vec::new(), video: Vec::new(), with_audio: events.iter().any(|e| e.audio) };
    // Media by source file (else reel), starting at the earliest timecode any event uses.
    let mut idx = Vec::with_capacity(events.len());
    for e in &events {
        let path = e.file.as_ref().map(PathBuf::from).unwrap_or_default();
        let known = tl.media.iter().position(|m| if e.file.is_some() { m.path == path } else { m.path.as_os_str().is_empty() && m.reel.as_deref() == Some(&e.reel) });
        if let Some(i) = known {
            let mm = &mut tl.media[i];
            mm.start_tc_frames = mm.start_tc_frames.min(e.tc[0]);
            idx.push(i);
            continue;
        }
        let mname = e.file.as_deref().map(stem).or_else(|| e.clip.clone()).unwrap_or_else(|| e.reel.clone());
        let m = MediaRef {
            id: tl.media.len() as i64 + 1,
            name: mname,
            path,
            rate,
            duration_frames: 0,
            start_tc_frames: i64::MAX,
            drop_frame: fcm_drop,
            width: 1920,
            height: 1080,
            audio_channels: if e.audio { 2 } else { 0 },
            reel: Some(e.reel.clone()),
        };
        tl.media.push(MediaRef { start_tc_frames: e.tc[0], ..m });
        idx.push(tl.media.len() - 1);
    }
    let mut clips = Vec::new();
    for (e, &mi) in events.iter().zip(&idx) {
        let ms = tl.media[mi].start_tc_frames;
        let (src_in, src_out) = (e.tc[0] - ms, e.tc[1] - ms);
        let m = &mut tl.media[mi];
        m.duration_frames = m.duration_frames.max(src_out);
        let markers = e
            .locs
            .iter()
            .map(|(at, color, n)| Marker { at: (at - e.tc[2]).max(0), duration: 1, name: n.clone(), comment: String::new(), color: *color })
            .collect();
        let name = e.clip.clone().unwrap_or_else(|| m.name.clone());
        clips.push((e.tc[2] - start_tc_frames, ClipItem { media: mi, name, src_in, src_out, markers, select_index: None, score: None }));
    }
    let items = place(&tl, clips);
    tl.video = if items.is_empty() { Vec::new() } else { vec![Track { name: "V1".into(), items }] };
    Ok(tl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Item;

    #[test]
    fn hand_written_edl() {
        let e = "TITLE: Cut 3\nFCM: NON-DROP FRAME\n\n\
                 001  A001     V     C        10:00:01:00 10:00:03:00 01:00:00:00 01:00:02:00\n\
                 * FROM CLIP NAME: Interview\n\
                 * LOC: 01:00:00:12 YELLOW  laugh here\n\
                 002  BL       V     C        00:00:00:00 00:00:01:00 01:00:02:00 01:00:03:00\n\
                 003  A001     AA    C        10:00:05:00 10:00:06:00 01:00:03:00 01:00:04:00\n\
                 004  A001     V     C        10:00:05:00 10:00:06:00 01:00:03:00 01:00:04:00\n";
        let t = import(e, FrameRate::new(25, 1)).unwrap();
        assert_eq!(t.name, "Cut 3");
        assert_eq!(t.start_tc_frames, 90_000);
        assert_eq!(t.media.len(), 1, "one reel");
        assert_eq!(t.media[0].start_tc_frames, 900_025);
        let items = &t.video[0].items;
        assert_eq!(items.len(), 3, "clip, black as gap, clip");
        let Item::Clip(c) = &items[0] else { panic!() };
        assert_eq!((c.src_in, c.src_out, c.name.as_str()), (0, 50, "Interview"));
        assert_eq!((c.markers[0].at, c.markers[0].color, c.markers[0].name.as_str()), (12, MarkerColor::Yellow, "laugh here"));
        assert_eq!(items[1], Item::Gap(25));
        let Item::Clip(c) = &items[2] else { panic!() };
        assert_eq!((c.src_in, c.src_out), (100, 125));
    }

    #[test]
    fn drop_frame_with_colon_labels() {
        let e = "FCM: DROP FRAME\n001  A  V  C  00:01:00:02 00:01:01:02 01:00:00:00 01:00:01:00\n";
        let t = import(e, FrameRate::new(30000, 1001)).unwrap();
        assert!(t.drop_frame);
        assert_eq!(t.media[0].start_tc_frames, 1800, "00:01:00;02 is frame 1800 in drop-frame");
        assert!(import("FCM: DROP FRAME\n001  A  V  C  00:01:00:00 00:01:01:02 01:00:00:00 01:00:01:00\n", FrameRate::new(30000, 1001)).is_err());
    }
}
