//! Importers for NLE timelines (SPEC P11.1): FCP7 XML, FCPXML, OTIO and CMX3600 EDL, read back
//! into the neutral [`Timeline`], plus relinking to project media.
//!
//! Imports are lossy where the format is: EDLs carry no media start timecode or rate (they are
//! rebased on relink), FCPXML markers have no colour, OTIO has no frame size unless ClipPro wrote it.

pub mod dom;
pub mod edl;
pub mod fcp7;
pub mod fcpxml;
pub mod otio;

use crate::model::{convert_frames, ClipItem, Item, MediaRef, Timeline, Track};
use std::path::{Path, PathBuf};

/// A file URL (`file://localhost/D:/a%20b.mov`, `file:///Users/x.mov`) or plain path → path.
pub fn url_to_path(url: &str) -> PathBuf {
    let s = url.trim();
    let s = s.strip_prefix("file://").map(|r| r.strip_prefix("localhost").unwrap_or(r)).unwrap_or(s);
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    let p = String::from_utf8_lossy(&out).into_owned();
    // "/D:/x" is a Windows drive path.
    let b = p.as_bytes();
    if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
        return PathBuf::from(&p[1..]);
    }
    PathBuf::from(p)
}

/// File stem of a path or name ("A001C003.mov" → "A001C003").
pub fn stem(s: &str) -> String {
    Path::new(s).file_stem().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| s.to_string())
}

/// Lays clips out on a track from their record positions (timeline frames from the start),
/// filling the space between them with gaps. Overlaps (transitions) are butted.
pub fn place(tl: &Timeline, mut clips: Vec<(i64, ClipItem)>) -> Vec<Item> {
    clips.sort_by_key(|c| c.0);
    let mut items = Vec::new();
    let mut at = 0i64;
    for (rin, c) in clips {
        if rin > at {
            items.push(Item::Gap(rin - at));
            at = rin;
        }
        let len = convert_frames(c.len(), tl.media[c.media].rate, tl.rate);
        items.push(Item::Clip(c));
        at += len;
    }
    items
}

/// Index of a media entry by path, adding it if new.
pub fn intern(media: &mut Vec<MediaRef>, m: MediaRef) -> usize {
    let key = norm_path(&m.path);
    if let Some(i) = media.iter().position(|x| !key.is_empty() && norm_path(&x.path) == key || key.is_empty() && x.name == m.name) {
        return i;
    }
    media.push(m);
    media.len() - 1
}

fn norm_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// How a timeline media entry was matched to a project clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkBy {
    Path,
    FileName,
    Name,
    ReelAndTimecode,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RelinkReport {
    /// (timeline media index, pool index, how).
    pub linked: Vec<(usize, usize, LinkBy)>,
    /// Timeline media indexes nothing matched.
    pub missing: Vec<usize>,
}

/// Source range (absolute timecode frames at the media rate) the timeline uses of media `i`.
fn used_range(tl: &Timeline, i: usize) -> Option<(i64, i64)> {
    let m = &tl.media[i];
    let mut r: Option<(i64, i64)> = None;
    for t in &tl.video {
        for it in &t.items {
            if let Item::Clip(c) = it {
                if c.media == i {
                    let (a, b) = (m.start_tc_frames + c.src_in, m.start_tc_frames + c.src_out);
                    r = Some(r.map_or((a, b), |(x, y)| (x.min(a), y.max(b))));
                }
            }
        }
    }
    r
}

/// Matches the timeline's media to `pool` (project media) by path, then file name, then clip
/// name, then reel with a covering timecode range; matched entries take the pool's metadata and
/// clip ranges are rebased onto the pool's start timecode and rate.
pub fn relink(tl: &mut Timeline, pool: &[MediaRef]) -> RelinkReport {
    let mut rep = RelinkReport::default();
    for i in 0..tl.media.len() {
        let m = tl.media[i].clone();
        let file = m.path.file_name().map(|f| f.to_string_lossy().to_lowercase());
        let by_path = pool.iter().position(|p| !m.path.as_os_str().is_empty() && norm_path(&p.path) == norm_path(&m.path));
        let by_file = || file.as_ref().and_then(|f| pool.iter().position(|p| p.path.file_name().map(|x| x.to_string_lossy().to_lowercase()).as_ref() == Some(f)));
        let by_name = || {
            let n = stem(&m.name).to_lowercase();
            pool.iter().position(|p| !n.is_empty() && (stem(&p.name).to_lowercase() == n || p.path.file_stem().map(|s| s.to_string_lossy().to_lowercase()) == Some(n.clone())))
        };
        let by_reel = || {
            let reel = m.reel.as_ref().or(Some(&m.name)).filter(|r| !r.is_empty())?.to_uppercase();
            let (a, b) = used_range(tl, i)?;
            pool.iter().position(|p| {
                let pr = crate::edl::reel_name(p, 32);
                let (pa, pb) = (convert_frames(a, m.rate, p.rate), convert_frames(b, m.rate, p.rate));
                (pr == reel || pr.starts_with(&reel) || reel.starts_with(&pr) && pr.len() >= 8)
                    && pa >= p.start_tc_frames
                    && pb <= p.start_tc_frames + p.duration_frames
            })
        };
        let found = by_path
            .map(|j| (j, LinkBy::Path))
            .or_else(|| by_file().map(|j| (j, LinkBy::FileName)))
            .or_else(|| by_name().map(|j| (j, LinkBy::Name)))
            .or_else(|| by_reel().map(|j| (j, LinkBy::ReelAndTimecode)));
        let Some((j, how)) = found else {
            rep.missing.push(i);
            continue;
        };
        rebase(tl, i, &pool[j]);
        rep.linked.push((i, j, how));
    }
    rep
}

/// Replaces media `i` with `to`, keeping every clip on the same absolute source timecode.
fn rebase(tl: &mut Timeline, i: usize, to: &MediaRef) {
    let from = tl.media[i].clone();
    for t in &mut tl.video {
        for it in &mut t.items {
            let Item::Clip(c) = it else { continue };
            if c.media != i {
                continue;
            }
            let abs_in = convert_frames(from.start_tc_frames + c.src_in, from.rate, to.rate);
            let len = convert_frames(c.len(), from.rate, to.rate).max(1);
            c.src_in = abs_in - to.start_tc_frames;
            c.src_out = c.src_in + len;
            for mk in &mut c.markers {
                mk.at = convert_frames(mk.at, from.rate, to.rate);
                mk.duration = convert_frames(mk.duration, from.rate, to.rate);
            }
        }
    }
    tl.media[i] = to.clone();
}

/// One clip as compared by [`edit_signature`]: (track, media path, src in, src out, record in).
pub type ClipSig = (usize, String, i64, i64, i64);

/// The edit a timeline describes, independent of format details: each clip's media, source
/// range and record position, in order.
pub fn edit_signature(tl: &Timeline) -> Vec<ClipSig> {
    let mut v = Vec::new();
    for (k, t) in tl.video.iter().enumerate() {
        for (it, (rin, _)) in t.items.iter().zip(tl.positions(t)) {
            if let Item::Clip(c) = it {
                v.push((k, norm_path(&tl.media[c.media].path), c.src_in, c.src_out, rin));
            }
        }
    }
    v
}

/// Empty tracks are dropped so layouts compare by content.
pub fn non_empty(tracks: Vec<Track>) -> Vec<Track> {
    tracks.into_iter().filter(|t| t.items.iter().any(|i| matches!(i, Item::Clip(_)))).collect()
}

/// Guesses the format of an interchange document from its content.
pub fn detect(text: &str) -> Option<crate::Format> {
    let head: String = text.chars().take(4096).collect();
    if head.contains("<xmeml") {
        Some(crate::Format::Xmeml)
    } else if head.contains("<fcpxml") {
        Some(crate::Format::Fcpxml)
    } else if head.contains("\"OTIO_SCHEMA\"") {
        Some(crate::Format::Otio)
    } else if head.lines().any(|l| l.starts_with("TITLE:") || l.starts_with("FCM:")) || head.lines().any(edl::is_event_line) {
        Some(crate::Format::Edl)
    } else {
        None
    }
}

/// Reads a timeline in any supported format. `rate` is used where the format has none (EDL).
pub fn import(text: &str, rate: clippro_types::FrameRate) -> Result<Timeline, String> {
    match detect(text) {
        Some(crate::Format::Xmeml) => fcp7::import(text),
        Some(crate::Format::Fcpxml) => fcpxml::import(text),
        Some(crate::Format::Otio) => otio::import(text),
        Some(crate::Format::Edl) => edl::import(text, rate),
        _ => Err("not a timeline ClipPro can read (FCP7 XML, FCPXML, OTIO or EDL)".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{media, selects};
    use crate::model::{build, ExportOptions, Layout};
    use crate::Format;
    use clippro_types::FrameRate;

    fn timelines() -> Vec<Timeline> {
        let mut out = vec![
            build(media(2), &selects(), &ExportOptions::default()),
            build(media(2), &selects(), &ExportOptions { gap_s: 1.0, ..ExportOptions::default() }),
            build(media(2), &selects(), &ExportOptions { layout: Layout::SelectsOverRejects, ..ExportOptions::default() }),
        ];
        let mut ntsc = media(1);
        ntsc[0].rate = FrameRate::new(30000, 1001);
        ntsc[0].drop_frame = true;
        ntsc[0].start_tc_frames = 0;
        out.push(build(ntsc, &selects()[..2], &ExportOptions::default()));
        out
    }

    fn markers(tl: &Timeline) -> Vec<(i64, i64, String)> {
        let mut v = Vec::new();
        for t in &tl.video {
            for it in &t.items {
                if let Item::Clip(c) = it {
                    v.extend(c.markers.iter().map(|m| (c.src_in + m.at, m.duration, m.name.clone())));
                }
            }
        }
        v
    }

    #[test]
    fn urls_to_paths() {
        assert_eq!(url_to_path("file://localhost/D:/Footage/Day%201/A.mov"), PathBuf::from("D:/Footage/Day 1/A.mov"));
        assert_eq!(url_to_path("file:///D:/A%20B.mov"), PathBuf::from("D:/A B.mov"));
        assert_eq!(url_to_path("file:///Users/me/a.mov"), PathBuf::from("/Users/me/a.mov"));
        assert_eq!(url_to_path("D:\\x.mov"), PathBuf::from("D:\\x.mov"));
        assert_eq!(url_to_path("file:///a%2"), PathBuf::from("/a%2"));
    }

    /// import(export(t)) ≅ t for FCP7 XML, FCPXML and OTIO (SPEC P11.1).
    #[test]
    fn xml_and_otio_round_trip() {
        for (n, t) in timelines().iter().enumerate() {
            for f in [Format::Xmeml, Format::Fcpxml, Format::Otio] {
                let text = f.render(t);
                assert_eq!(detect(&text), Some(f));
                let back = import(&text, FrameRate::new(25, 1)).unwrap_or_else(|e| panic!("{f:?} #{n}: {e}"));
                assert_eq!(edit_signature(&back), edit_signature(t), "{f:?} #{n}");
                assert_eq!(back.rate, t.rate, "{f:?} #{n}");
                assert_eq!(back.start_tc_frames, t.start_tc_frames, "{f:?} #{n}");
                assert_eq!(back.drop_frame, t.drop_frame, "{f:?} #{n}");
                assert_eq!(back.name, t.name, "{f:?} #{n}");
                assert_eq!(markers(&back), markers(t), "{f:?} #{n}");
                for (a, b) in back.media.iter().zip(&t.media) {
                    assert_eq!((a.rate, a.start_tc_frames, a.duration_frames, &a.name), (b.rate, b.start_tc_frames, b.duration_frames, &b.name), "{f:?} #{n}");
                }
                assert_eq!((back.width, back.height), (t.width, t.height), "{f:?} #{n}");
            }
        }
    }

    #[test]
    fn marker_colours_survive_where_the_format_has_them() {
        let t = &timelines()[0];
        let colours = |tl: &Timeline| -> Vec<crate::MarkerColor> {
            tl.video[0].items.iter().filter_map(|i| if let Item::Clip(c) = i { Some(c.markers[0].color) } else { None }).collect()
        };
        for f in [Format::Xmeml, Format::Otio] {
            assert_eq!(colours(&import(&f.render(t), t.rate).unwrap()), colours(t), "{f:?}");
        }
        let o = otio::import(&Format::Otio.render(t)).unwrap();
        let Item::Clip(c) = &o.video[0].items[0] else { panic!() };
        assert_eq!((c.score, c.select_index), (Some(0.82), Some(0)), "ClipPro metadata survives OTIO");
    }

    /// EDLs lose media start TC and rate; relinking to the project media restores the edit.
    #[test]
    fn edl_round_trip_through_relink() {
        for (n, t) in timelines().iter().enumerate() {
            let text = Format::Edl.render(t);
            assert_eq!(detect(&text), Some(Format::Edl));
            let mut back = import(&text, t.rate).unwrap();
            assert_eq!(back.drop_frame, t.drop_frame);
            assert_eq!(back.start_tc_frames, t.start_tc_frames, "#{n}");
            let rep = relink(&mut back, &t.media);
            assert!(rep.missing.is_empty(), "#{n}: {rep:?}");
            assert!(rep.linked.iter().all(|l| l.2 == LinkBy::Path));
            // EDL holds the first track only.
            let want: Vec<ClipSig> = edit_signature(t).into_iter().filter(|c| c.0 == 0).collect();
            assert_eq!(edit_signature(&back), want, "#{n}");
            let at: Vec<i64> = markers(&back).iter().map(|m| m.0).collect();
            let want_at: Vec<i64> = markers(t).iter().map(|m| m.0).collect();
            assert_eq!(at, want_at, "#{n}");
        }
    }

    #[test]
    fn relink_by_name_and_reel() {
        let t = build(media(2), &selects(), &ExportOptions::default());
        // Moved footage: the same files under another folder.
        let mut moved = media(2);
        for m in &mut moved {
            m.path = PathBuf::from("E:/Archive").join(m.path.file_name().unwrap());
        }
        let mut a = t.clone();
        let rep = relink(&mut a, &moved);
        assert!(rep.linked.iter().all(|l| l.2 == LinkBy::FileName));
        assert_eq!(a.media[0].path, moved[0].path);
        // Renamed files: only the reel and timecode match.
        let mut renamed = moved.clone();
        for (k, m) in renamed.iter_mut().enumerate() {
            m.path = PathBuf::from(format!("E:/Archive/clip_{k}.mxf"));
            m.name = format!("clip_{k}");
        }
        let mut b = t.clone();
        for m in &mut b.media {
            m.path = PathBuf::new();
            m.name = String::new();
        }
        let rep = relink(&mut b, &renamed);
        assert_eq!(rep.linked.iter().map(|l| (l.0, l.1, l.2)).collect::<Vec<_>>(), vec![(0, 0, LinkBy::ReelAndTimecode), (1, 1, LinkBy::ReelAndTimecode)]);
        assert_eq!(edit_signature(&b).iter().map(|c| (c.2, c.3, c.4)).collect::<Vec<_>>(), edit_signature(&t).iter().map(|c| (c.2, c.3, c.4)).collect::<Vec<_>>());
        // Nothing matches.
        let mut c = t.clone();
        let rep = relink(&mut c, &[]);
        assert_eq!(rep.missing, vec![0, 1]);
    }

    #[test]
    fn relink_rebases_start_timecode_and_rate() {
        let t = build(media(1), &selects()[..1], &ExportOptions::default());
        let mut p = media(1);
        p[0].rate = FrameRate::new(50, 1);
        p[0].duration_frames = 500;
        p[0].start_tc_frames = 2 * (25 * 3600 * 10); // same 10:00:00:00 at 50 fps
        let mut a = t.clone();
        relink(&mut a, &p);
        let Item::Clip(c) = &a.video[0].items[0] else { panic!() };
        assert_eq!((c.src_in, c.src_out), (12, 238));
        assert_eq!(c.markers[0].at, 38);
    }

    #[test]
    fn rejects_unknown_documents() {
        assert!(import("hello", FrameRate::new(25, 1)).is_err());
        assert!(import("<fcpxml version=\"1.10\"><resources>", FrameRate::new(25, 1)).is_err());
    }
}
