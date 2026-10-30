//! AAF (SPEC §16.2 EXP-3, PLAN P7b/P11.2): a writer and reader for the object-model subset
//! ClipPro needs: CompositionMob (picture tracks, timecode, DescriptiveMarkers with colour),
//! MasterMob and SourceMob (CDCI descriptor, NetworkLocator, timecode), Sequence, SourceClip,
//! Filler.
//!
//! Writing fills an embedded template (`assets/aaf-template.aaf`, authored by pyaaf2 through
//! `scripts/aaf/make_template.py`). The template carries the full baseline MetaDictionary, the
//! default Dictionary and an empty ContentStorage, so only the mobs are written here, in the
//! AAF Low-Level Container encoding: one compound-file storage per object (CLSID = class AUID)
//! with a `properties` stream, child storages for strong references, `index` streams for strong
//! reference vectors and sets, and weak references through the `referenced properties` table.

use crate::model::{convert_frames, ClipItem, Item, Marker, MarkerColor, MediaRef, Timeline, Track};
use cfb::CompoundFile;
use std::io::{Cursor, Read, Write};
use uuid::Uuid;

const TEMPLATE: &[u8] = include_bytes!("../assets/aaf-template.aaf");

// Stored forms.
const SF_DATA: u16 = 0x82;
const SF_STRONG: u16 = 0x22;
const SF_STRONG_VECTOR: u16 = 0x32;
const SF_WEAK: u16 = 0x02;

// Classes.
const CLS_COMPOSITION_MOB: &str = "0d010101-0101-3500-060e-2b3402060101";
const CLS_MASTER_MOB: &str = "0d010101-0101-3600-060e-2b3402060101";
const CLS_SOURCE_MOB: &str = "0d010101-0101-3700-060e-2b3402060101";
const CLS_TIMELINE_SLOT: &str = "0d010101-0101-3b00-060e-2b3402060101";
const CLS_EVENT_SLOT: &str = "0d010101-0101-3900-060e-2b3402060101";
const CLS_SEQUENCE: &str = "0d010101-0101-0f00-060e-2b3402060101";
const CLS_SOURCE_CLIP: &str = "0d010101-0101-1100-060e-2b3402060101";
const CLS_FILLER: &str = "0d010101-0101-0900-060e-2b3402060101";
const CLS_TIMECODE: &str = "0d010101-0101-1400-060e-2b3402060101";
const CLS_MARKER: &str = "0d010101-0101-4100-060e-2b3402060101";
const CLS_CDCI: &str = "0d010101-0101-2800-060e-2b3402060101";
const CLS_NETWORK_LOCATOR: &str = "0d010101-0101-3200-060e-2b3402060101";

// Data definitions.
const DD_PICTURE: &str = "01030202-0100-0000-060e-2b3404010101";
const DD_TIMECODE: &str = "01030201-0100-0000-060e-2b3404010101";
const DD_DESCRIPTIVE: &str = "01030201-1000-0000-060e-2b3404010101";

/// Index of Header → Dictionary → DataDefinitions in the template's weak-reference table.
const WEAKREF_DATADEFS: u16 = 2;
/// Dynamic pid of the CommentMarkerColor extension property in the template's MetaDictionary.
const PID_MARKER_COLOR: u16 = 0xFFB9;

/// SMPTE UMID label prefix pyaaf2 (and the AAF SDK) use for generated MobIDs.
const MOBID_PREFIX: [u8; 16] = [0x06, 0x0a, 0x2b, 0x34, 0x01, 0x01, 0x01, 0x05, 0x01, 0x01, 0x0f, 0x20, 0x13, 0x00, 0x00, 0x00];

fn auid(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap_or(Uuid::nil())
}

fn utf16z(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    v.extend([0, 0]);
    v
}

fn from_utf16z(b: &[u8]) -> String {
    let u: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|c| *c != 0).collect();
    String::from_utf16_lossy(&u)
}

/// AAF timestamp: year u16, month, day, hour, minute, second, 1/250 s.
fn timestamp(t: std::time::SystemTime) -> Vec<u8> {
    let secs = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let mut v = (y as u16).to_le_bytes().to_vec();
    v.extend([m as u8, d as u8, (rem / 3600) as u8, (rem / 60 % 60) as u8, (rem % 60) as u8, 0]);
    v
}

/// A 32-byte MobID: the UMID prefix plus 16 bytes derived from `seed` (stable per export).
fn mob_id(seed: &str, salt: u64) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[..16].copy_from_slice(&MOBID_PREFIX);
    // Two FNV-1a passes give 16 bytes; mark them as a version-4-style UUID.
    let mut h1: u64 = 0xcbf2_9ce4_8422_2325 ^ salt;
    let mut h2: u64 = 0x8422_2325_cbf2_9ce4 ^ salt.rotate_left(17);
    for b in seed.bytes() {
        h1 = (h1 ^ b as u64).wrapping_mul(0x0100_0000_01b3);
        h2 = (h2 ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3 ^ 0x5bd1_e995);
    }
    out[16..24].copy_from_slice(&h1.to_le_bytes());
    out[24..32].copy_from_slice(&h2.to_le_bytes());
    out[22] = (out[22] & 0x0f) | 0x40;
    out[24] = (out[24] & 0x3f) | 0x80;
    out
}

fn rational(num: i64, den: i64) -> Vec<u8> {
    let mut v = (num as i32).to_le_bytes().to_vec();
    v.extend((den as i32).to_le_bytes());
    v
}

fn weak_datadef(dd: &str) -> Vec<u8> {
    let mut v = WEAKREF_DATADEFS.to_le_bytes().to_vec();
    v.extend(0x1B01u16.to_le_bytes());
    v.push(16);
    v.extend(auid(dd).to_bytes_le());
    v
}

/// One AAF object to write.
struct Obj {
    class: &'static str,
    props: Vec<(u16, Prop)>,
}

enum Prop {
    Data(Vec<u8>),
    Weak(Vec<u8>),
    Strong(&'static str, Box<Obj>),
    Vector(&'static str, Vec<Obj>),
}

impl Obj {
    fn new(class: &'static str) -> Obj {
        Obj { class, props: Vec::new() }
    }
    fn data(mut self, pid: u16, v: Vec<u8>) -> Obj {
        self.props.push((pid, Prop::Data(v)));
        self
    }
    fn weak(mut self, pid: u16, v: Vec<u8>) -> Obj {
        self.props.push((pid, Prop::Weak(v)));
        self
    }
    fn strong(mut self, pid: u16, name: &'static str, o: Obj) -> Obj {
        self.props.push((pid, Prop::Strong(name, Box::new(o))));
        self
    }
    fn vector(mut self, pid: u16, name: &'static str, v: Vec<Obj>) -> Obj {
        self.props.push((pid, Prop::Vector(name, v)));
        self
    }
}

fn props_stream(entries: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut v = vec![0x4c, 32];
    v.extend((entries.len() as u16).to_le_bytes());
    for (pid, fmt, data) in entries {
        v.extend(pid.to_le_bytes());
        v.extend(fmt.to_le_bytes());
        v.extend((data.len() as u16).to_le_bytes());
    }
    for (_, _, data) in entries {
        v.extend(data);
    }
    v
}

fn parse_props(b: &[u8]) -> Vec<(u16, u16, Vec<u8>)> {
    if b.len() < 4 || b[0] != 0x4c {
        return Vec::new();
    }
    let n = u16::from_le_bytes([b[2], b[3]]) as usize;
    let mut heads = Vec::with_capacity(n);
    let mut at = 4;
    for _ in 0..n {
        if at + 6 > b.len() {
            return Vec::new();
        }
        let f = |i: usize| u16::from_le_bytes([b[at + i], b[at + i + 1]]);
        heads.push((f(0), f(2), f(4) as usize));
        at += 6;
    }
    let mut out = Vec::with_capacity(n);
    for (pid, fmt, len) in heads {
        let data = b.get(at..at + len).map(<[u8]>::to_vec).unwrap_or_default();
        at += len;
        out.push((pid, fmt, data));
    }
    out
}

type Cf = CompoundFile<Cursor<Vec<u8>>>;

fn io_err(e: std::io::Error) -> String {
    format!("AAF: {e}")
}

fn write_stream(cf: &mut Cf, path: &str, data: &[u8]) -> Result<(), String> {
    let mut s = cf.create_stream(path).map_err(io_err)?;
    s.write_all(data).map_err(io_err)
}

fn read_stream(cf: &mut Cf, path: &str) -> Option<Vec<u8>> {
    let mut s = cf.open_stream(path).ok()?;
    let mut v = Vec::new();
    s.read_to_end(&mut v).ok()?;
    Some(v)
}

/// Writes `obj` into the (new) storage at `path`.
fn write_obj(cf: &mut Cf, path: &str, obj: &Obj) -> Result<(), String> {
    if !cf.exists(path) {
        cf.create_storage(path).map_err(io_err)?;
    }
    cf.set_storage_clsid(path, auid(obj.class)).map_err(io_err)?;
    let mut entries = Vec::new();
    for (pid, p) in &obj.props {
        match p {
            Prop::Data(d) => entries.push((*pid, SF_DATA, d.clone())),
            Prop::Weak(d) => entries.push((*pid, SF_WEAK, d.clone())),
            Prop::Strong(name, child) => {
                entries.push((*pid, SF_STRONG, utf16z(name)));
                write_obj(cf, &format!("{path}/{name}"), child)?;
            }
            Prop::Vector(name, items) => {
                entries.push((*pid, SF_STRONG_VECTOR, utf16z(name)));
                let mut idx = Vec::new();
                idx.extend((items.len() as u32).to_le_bytes());
                idx.extend((items.len() as u32).to_le_bytes());
                idx.extend(0xFFFF_FFFFu32.to_le_bytes());
                for (k, item) in items.iter().enumerate() {
                    idx.extend((k as u32).to_le_bytes());
                    write_obj(cf, &format!("{path}/{name}{{{k:x}}}"), item)?;
                }
                write_stream(cf, &format!("{path}/{name} index"), &idx)?;
            }
        }
    }
    write_stream(cf, &format!("{path}/properties"), &props_stream(&entries))
}

fn mob(class: &'static str, name: &str, id: &[u8; 32], now: &[u8], slots: Vec<Obj>) -> Obj {
    Obj::new(class)
        .data(0x4402, utf16z(name))
        .data(0x4401, id.to_vec())
        .data(0x4405, now.to_vec())
        .data(0x4404, now.to_vec())
        .vector(0x4403, "Slots-4403", slots)
}

fn timeline_slot(id: u32, track: Option<u32>, rate: (i64, i64), segment: Obj) -> Obj {
    let mut o = Obj::new(CLS_TIMELINE_SLOT).data(0x4801, id.to_le_bytes().to_vec()).data(0x4802, utf16z(""));
    if let Some(t) = track {
        o = o.data(0x4804, t.to_le_bytes().to_vec());
    }
    o.data(0x4b02, 0i64.to_le_bytes().to_vec()).data(0x4b01, rational(rate.0, rate.1)).strong(0x4803, "Segment-4803", segment)
}

fn source_clip(len: i64, start: i64, source: &[u8; 32], slot: u32) -> Obj {
    Obj::new(CLS_SOURCE_CLIP)
        .weak(0x0201, weak_datadef(DD_PICTURE))
        .data(0x0202, len.to_le_bytes().to_vec())
        .data(0x1201, start.to_le_bytes().to_vec())
        .data(0x1101, source.to_vec())
        .data(0x1102, slot.to_le_bytes().to_vec())
}

fn timecode(len: i64, start: i64, fps: u16, drop: bool) -> Obj {
    Obj::new(CLS_TIMECODE)
        .weak(0x0201, weak_datadef(DD_TIMECODE))
        .data(0x0202, len.to_le_bytes().to_vec())
        .data(0x1501, start.to_le_bytes().to_vec())
        .data(0x1502, fps.to_le_bytes().to_vec())
        .data(0x1503, vec![drop as u8])
}

fn rate_of(r: clippro_types::FrameRate) -> (i64, i64) {
    (r.0.num, r.0.den)
}

fn colour_rgb(c: MarkerColor) -> [u16; 3] {
    match c {
        MarkerColor::Red => [65535, 0, 0],
        MarkerColor::Orange => [65535, 32768, 0],
        MarkerColor::Yellow => [65535, 65535, 0],
        MarkerColor::Green => [0, 65535, 0],
        MarkerColor::Cyan => [0, 65535, 65535],
        MarkerColor::Blue => [0, 0, 65535],
        MarkerColor::Purple => [32768, 0, 65535],
        MarkerColor::Magenta => [65535, 0, 65535],
        MarkerColor::White => [65535, 65535, 65535],
    }
}

fn nearest_colour(rgb: [u16; 3]) -> MarkerColor {
    MarkerColor::ALL
        .into_iter()
        .min_by_key(|c| {
            let k = colour_rgb(*c);
            (0..3).map(|i| (k[i] as i64 - rgb[i] as i64).pow(2)).sum::<i64>()
        })
        .unwrap_or(MarkerColor::Red)
}

/// The timeline as AAF bytes.
pub fn export(tl: &Timeline) -> Result<Vec<u8>, String> {
    let mut cf = CompoundFile::open(Cursor::new(TEMPLATE.to_vec())).map_err(io_err)?;
    let now = timestamp(std::time::SystemTime::now());
    let rate = rate_of(tl.rate);
    let mut mobs: Vec<([u8; 32], Obj)> = Vec::new();
    // Per media: a SourceMob (file, descriptor, timecode) and a MasterMob over it.
    let mut masters: Vec<[u8; 32]> = Vec::new();
    for (i, m) in tl.media.iter().enumerate() {
        let seed = format!("{}|{}", m.path.display(), m.name);
        let (src_id, master_id) = (mob_id(&seed, 1 + 2 * i as u64), mob_id(&seed, 2 + 2 * i as u64));
        let mr = rate_of(m.rate);
        let len = m.duration_frames.max(1);
        let locator = Obj::new(CLS_NETWORK_LOCATOR).data(0x4001, utf16z(&crate::xml::file_url(&m.path)));
        let desc = Obj::new(CLS_CDCI)
            .data(0x3301, 8u32.to_le_bytes().to_vec())
            .data(0x3302, 2u32.to_le_bytes().to_vec())
            .data(0x320e, rational(m.width.max(1) as i64, m.height.max(1) as i64))
            .data(0x3203, m.width.to_le_bytes().to_vec())
            .data(0x3202, m.height.to_le_bytes().to_vec())
            .data(0x320c, vec![0])
            .data(0x320d, [42i32.to_le_bytes(), 0i32.to_le_bytes()].concat())
            .data(0x3001, rational(mr.0, mr.1))
            .data(0x3002, len.to_le_bytes().to_vec())
            .vector(0x2f01, "Locator-2f01", vec![locator]);
        let src_slots = vec![
            timeline_slot(1, None, mr, source_clip(len, 0, &[0; 32], 0)),
            timeline_slot(2, None, mr, timecode(len, m.start_tc_frames, m.rate.timecode_base() as u16, m.drop_frame)),
        ];
        let src = mob(CLS_SOURCE_MOB, &m.name, &src_id, &now, src_slots).strong(0x4701, "EssenceDescription-4701", desc);
        let master = mob(CLS_MASTER_MOB, &m.name, &master_id, &now, vec![timeline_slot(1, None, mr, source_clip(len, 0, &src_id, 1))]);
        mobs.push((src_id, src));
        mobs.push((master_id, master));
        masters.push(master_id);
    }
    // The composition: one picture slot per video track, timecode, and markers.
    let mut slots = Vec::new();
    let mut events = Vec::new();
    for (k, track) in tl.video.iter().enumerate() {
        let mut comps = Vec::new();
        let mut total = 0i64;
        for (it, (rin, _)) in track.items.iter().zip(tl.positions(track)) {
            let len = tl.item_len(it);
            match it {
                Item::Gap(_) | Item::Slug { .. } => comps.push(Obj::new(CLS_FILLER).weak(0x0201, weak_datadef(DD_PICTURE)).data(0x0202, len.to_le_bytes().to_vec())),
                Item::Clip(c) => {
                    let m = &tl.media[c.media];
                    let start = convert_frames(c.src_in, m.rate, tl.rate);
                    comps.push(source_clip(len, start, &masters[c.media], 1));
                    for mk in &c.markers {
                        let pos = rin + convert_frames(mk.at, m.rate, tl.rate);
                        events.push((k as u32 + 1, pos, convert_frames(mk.duration, m.rate, tl.rate).max(1), mk));
                    }
                }
            }
            total += len;
        }
        let seq = Obj::new(CLS_SEQUENCE).weak(0x0201, weak_datadef(DD_PICTURE)).data(0x0202, total.to_le_bytes().to_vec()).vector(0x1001, "Components-1001", comps);
        slots.push(timeline_slot(k as u32 + 1, Some(k as u32 + 1), rate, seq));
    }
    let n_video = tl.video.len() as u32;
    slots.push(timeline_slot(n_video + 1, Some(1), rate, timecode(tl.duration().max(1), tl.start_tc_frames, tl.rate.timecode_base() as u16, tl.drop_frame)));
    if !events.is_empty() {
        events.sort_by_key(|e| e.1);
        let markers: Vec<Obj> = events
            .iter()
            .map(|(slot, pos, len, mk)| {
                let comment = if mk.comment.trim().is_empty() { mk.name.clone() } else { format!("{} – {}", mk.name, mk.comment) };
                let rgb = colour_rgb(mk.color);
                Obj::new(CLS_MARKER)
                    .weak(0x0201, weak_datadef(DD_DESCRIPTIVE))
                    .data(0x0202, len.to_le_bytes().to_vec())
                    .data(0x0601, pos.to_le_bytes().to_vec())
                    .data(0x0602, utf16z(&comment))
                    .data(PID_MARKER_COLOR, rgb.iter().flat_map(|c| c.to_le_bytes()).collect())
                    .data(0x6102, slot.to_le_bytes().to_vec())
            })
            .collect();
        let seq = Obj::new(CLS_SEQUENCE).weak(0x0201, weak_datadef(DD_DESCRIPTIVE)).data(0x0202, 0i64.to_le_bytes().to_vec()).vector(0x1001, "Components-1001", markers);
        slots.push(
            Obj::new(CLS_EVENT_SLOT)
                .data(0x4802, utf16z(""))
                .data(0x4901, rational(rate.0, rate.1))
                .data(0x4801, 1000u32.to_le_bytes().to_vec())
                .strong(0x4803, "Segment-4803", seq),
        );
    }
    let comp_id = mob_id(&format!("{}|composition", tl.name), 0);
    // Usage code: top-level composition (Avid shows it as a sequence).
    let comp = mob(CLS_COMPOSITION_MOB, &tl.name, &comp_id, &now, slots).data(0x4408, auid("0d010102-0101-0700-060e-2b3404010101").to_bytes_le().to_vec());
    mobs.insert(0, (comp_id, comp));

    // Content → Mobs (a strong reference set keyed by MobID).
    let content = "/Header-2/Content-3b03";
    let mut idx = Vec::new();
    idx.extend((mobs.len() as u32).to_le_bytes());
    idx.extend((mobs.len() as u32).to_le_bytes());
    idx.extend(0xFFFF_FFFFu32.to_le_bytes());
    idx.extend(0x4401u16.to_le_bytes());
    idx.push(32);
    for (k, (id, obj)) in mobs.iter().enumerate() {
        idx.extend((k as u32).to_le_bytes());
        idx.extend(1u32.to_le_bytes());
        idx.extend(id);
        write_obj(&mut cf, &format!("{content}/Mobs-1901{{{k:x}}}"), obj)?;
    }
    write_stream(&mut cf, &format!("{content}/Mobs-1901 index"), &idx)?;
    // Stamp the header and identification.
    for (path, pids) in [("/Header-2/properties", &[0x3b02u16][..]), ("/Header-2/Identifi-ionList-3b06{0}/properties", &[0x3c06u16][..])] {
        if let Some(b) = read_stream(&mut cf, path) {
            let mut e = parse_props(&b);
            for x in e.iter_mut().filter(|x| pids.contains(&x.0)) {
                x.2 = now.clone();
            }
            write_stream(&mut cf, path, &props_stream(&e))?;
        }
    }
    cf.flush().map_err(io_err)?;
    Ok(cf.into_inner().into_inner())
}

// ---------- Reading (P11.2) ----------

struct RObj {
    path: String,
    class: Uuid,
    props: Vec<(u16, u16, Vec<u8>)>,
}

impl RObj {
    fn get(&self, pid: u16) -> Option<&[u8]> {
        self.props.iter().find(|p| p.0 == pid).map(|p| p.2.as_slice())
    }
    fn i64(&self, pid: u16) -> Option<i64> {
        self.get(pid).filter(|b| b.len() >= 8).map(|b| i64::from_le_bytes(b[..8].try_into().unwrap_or([0; 8])))
    }
    fn u32(&self, pid: u16) -> Option<u32> {
        self.get(pid).filter(|b| b.len() >= 4).map(|b| u32::from_le_bytes(b[..4].try_into().unwrap_or([0; 4])))
    }
    fn string(&self, pid: u16) -> Option<String> {
        self.get(pid).map(from_utf16z)
    }
    fn rational(&self, pid: u16) -> Option<(i64, i64)> {
        self.get(pid).filter(|b| b.len() >= 8).map(|b| (i32::from_le_bytes(b[..4].try_into().unwrap_or([0; 4])) as i64, i32::from_le_bytes(b[4..8].try_into().unwrap_or([0; 4])) as i64))
    }
    fn is(&self, class: &str) -> bool {
        self.class == auid(class)
    }
}

fn read_obj(cf: &mut Cf, path: &str) -> Option<RObj> {
    let class = *cf.entry(path).ok()?.clsid();
    let props = parse_props(&read_stream(cf, &format!("{path}/properties"))?);
    Some(RObj { path: path.to_string(), class, props })
}

fn strong(cf: &mut Cf, o: &RObj, pid: u16) -> Option<RObj> {
    let name = from_utf16z(o.get(pid)?);
    read_obj(cf, &format!("{}/{name}", o.path))
}

fn vector(cf: &mut Cf, o: &RObj, pid: u16) -> Vec<RObj> {
    let Some(name) = o.get(pid).map(from_utf16z) else { return Vec::new() };
    let Some(idx) = read_stream(cf, &format!("{}/{name} index", o.path)) else { return Vec::new() };
    if idx.len() < 12 {
        return Vec::new();
    }
    let n = u32::from_le_bytes(idx[..4].try_into().unwrap_or([0; 4])) as usize;
    (0..n)
        .filter_map(|k| {
            let key = u32::from_le_bytes(idx.get(12 + 4 * k..16 + 4 * k)?.try_into().ok()?);
            read_obj(cf, &format!("{}/{name}{{{key:x}}}", o.path))
        })
        .collect()
}

/// Mobs of the ContentStorage with their MobIDs.
fn mobs(cf: &mut Cf) -> Vec<([u8; 32], RObj)> {
    let content = "/Header-2/Content-3b03";
    let Some(c) = read_obj(cf, content) else { return Vec::new() };
    let Some(name) = c.get(0x1901).map(from_utf16z) else { return Vec::new() };
    let Some(idx) = read_stream(cf, &format!("{content}/{name} index")) else { return Vec::new() };
    if idx.len() < 15 {
        return Vec::new();
    }
    let n = u32::from_le_bytes(idx[..4].try_into().unwrap_or([0; 4])) as usize;
    let key_size = idx[14] as usize;
    let mut out = Vec::new();
    for k in 0..n {
        let at = 15 + k * (8 + key_size);
        let Some(local) = idx.get(at..at + 4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes) else { break };
        if let Some(o) = read_obj(cf, &format!("{content}/{name}{{{local:x}}}")) {
            let mut id = [0u8; 32];
            if let Some(m) = o.get(0x4401) {
                let l = m.len().min(32);
                id[..l].copy_from_slice(&m[..l]);
            }
            out.push((id, o));
        }
    }
    out
}

/// Reads the top-level composition of an AAF file into a timeline.
pub fn import(bytes: &[u8]) -> Result<Timeline, String> {
    let mut cf = CompoundFile::open(Cursor::new(bytes.to_vec())).map_err(|e| format!("not an AAF file: {e}"))?;
    let all = mobs(&mut cf);
    let comp = all
        .iter()
        .filter(|(_, o)| o.is(CLS_COMPOSITION_MOB))
        // Prefer the top-level composition (usage code), else the first.
        .max_by_key(|(_, o)| o.get(0x4408).map(|u| u == auid("0d010102-0101-0700-060e-2b3404010101").to_bytes_le()).unwrap_or(false))
        .ok_or("the AAF has no composition")?;
    let name = comp.1.string(0x4402).unwrap_or_else(|| "Imported AAF".into());
    let slots = vector(&mut cf, &comp.1, 0x4403);
    // Resolve a master (or source) mob to media: file URL, rate, start TC, length.
    let mut media: Vec<MediaRef> = Vec::new();
    let mut media_by_mob: Vec<([u8; 32], usize)> = Vec::new();
    let resolve = |cf: &mut Cf, id: &[u8; 32], media: &mut Vec<MediaRef>, by: &mut Vec<([u8; 32], usize)>| -> Option<usize> {
        if let Some(i) = by.iter().find(|x| &x.0 == id).map(|x| x.1) {
            return Some(i);
        }
        let mut mob = all.iter().find(|(k, _)| k == id)?;
        let mut rate = clippro_types::FrameRate::default();
        // Follow MasterMob → SourceMob (with a descriptor).
        for _ in 0..4 {
            if mob.1.get(0x4701).is_some() {
                break;
            }
            let s = vector(cf, &mob.1, 0x4403);
            let pic = s.into_iter().find_map(|sl| strong(cf, &sl, 0x4803).filter(|seg| seg.is(CLS_SOURCE_CLIP)).map(|seg| (sl, seg)))?;
            if let Some((n, d)) = pic.0.rational(0x4b01) {
                rate = clippro_types::FrameRate::new(n, d.max(1));
            }
            let next: [u8; 32] = pic.1.get(0x1101)?.try_into().ok()?;
            mob = all.iter().find(|(k, _)| *k == next)?;
        }
        let desc = strong(cf, &mob.1, 0x4701);
        let url = desc.as_ref().and_then(|d| vector(cf, d, 0x2f01).into_iter().find_map(|l| l.string(0x4001))).unwrap_or_default();
        let (w, h) = desc.as_ref().map(|d| (d.u32(0x3203).unwrap_or(1920), d.u32(0x3202).unwrap_or(1080))).unwrap_or((1920, 1080));
        if let Some((n, d)) = desc.as_ref().and_then(|d| d.rational(0x3001)) {
            rate = clippro_types::FrameRate::new(n, d.max(1));
        }
        let mut start = 0;
        let mut drop = false;
        let mut length = desc.as_ref().and_then(|d| d.i64(0x3002)).unwrap_or(0);
        for sl in vector(cf, &mob.1, 0x4403) {
            if let Some(seg) = strong(cf, &sl, 0x4803) {
                if seg.is(CLS_TIMECODE) {
                    start = seg.i64(0x1501).unwrap_or(0);
                    drop = seg.get(0x1503).is_some_and(|b| b.first() == Some(&1));
                } else if length == 0 {
                    length = seg.i64(0x0202).unwrap_or(0);
                }
            }
        }
        let path = if url.is_empty() { std::path::PathBuf::new() } else { crate::import::url_to_path(&url) };
        let mname = mob.1.string(0x4402).unwrap_or_default();
        let m = MediaRef { id: media.len() as i64 + 1, name: mname, path, rate, duration_frames: length, start_tc_frames: start, drop_frame: drop, width: w, height: h, audio_channels: 0, reel: None };
        let i = crate::import::intern(media, m);
        by.push((*id, i));
        Some(i)
    };
    let mut rate = clippro_types::FrameRate::new(25, 1);
    let mut start_tc = 0;
    let mut drop = false;
    let mut tracks: Vec<(u32, Vec<(i64, ClipItem)>)> = Vec::new();
    let mut marks: Vec<(u32, i64, i64, String, MarkerColor)> = Vec::new();
    for sl in &slots {
        let Some(seg) = strong(&mut cf, sl, 0x4803) else { continue };
        let slot_id = sl.u32(0x4801).unwrap_or(0);
        if seg.is(CLS_TIMECODE) {
            start_tc = seg.i64(0x1501).unwrap_or(0);
            drop = seg.get(0x1503).is_some_and(|b| b.first() == Some(&1));
            continue;
        }
        if sl.is(CLS_EVENT_SLOT) {
            for m in vector(&mut cf, &seg, 0x1001) {
                let rgb = m.get(PID_MARKER_COLOR).filter(|b| b.len() >= 6).map(|b| [u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]), u16::from_le_bytes([b[4], b[5]])]);
                let on = m.u32(0x6102).unwrap_or(1);
                marks.push((on, m.i64(0x0601).unwrap_or(0), m.i64(0x0202).unwrap_or(1), m.string(0x0602).unwrap_or_default(), rgb.map(nearest_colour).unwrap_or(MarkerColor::Red)));
            }
            continue;
        }
        if let Some((n, d)) = sl.rational(0x4b01) {
            rate = clippro_types::FrameRate::new(n, d.max(1));
        }
        let comps = if seg.is(CLS_SEQUENCE) { vector(&mut cf, &seg, 0x1001) } else { vec![seg] };
        let mut at = 0i64;
        let mut clips = Vec::new();
        for c in comps {
            let len = c.i64(0x0202).unwrap_or(0);
            if c.is(CLS_SOURCE_CLIP) {
                let id: Option<[u8; 32]> = c.get(0x1101).and_then(|b| b.try_into().ok());
                if let Some(mi) = id.and_then(|id| resolve(&mut cf, &id, &mut media, &mut media_by_mob)) {
                    let m = &media[mi];
                    let src_in = convert_frames(c.i64(0x1201).unwrap_or(0), rate, m.rate);
                    let src_out = src_in + convert_frames(len, rate, m.rate).max(1);
                    clips.push((at, ClipItem { media: mi, name: m.name.clone(), src_in, src_out, markers: Vec::new(), select_index: None, score: None }));
                }
            }
            at += len;
        }
        tracks.push((slot_id, clips));
    }
    let mut tl = Timeline { name, rate, drop_frame: drop && rate.supports_drop_frame(), start_tc_frames: start_tc, width: 1920, height: 1080, media, video: Vec::new(), with_audio: false };
    if let Some(m) = tl.media.first() {
        (tl.width, tl.height) = (m.width, m.height);
    }
    // Markers sit on the clip under them, in that clip's source frames.
    for (slot, pos, len, comment, color) in marks {
        // The described slot, or the first picture track when it names none of them.
        let Some(ti) = tracks.iter().position(|t| t.0 == slot).or(if tracks.is_empty() { None } else { Some(0) }) else { continue };
        let t = &mut tracks[ti];
        let media = &tl.media;
        if let Some((rin, c)) = t.1.iter_mut().find(|(rin, c)| pos >= *rin && pos < *rin + convert_frames(c.len(), media[c.media].rate, rate)) {
            let mr = media[c.media].rate;
            let (name, note) = match comment.split_once(" – ") {
                Some((a, b)) => (a.to_string(), b.to_string()),
                None => (comment.clone(), String::new()),
            };
            c.markers.push(Marker { at: convert_frames(pos - *rin, rate, mr), duration: convert_frames(len, rate, mr), name, comment: note, color });
        }
    }
    let video: Vec<Track> = tracks.into_iter().map(|(id, c)| Track { name: format!("V{id}"), items: crate::import::place(&tl, c) }).collect();
    tl.video = crate::import::non_empty(video);
    Ok(tl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{media, selects};
    use crate::model::{build, ExportOptions, Layout};

    #[test]
    fn round_trip_through_our_reader() {
        for layout in [Layout::SelectsOnly, Layout::SelectsOverRejects] {
            let t = build(media(2), &selects(), &ExportOptions { layout, gap_s: 1.0, ..ExportOptions::default() });
            let bytes = export(&t).unwrap();
            assert!(bytes.len() > 100_000, "the template's MetaDictionary is in");
            let back = import(&bytes).unwrap();
            assert_eq!(crate::import::edit_signature(&back), crate::import::edit_signature(&t), "{layout:?}");
            assert_eq!((back.rate, back.start_tc_frames, back.name.as_str()), (t.rate, t.start_tc_frames, t.name.as_str()));
            for (a, b) in back.media.iter().zip(&t.media) {
                assert_eq!((a.start_tc_frames, a.duration_frames, a.rate, &a.path), (b.start_tc_frames, b.duration_frames, b.rate, &b.path));
            }
            let mk = |tl: &Timeline| -> Vec<(i64, String, MarkerColor)> {
                tl.video[0].items.iter().filter_map(|i| if let Item::Clip(c) = i { Some(c) } else { None }).flat_map(|c| c.markers.iter().map(move |m| (c.src_in + m.at, m.name.clone(), m.color))).collect()
            };
            assert_eq!(mk(&back), mk(&t));
        }
    }

    #[test]
    fn mob_ids_and_strings() {
        let a = mob_id("x", 1);
        assert_eq!(&a[..16], &MOBID_PREFIX);
        assert_ne!(a, mob_id("x", 2));
        assert_eq!(a, mob_id("x", 1));
        assert_eq!(from_utf16z(&utf16z("Café – 1")), "Café – 1");
        assert_eq!(nearest_colour([60000, 1000, 0]), MarkerColor::Red);
        assert!(import(b"not an aaf").is_err());
    }
}
