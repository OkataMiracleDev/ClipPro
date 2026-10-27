//! CMX3600 EDL with locators (SPEC §16.2) and Avid ALE + marker list (the 1.0 Avid path
//! until AAF is validated, PLAN P7.5).

use crate::model::{convert_frames, Item, MarkerColor, MediaRef, Timeline};
use clippro_types::Timecode;

/// 8-character reel name (uppercase alphanumerics) from the reel metadata or file name.
pub fn reel_name(m: &MediaRef, max: usize) -> String {
    let base = m.reel.clone().unwrap_or_else(|| m.name.clone());
    let s: String = base.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').map(|c| c.to_ascii_uppercase()).collect();
    let s = if s.is_empty() { "AX".to_string() } else { s };
    s.chars().take(max).collect()
}

/// Folds text to printable ASCII for EDL/ALE/Avid parsers ("·" and dashes become "-").
pub fn ascii(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '·' | '–' | '—' | '•' => Some('-'),
            '“' | '”' => Some('"'),
            '‘' | '’' => Some('\''),
            c if c.is_ascii() && !c.is_ascii_control() => Some(c),
            c if c.is_ascii_control() => Some(' '),
            _ => Some('?'),
        })
        .collect()
}

fn loc_color(c: MarkerColor) -> &'static str {
    match c {
        MarkerColor::Red => "RED",
        MarkerColor::Orange | MarkerColor::Yellow => "YELLOW",
        MarkerColor::Green => "GREEN",
        MarkerColor::Cyan => "CYAN",
        MarkerColor::Blue => "BLUE",
        MarkerColor::Purple | MarkerColor::Magenta => "MAGENTA",
        MarkerColor::White => "WHITE",
    }
}

fn tc(frames: i64, tl: &Timeline) -> String {
    Timecode::from_frames(frames, tl.rate, tl.drop_frame).to_string()
}

/// CMX3600 EDL of the first video track. Source and record timecodes are at the timeline
/// rate (media at other rates are converted).
pub fn export_edl(tl: &Timeline, long_reels: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!("TITLE: {}\n", tl.name.chars().filter(|c| !c.is_control()).collect::<String>()));
    out.push_str(if tl.drop_frame { "FCM: DROP FRAME\n\n" } else { "FCM: NON-DROP FRAME\n\n" });
    let Some(track) = tl.video.first() else { return out };
    let mut n = 0;
    for (it, (rin, rout)) in track.items.iter().zip(tl.positions(track)) {
        let Item::Clip(c) = it else { continue };
        n += 1;
        let m = &tl.media[c.media];
        let s_in = convert_frames(m.start_tc_frames + c.src_in, m.rate, tl.rate);
        let s_out = s_in + (rout - rin);
        let reel = reel_name(m, if long_reels { 32 } else { 8 });
        let what = if tl.with_audio && m.audio_channels > 0 { "AA/V" } else { "V" };
        out.push_str(&format!(
            "{n:03}  {reel:<8} {what:<5} C        {} {} {} {}\n",
            tc(s_in, tl),
            tc(s_out, tl),
            tc(tl.start_tc_frames + rin, tl),
            tc(tl.start_tc_frames + rout, tl)
        ));
        out.push_str(&format!("* FROM CLIP NAME: {}\n", ascii(&c.name)));
        out.push_str(&format!("* SOURCE FILE: {}\n", m.path.display()));
        for mk in &c.markers {
            let at = tl.start_tc_frames + rin + convert_frames(mk.at, m.rate, tl.rate);
            out.push_str(&format!("* LOC: {} {:<7} {}\n", tc(at, tl), loc_color(mk.color), ascii(&mk.name)));
        }
        out.push('\n');
    }
    out
}

fn avid_color(c: MarkerColor) -> &'static str {
    match c {
        MarkerColor::Red | MarkerColor::Orange => "red",
        MarkerColor::Yellow => "yellow",
        MarkerColor::Green => "green",
        MarkerColor::Cyan => "cyan",
        MarkerColor::Blue => "blue",
        MarkerColor::Purple | MarkerColor::Magenta => "magenta",
        MarkerColor::White => "white",
    }
}

/// Avid Log Exchange: one subclip row per select.
pub fn export_ale(tl: &Timeline) -> String {
    let fps = tl.rate.fps();
    let fps_s = if (fps - fps.round()).abs() < 1e-6 { format!("{}", fps.round() as i64) } else { format!("{fps:.2}") };
    let mut out = format!(
        "Heading\nFIELD_DELIM\tTABS\nVIDEO_FORMAT\t{}\nAUDIO_FORMAT\t48khz\nFPS\t{fps_s}\n\nColumn\nName\tTracks\tStart\tEnd\tTape\tSource File\tComments\tClipPro Score\n\nData\n",
        if tl.height >= 2160 { "2160" } else if tl.height >= 1080 { "1080" } else { "720" }
    );
    let Some(track) = tl.video.first() else { return out };
    for it in &track.items {
        let Item::Clip(c) = it else { continue };
        let m = &tl.media[c.media];
        let tracks = if m.audio_channels >= 2 { "VA1A2" } else if m.audio_channels == 1 { "VA1" } else { "V" };
        let start = Timecode::from_frames(m.start_tc_frames + c.src_in, m.rate, m.drop_frame);
        let end = Timecode::from_frames(m.start_tc_frames + c.src_out, m.rate, m.drop_frame);
        let comment = c.markers.first().map(|mk| format!("{} {}", mk.name, mk.comment)).unwrap_or_default();
        let name = match c.select_index {
            Some(i) => format!("{} Select {}", c.name, i + 1),
            None => c.name.clone(),
        };
        let file = m.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let score = c.score.map(|s| format!("{}", (s * 100.0).round() as i64)).unwrap_or_default();
        out.push_str(&format!("{}\t{tracks}\t{start}\t{end}\t{}\t{file}\t{}\t{score}\n", ascii(&name), reel_name(m, 32), ascii(&comment)));
    }
    out
}

/// Avid marker list (Media Composer "Import Markers"): name, TC, track, colour, comment, duration.
pub fn export_avid_markers(tl: &Timeline) -> String {
    let mut out = String::new();
    let Some(track) = tl.video.first() else { return out };
    for (it, (rin, _)) in track.items.iter().zip(tl.positions(track)) {
        let Item::Clip(c) = it else { continue };
        let m = &tl.media[c.media];
        for mk in &c.markers {
            let at = tl.start_tc_frames + rin + convert_frames(mk.at, m.rate, tl.rate);
            let dur = convert_frames(mk.duration, m.rate, tl.rate).max(1);
            out.push_str(&format!(
                "ClipPro\t{}\tV1\t{}\t{}\t{}\t\t{}\n",
                tc(at, tl),
                avid_color(mk.color),
                ascii(&format!("{} {}", mk.name, mk.comment)),
                dur,
                avid_color(mk.color)
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{media, selects};
    use crate::model::{build, ExportOptions};

    #[test]
    fn edl_events_and_locators() {
        let tl = build(media(2), &selects(), &ExportOptions::default());
        let e = export_edl(&tl, false);
        let lines: Vec<&str> = e.lines().collect();
        assert_eq!(lines[0], "TITLE: ClipPro Selects");
        assert_eq!(lines[1], "FCM: NON-DROP FRAME");
        assert_eq!(lines[3], "001  A001     AA/V  C        10:00:00:06 10:00:04:19 01:00:00:00 01:00:04:13");
        assert!(e.contains("* FROM CLIP NAME: A001C003"));
        assert!(e.contains("* LOC: 01:00:00:19 GREEN   Select 1 - 82"));
        assert_eq!(ascii("Café · “take” — ok"), "Caf? - \"take\" - ok");
        assert_eq!(e.matches("\n00").count(), 3);
    }

    #[test]
    fn reel_names() {
        let mut m = media(1).remove(0);
        m.reel = Some("a001-c003 (card 1)".into());
        assert_eq!(reel_name(&m, 8), "A001C003");
        m.reel = None;
        m.name = "Interview Sarah Wide".into();
        assert_eq!(reel_name(&m, 8), "INTERVIE");
    }

    #[test]
    fn ale_and_markers() {
        let tl = build(media(2), &selects(), &ExportOptions::default());
        let a = export_ale(&tl);
        assert!(a.starts_with("Heading\nFIELD_DELIM\tTABS\n"));
        assert!(a.contains("FPS\t25\n"));
        assert!(a.contains("A001C003 Select 1\tVA1A2\t10:00:00:06\t10:00:04:19\tA001\tA001C003.mov\t"));
        let mk = export_avid_markers(&tl);
        assert_eq!(mk.lines().count(), 3);
        assert!(mk.starts_with("ClipPro\t01:00:00:19\tV1\tgreen\t"));
    }
}
