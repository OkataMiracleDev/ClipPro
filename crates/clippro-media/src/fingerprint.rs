//! File fingerprint for caching and relinking (SPEC §9.1, P1.7):
//! `xxh3_128(size ‖ mtime ‖ first 1 MiB ‖ last 1 MiB ‖ 4 × 64 KiB at 20/40/60/80 %)`.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::UNIX_EPOCH;
use xxhash_rust::xxh3::Xxh3;

const MIB: u64 = 1 << 20;
const CHUNK: u64 = 64 << 10;

fn read_at(f: &mut File, off: u64, len: u64, buf: &mut Vec<u8>) -> std::io::Result<()> {
    buf.resize(len as usize, 0);
    f.seek(SeekFrom::Start(off))?;
    f.read_exact(buf)?;
    Ok(())
}

/// Content fingerprint including size and modification time (hex, 32 chars).
pub fn fingerprint(path: &Path) -> std::io::Result<String> {
    let meta = std::fs::metadata(path)?;
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
    let mut h = content_hasher(path, meta.len())?;
    h.update(&mtime.to_le_bytes());
    Ok(format!("{:032x}", h.digest128()))
}

/// Fingerprint ignoring mtime (used to recognise the same media after a copy/move).
pub fn content_fingerprint(path: &Path) -> std::io::Result<String> {
    let len = std::fs::metadata(path)?.len();
    Ok(format!("{:032x}", content_hasher(path, len)?.digest128()))
}

fn content_hasher(path: &Path, size: u64) -> std::io::Result<Xxh3> {
    let mut f = File::open(path)?;
    let mut h = Xxh3::new();
    h.update(&size.to_le_bytes());
    let mut buf = Vec::new();
    if size <= 2 * MIB + 4 * CHUNK {
        f.read_to_end(&mut buf)?;
        h.update(&buf);
        return Ok(h);
    }
    read_at(&mut f, 0, MIB, &mut buf)?;
    h.update(&buf);
    read_at(&mut f, size - MIB, MIB, &mut buf)?;
    h.update(&buf);
    for pct in [20u64, 40, 60, 80] {
        read_at(&mut f, size * pct / 100, CHUNK, &mut buf)?;
        h.update(&buf);
    }
    Ok(h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn stable_and_sensitive() {
        // Under target/ (not the system temp folder on C:), removed at the end.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-fp-test");
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.bin");
        let data: Vec<u8> = (0..5_000_000u32).map(|i| (i * 31 % 251) as u8).collect();
        std::fs::File::create(&a).unwrap().write_all(&data).unwrap();
        let b = dir.join("b.bin");
        std::fs::copy(&a, &b).unwrap();
        assert_eq!(content_fingerprint(&a).unwrap(), content_fingerprint(&b).unwrap());
        // Change one byte inside the 40 % chunk.
        let mut d2 = data.clone();
        d2[2_000_010] ^= 0xFF;
        std::fs::File::create(&b).unwrap().write_all(&d2).unwrap();
        assert_ne!(content_fingerprint(&a).unwrap(), content_fingerprint(&b).unwrap());
        assert_eq!(fingerprint(&a).unwrap().len(), 32);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
