//! Hand-written timelines in the shapes Premiere, Resolve, FCP and Avid produce (SPEC P11.1):
//! transitions, compound clips, connected lanes, Clip.1 OTIO, dissolves and audio-only events.

use clippro_interchange::import::{self, LinkBy};
use clippro_interchange::model::{Item, MarkerColor, MediaRef, Timeline};
use clippro_types::FrameRate;
use std::path::{Path, PathBuf};

fn sample(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/interchange").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// (src_in, src_out, record_in) of the clips on track `k`.
fn clips(t: &Timeline, k: usize) -> Vec<(i64, i64, i64)> {
    let tr = &t.video[k];
    tr.items
        .iter()
        .zip(t.positions(tr))
        .filter_map(|(it, (rin, _))| if let Item::Clip(c) = it { Some((c.src_in, c.src_out, rin)) } else { None })
        .collect()
}

fn first_clip(t: &Timeline, k: usize) -> &clippro_interchange::model::ClipItem {
    t.video[k].items.iter().find_map(|i| if let Item::Clip(c) = i { Some(c) } else { None }).expect("a clip")
}

#[test]
fn premiere_xml_with_transition_and_disabled_clip() {
    let t = import::import(&sample("premiere_transition.xml"), FrameRate::new(25, 1)).expect("import");
    assert_eq!(t.name, "Interview Cut v2");
    assert_eq!(t.rate, FrameRate::new(24000, 1001));
    assert_eq!((t.width, t.height), (3840, 2160));
    assert_eq!(t.media.len(), 2);
    let a = &t.media[0];
    assert_eq!(a.path, PathBuf::from("D:/Shoot/Day 2/A001_C004_0912XY.mov"));
    assert_eq!((a.name.as_str(), a.start_tc_frames, a.duration_frames, a.reel.as_deref(), a.audio_channels), ("A001_C004_0912XY", 1_241_760, 2400, Some("A001"), 2));
    // The second clip's start is -1 (inside the dissolve): placed from its end, butted to the first.
    assert_eq!(clips(&t, 0), vec![(100, 212, 0), (40, 128, 112)]);
    assert_eq!(t.media[1].start_tc_frames, 1_241_568, "from the timecode string when no frame count");
    assert_eq!(clips(&t, 1), vec![(500, 524, 50)]);
    let m = &first_clip(&t, 0).markers[0];
    assert_eq!((m.at, m.duration, m.name.as_str(), m.comment.as_str()), (30, 0, "Good line", "use this"));
}

#[test]
fn resolve_fcpxml_with_compound_clip_and_lanes() {
    let t = import::import(&sample("resolve_compound.fcpxml"), FrameRate::new(25, 1)).expect("import");
    assert_eq!(t.name, "Drone Edit");
    assert_eq!(t.rate, FrameRate::new(30000, 1001));
    assert!(t.drop_frame);
    assert_eq!(t.start_tc_frames, 107_892, "01:00:00;00");
    assert_eq!(t.media.len(), 2);
    assert_eq!(t.media[0].path, PathBuf::from("/Volumes/RAID/Drone_0042.MP4"));
    assert_eq!(t.media[0].duration_frames, 1800);
    let b = &t.media[1];
    assert_eq!((b.name.as_str(), b.rate, b.start_tc_frames, b.audio_channels), ("Cam & B", FrameRate::new(25, 1), 900_000, 4));
    assert_eq!(b.path, PathBuf::from("/Volumes/RAID/Cam B/CamB_001.mxf"));
    assert_eq!(clips(&t, 0), vec![(30, 90, 0), (300, 330, 90)]);
    assert_eq!(clips(&t, 1), vec![(250, 300, 60)], "lane 1 over the gap; the audio lane is skipped");
    let m = &first_clip(&t, 0).markers[0];
    assert_eq!((m.at, m.duration, m.name.as_str()), (15, 1, "Reveal"));
}

#[test]
fn resolve_otio_clip1_with_missing_media() {
    let t = import::import(&sample("resolve_clip1.otio"), FrameRate::new(25, 1)).expect("import");
    assert_eq!((t.name.as_str(), t.rate, t.start_tc_frames), ("Resolve Export", FrameRate::new(24, 1), 86_400));
    assert_eq!(t.video.len(), 1, "the audio track is not a video track");
    assert_eq!(clips(&t, 0), vec![(48, 96, 12), (0, 24, 60)], "the transition takes no track time");
    assert_eq!(t.media[0].path, PathBuf::from("D:/Shoot/Take 3.mov"));
    assert_eq!((t.media[0].name.as_str(), t.media[0].start_tc_frames), ("Take 3", 1_036_800));
    assert_eq!((t.media[1].name.as_str(), t.media[1].path.as_os_str().is_empty()), ("Offline", true));
    let m = &first_clip(&t, 0).markers[0];
    assert_eq!((m.at, m.color, m.name.as_str()), (12, MarkerColor::Magenta, "focus buzz"));
}

#[test]
fn avid_edl_with_dissolve_relinked_by_reel() {
    let mut t = import::import(&sample("avid_dissolve.edl"), FrameRate::new(25, 1)).expect("import");
    assert_eq!(t.name, "AVID CUT 4");
    assert_eq!(t.start_tc_frames, 90_000);
    assert_eq!(t.media.len(), 2, "the zero-length dissolve source and the audio-only event are dropped");
    assert_eq!(clips(&t, 0), vec![(0, 50, 0), (0, 75, 50)]);
    let c = first_clip(&t, 0);
    assert_eq!(c.name, "A002C001_Beach");
    let pier = t.video[0].items.iter().filter_map(|i| if let Item::Clip(c) = i { Some(c) } else { None }).nth(1).expect("second clip");
    assert_eq!((pier.markers[0].at, pier.markers[0].color, pier.markers[0].name.as_str()), (25, MarkerColor::Blue, "pan starts"));
    // Project media: real start timecodes, names that differ from the EDL's.
    let pool = |reel: &str, start: i64| MediaRef {
        id: 7,
        name: format!("{reel}_proxy"),
        path: PathBuf::from(format!("D:/Card/{reel}_proxy.mov")),
        rate: FrameRate::new(25, 1),
        duration_frames: 10_000,
        start_tc_frames: start,
        drop_frame: false,
        width: 1920,
        height: 1080,
        audio_channels: 2,
        reel: Some(reel.into()),
    };
    let rep = import::relink(&mut t, &[pool("A003C007", 1_184_000), pool("A002C001", 1_079_990)]);
    assert_eq!(rep.linked, vec![(0, 1, LinkBy::ReelAndTimecode), (1, 0, LinkBy::ReelAndTimecode)]);
    assert_eq!(clips(&t, 0), vec![(10, 60, 0), (1000, 1075, 50)]);
}
