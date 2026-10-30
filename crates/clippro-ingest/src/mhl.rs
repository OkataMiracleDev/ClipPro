//! ASC Media Hash List v2 history (`ascmhl/`): one manifest per generation plus the
//! `ascmhl_chain.xml` that chains them by C4 ID.

use crate::hash::{c4_id, HashKind};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DIR: &str = "ascmhl";
pub const CHAIN: &str = "ascmhl_chain.xml";

/// One file in a manifest.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Relative to the history's root, with `/` separators.
    pub path: String,
    pub size: u64,
    pub modified: SystemTime,
    pub kind: HashKind,
    pub hash: String,
    /// "original", "verified" or "failed".
    pub action: &'static str,
}

/// (year, month, day, hour, minute, second) in UTC.
pub fn civil(t: SystemTime) -> (i64, u32, u32, u32, u32, u32) {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, (rem / 3600) as u32, (rem / 60 % 60) as u32, (rem % 60) as u32)
}

/// ISO 8601 with offset, as ASC MHL writes dates: `2026-10-30T09:15:00+00:00`.
pub fn iso(t: SystemTime) -> String {
    let (y, mo, d, h, mi, s) = civil(t);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}+00:00")
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Generation numbers already in the history (from the manifest file names).
pub fn generations(root: &Path) -> Vec<u32> {
    let Ok(rd) = std::fs::read_dir(root.join(DIR)) else { return Vec::new() };
    let mut v: Vec<u32> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            if !n.ends_with(".mhl") {
                return None;
            }
            n.split('_').next()?.parse().ok()
        })
        .collect();
    v.sort_unstable();
    v
}

/// Renders a manifest.
pub fn manifest(entries: &[Entry], process: &str, ignore: &[String], now: SystemTime) -> String {
    let host = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "localhost".into());
    let mut x = String::new();
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<hashlist version=\"2.0\" xmlns=\"urn:ASC:MHL:v2.0\">\n");
    x.push_str("  <creatorinfo>\n");
    let _ = writeln!(x, "    <creationdate>{}</creationdate>", iso(now));
    let _ = writeln!(x, "    <hostname>{}</hostname>", esc(&host));
    let _ = writeln!(x, "    <tool version=\"{}\">ClipPro</tool>", env!("CARGO_PKG_VERSION"));
    x.push_str("  </creatorinfo>\n  <processinfo>\n");
    let _ = writeln!(x, "    <process>{process}</process>");
    x.push_str("    <ignore>\n");
    for p in ignore {
        let _ = writeln!(x, "      <pattern>{}</pattern>", esc(p));
    }
    x.push_str("    </ignore>\n  </processinfo>\n  <hashes>\n");
    for e in entries {
        x.push_str("    <hash>\n");
        let _ = writeln!(x, "      <path size=\"{}\" lastmodificationdate=\"{}\">{}</path>", e.size, iso(e.modified), esc(&e.path));
        let _ = writeln!(x, "      <{k} action=\"{}\" hashdate=\"{}\">{}</{k}>", e.action, iso(now), e.hash, k = e.kind.mhl_name());
        x.push_str("    </hash>\n");
    }
    x.push_str("  </hashes>\n</hashlist>\n");
    x
}

/// Writes the next generation into `root/ascmhl/` and rewrites the chain; returns the manifest.
pub fn write_generation(root: &Path, entries: &[Entry], process: &str, ignore: &[String], now: SystemTime) -> std::io::Result<PathBuf> {
    let dir = root.join(DIR);
    std::fs::create_dir_all(&dir)?;
    let seq = generations(root).last().copied().unwrap_or(0) + 1;
    let (y, mo, d, h, mi, s) = civil(now);
    let folder = root.file_name().map(|f| f.to_string_lossy().replace(['_', ' '], "-")).unwrap_or_else(|| "root".into());
    let name = format!("{seq:04}_{folder}_{y:04}-{mo:02}-{d:02}_{h:02}{mi:02}{s:02}Z.mhl");
    let text = manifest(entries, process, ignore, now);
    std::fs::write(dir.join(&name), &text)?;
    // The chain lists every generation with the C4 ID of its manifest.
    let mut gens: Vec<(u32, String)> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".mhl"))
        .filter_map(|n| Some((n.split('_').next()?.parse().ok()?, n)))
        .collect();
    gens.sort();
    let mut chain = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ascmhldirectory xmlns=\"urn:ASC:MHL:DIRECTORY:v2.0\">\n");
    for (n, file) in gens {
        let bytes = std::fs::read(dir.join(&file))?;
        let _ = writeln!(chain, "  <hashlist sequencenr=\"{n}\">\n    <path>{}</path>\n    <c4>{}</c4>\n  </hashlist>", esc(&file), c4_id(&bytes));
    }
    chain.push_str("</ascmhldirectory>\n");
    std::fs::write(dir.join(CHAIN), chain)?;
    Ok(dir.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn dates() {
        let t = UNIX_EPOCH + Duration::from_secs(1_579_166_100); // 2020-01-16 09:15:00 UTC
        assert_eq!(iso(t), "2020-01-16T09:15:00+00:00");
        assert_eq!(civil(UNIX_EPOCH + Duration::from_secs(951_782_400)), (2000, 2, 29, 0, 0, 0));
    }

    #[test]
    fn manifest_shape() {
        let t = UNIX_EPOCH + Duration::from_secs(1_579_166_100);
        let e = Entry { path: "Clips/A&B.mov".into(), size: 3, modified: t, kind: HashKind::Xxh64, hash: "0ea03b369a463d9d".into(), action: "original" };
        let x = manifest(&[e], "transfer", &[".DS_Store".into()], t);
        assert!(x.contains("<hashlist version=\"2.0\" xmlns=\"urn:ASC:MHL:v2.0\">"));
        assert!(x.contains("<path size=\"3\" lastmodificationdate=\"2020-01-16T09:15:00+00:00\">Clips/A&amp;B.mov</path>"));
        assert!(x.contains("<xxh64 action=\"original\" hashdate=\"2020-01-16T09:15:00+00:00\">0ea03b369a463d9d</xxh64>"));
        assert!(x.contains("<process>transfer</process>"));
    }
}
