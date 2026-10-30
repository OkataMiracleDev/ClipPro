//! Checksums for offload verification and ASC MHL (SPEC §19): xxh64 by default; xxh3, xxh128,
//! MD5 and SHA-1 on request.

use md5::Digest as _;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HashKind {
    #[default]
    Xxh64,
    Xxh3,
    Xxh128,
    Md5,
    Sha1,
}

impl HashKind {
    /// Element name in ASC MHL v2.
    pub fn mhl_name(self) -> &'static str {
        match self {
            HashKind::Xxh64 => "xxh64",
            HashKind::Xxh3 => "xxh3",
            HashKind::Xxh128 => "xxh128",
            HashKind::Md5 => "md5",
            HashKind::Sha1 => "sha1",
        }
    }

    pub fn parse(s: &str) -> Option<HashKind> {
        [HashKind::Xxh64, HashKind::Xxh3, HashKind::Xxh128, HashKind::Md5, HashKind::Sha1].into_iter().find(|k| k.mhl_name().eq_ignore_ascii_case(s.trim()))
    }

    pub fn hasher(self) -> Hasher {
        match self {
            HashKind::Xxh64 => Hasher::Xxh64(Box::new(xxhash_rust::xxh64::Xxh64::new(0))),
            HashKind::Xxh3 => Hasher::Xxh3(Box::new(xxhash_rust::xxh3::Xxh3::new()), false),
            HashKind::Xxh128 => Hasher::Xxh3(Box::new(xxhash_rust::xxh3::Xxh3::new()), true),
            HashKind::Md5 => Hasher::Md5(md5::Md5::new()),
            HashKind::Sha1 => Hasher::Sha1(sha1::Sha1::new()),
        }
    }
}

/// A streaming hasher; `finish` gives lowercase hex as ASC MHL writes it.
pub enum Hasher {
    Xxh64(Box<xxhash_rust::xxh64::Xxh64>),
    /// (state, 128-bit)
    Xxh3(Box<xxhash_rust::xxh3::Xxh3>, bool),
    Md5(md5::Md5),
    Sha1(sha1::Sha1),
}

impl Hasher {
    pub fn update(&mut self, b: &[u8]) {
        match self {
            Hasher::Xxh64(h) => h.update(b),
            Hasher::Xxh3(h, _) => h.update(b),
            Hasher::Md5(h) => h.update(b),
            Hasher::Sha1(h) => h.update(b),
        }
    }

    pub fn finish(self) -> String {
        match self {
            Hasher::Xxh64(h) => format!("{:016x}", h.digest()),
            Hasher::Xxh3(h, false) => format!("{:016x}", h.digest()),
            Hasher::Xxh3(h, true) => format!("{:032x}", h.digest128()),
            Hasher::Md5(h) => hex(&h.finalize()),
            Hasher::Sha1(h) => hex(&h.finalize()),
        }
    }
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// One-shot hash of bytes.
pub fn hash_bytes(kind: HashKind, b: &[u8]) -> String {
    let mut h = kind.hasher();
    h.update(b);
    h.finish()
}

/// C4 ID (SMPTE ST 2114): SHA-512, base58 with the C4 alphabet, left-padded with '1' to 88
/// characters, prefixed "c4". ASC MHL uses it to chain manifest generations.
pub fn c4_id(b: &[u8]) -> String {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let digest = sha2::Sha512::digest(b);
    // Base-58 of the 512-bit big-endian number, by repeated division.
    let mut num: Vec<u8> = digest.to_vec();
    let mut out = Vec::with_capacity(88);
    while num.iter().any(|&d| d != 0) {
        let mut rem = 0u32;
        for d in num.iter_mut() {
            let acc = (rem << 8) | *d as u32;
            *d = (acc / 58) as u8;
            rem = acc % 58;
        }
        out.push(ALPHABET[rem as usize]);
    }
    while out.len() < 88 {
        out.push(b'1');
    }
    out.reverse();
    format!("c4{}", String::from_utf8_lossy(&out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(hash_bytes(HashKind::Xxh64, b""), "ef46db3751d8e999");
        assert_eq!(hash_bytes(HashKind::Md5, b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hash_bytes(HashKind::Sha1, b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(hash_bytes(HashKind::Xxh3, b"").len(), 16);
        assert_eq!(hash_bytes(HashKind::Xxh128, b"").len(), 32);
        assert_eq!(HashKind::parse(" XXH64 "), Some(HashKind::Xxh64));
        // Streaming in pieces equals one shot.
        let mut h = HashKind::Xxh64.hasher();
        h.update(b"hello ");
        h.update(b"world");
        assert_eq!(h.finish(), hash_bytes(HashKind::Xxh64, b"hello world"));
    }

    #[test]
    fn c4_ids() {
        // SMPTE ST 2114 reference: the ID of the empty string.
        assert_eq!(c4_id(b""), "c459dsjfscH38cYeXXYogktxf4Cd9ibshE3BHUo6a58hBXmRQdZrAkZzsWcbWtDg5oQstpDuni4Hirj75GEmTc1sFT");
        assert_eq!(c4_id(b"foo").len(), 90);
    }
}
