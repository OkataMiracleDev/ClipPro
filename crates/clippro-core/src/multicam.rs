//! Multicam sync groups (SPEC EXP-11, PLAN P11.5): sync clips by audio (ALGORITHMS §8.3) or
//! start timecode, keep the groups in the project, and lay a group out as a multicam timeline
//! (stacked tracks for XML/OTIO, an `mc-clip` for FCPXML).

use crate::session::{media_ref, Result, Session, SessionError};
use crate::store::lock;
use clippro_audio::sync::{sync_group, SyncOptions};
use clippro_interchange::model::{ClipItem, Item, Track};
use clippro_interchange::Timeline;
use clippro_media::MediaInfo;
use clippro_types::{ClipId, Timecode};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMethod {
    Audio,
    Timecode,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyncMember {
    pub clip: ClipId,
    pub name: String,
    /// Angle label: the camera model when known, else the clip name.
    pub angle: String,
    /// Start on the reference clip's clock (µs); `None` = not synced.
    pub offset_us: Option<i64>,
    /// Confidence of the audio match.
    pub psr: Option<f32>,
    /// Offset from start timecodes, when both clips have one.
    pub tc_offset_us: Option<i64>,
    /// |audio − timecode| when both exist.
    pub disagreement_us: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyncGroup {
    pub id: Option<i64>,
    pub name: String,
    pub method: SyncMethod,
    pub reference: ClipId,
    pub members: Vec<SyncMember>,
    pub warnings: Vec<String>,
}

struct Source {
    clip: ClipId,
    name: String,
    camera: Option<String>,
    path: PathBuf,
    info: MediaInfo,
    stream: Option<usize>,
}

/// Start timecode in µs since midnight, if the media has one.
fn tc_us(info: &MediaInfo) -> Option<i64> {
    let tc = info.start_timecode.as_deref()?;
    let rate = info.fps();
    let t = Timecode::parse(tc, rate.timecode_base()).ok()?;
    Some(rate.frames_to_us(t.frames))
}

impl Session {
    fn sources(&self, clips: &[ClipId]) -> Result<Vec<Source>> {
        let db = lock(&self.db);
        let rows = db.clips()?;
        let mut out = Vec::new();
        for &c in clips {
            let row = rows.iter().find(|r| r.id == c).ok_or_else(|| SessionError::Other("Unknown clip".into()))?;
            let m = self.clip_media(&db, c)?;
            let info: MediaInfo = serde_json::from_str(&m.probe_json).map_err(|e| SessionError::Other(e.to_string()))?;
            out.push(Source { clip: c, name: row.name.clone(), camera: info.camera_model.clone(), path: PathBuf::from(&m.path), info, stream: m.audio_stream.map(|s| s as usize) });
        }
        Ok(out)
    }

    /// Syncs `clips` (≥ 2) by audio or timecode. The reference is the longest clip (audio) or
    /// the earliest start timecode (timecode); both offsets are reported when available.
    pub fn sync_clips(&self, name: &str, clips: &[ClipId], method: SyncMethod, opts: &SyncOptions) -> Result<SyncGroup> {
        if clips.len() < 2 {
            return Err(SessionError::Other("Choose at least two clips to sync.".into()));
        }
        let src = self.sources(clips)?;
        let tcs: Vec<Option<i64>> = src.iter().map(|s| tc_us(&s.info)).collect();
        let mut warnings = Vec::new();
        let (reference, offsets, psr): (usize, Vec<Option<i64>>, Vec<Option<f32>>) = match method {
            SyncMethod::Audio => {
                let audio: Vec<Vec<f32>> = src.iter().map(|s| mono_48k(s).unwrap_or_default()).collect();
                for (s, a) in src.iter().zip(&audio) {
                    if a.is_empty() {
                        warnings.push(format!("{} has no audio to sync.", s.name));
                    }
                }
                let g = sync_group(&audio, opts);
                warnings.extend(g.warnings.iter().map(|w| name_clips(w, &src)));
                (g.reference, g.offsets.iter().map(|o| o.map(|s| (s * 1e6).round() as i64)).collect(), g.psr)
            }
            SyncMethod::Timecode => {
                let Some(r) = (0..src.len()).filter(|&i| tcs[i].is_some()).min_by_key(|&i| tcs[i]) else {
                    return Err(SessionError::Other("None of these clips has a start timecode.".into()));
                };
                let base = tcs[r].unwrap_or(0);
                for (s, t) in src.iter().zip(&tcs) {
                    if t.is_none() {
                        warnings.push(format!("{} has no start timecode.", s.name));
                    }
                }
                (r, tcs.iter().map(|t| t.map(|t| t - base)).collect(), vec![None; src.len()])
            }
        };
        // Timecode offsets on the reference's clock, and the audio/TC cross-check (> 1 frame).
        let ref_tc = tcs[reference];
        let frame_us = src[reference].info.fps().frame_duration_us().round() as i64;
        let members: Vec<SyncMember> = src
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let tc_offset_us = match (tcs[i], ref_tc) {
                    (Some(t), Some(r)) => Some(t - r),
                    _ => None,
                };
                let disagreement_us = match (method, offsets[i], tc_offset_us) {
                    (SyncMethod::Audio, Some(a), Some(t)) => Some((a - t).abs()),
                    _ => None,
                };
                if disagreement_us.is_some_and(|d| d > frame_us) && i != reference {
                    warnings.push(format!("{}: audio and timecode disagree by {} ms.", s.name, disagreement_us.unwrap_or(0) / 1000));
                }
                SyncMember {
                    clip: s.clip,
                    name: s.name.clone(),
                    angle: s.camera.clone().filter(|c| !c.is_empty()).unwrap_or_else(|| s.name.clone()),
                    offset_us: offsets[i],
                    psr: psr[i],
                    tc_offset_us,
                    disagreement_us,
                }
            })
            .collect();
        // Two clips from the same camera would share an angle name: number them.
        let mut members = members;
        for i in 0..members.len() {
            if members[..i].iter().any(|m| m.angle == members[i].angle) || members[i + 1..].iter().any(|m| m.angle == members[i].angle) {
                members[i].angle = format!("{} · {}", members[i].angle, members[i].name);
            }
        }
        Ok(SyncGroup { id: None, name: name.to_string(), method, reference: src[reference].clip, members, warnings })
    }

    pub fn sync_groups(&self) -> Result<Vec<SyncGroup>> {
        let rows = lock(&self.db).sync_groups()?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, _, _, json)| {
                let mut g: SyncGroup = serde_json::from_str(&json).ok()?;
                g.id = Some(id);
                Some(g)
            })
            .collect())
    }

    /// Saves (or replaces, when it has an id) a sync group; returns its id.
    pub fn save_sync_group(&self, g: &SyncGroup) -> Result<i64> {
        let json = serde_json::to_string(g).map_err(|e| SessionError::Other(e.to_string()))?;
        let method = if g.method == SyncMethod::Audio { "audio" } else { "timecode" };
        Ok(lock(&self.db).save_sync_group(g.id, &g.name, method, &json)?)
    }

    pub fn delete_sync_group(&self, id: i64) -> Result<()> {
        Ok(lock(&self.db).delete_sync_group(id)?)
    }

    /// The group as stacked tracks, one angle per track (V1 = the reference), whole clips at
    /// their synced positions. Unsynced members are left out.
    pub fn multicam_timeline(&self, g: &SyncGroup) -> Result<Timeline> {
        let synced: Vec<&SyncMember> = g.members.iter().filter(|m| m.offset_us.is_some()).collect();
        let mut order: Vec<&SyncMember> = synced.iter().copied().filter(|m| m.clip == g.reference).collect();
        order.extend(synced.iter().copied().filter(|m| m.clip != g.reference));
        let src = self.sources(&order.iter().map(|m| m.clip).collect::<Vec<_>>())?;
        let first = order.iter().filter_map(|m| m.offset_us).min().unwrap_or(0);
        let media: Vec<_> = src.iter().enumerate().map(|(i, s)| media_ref(&s.info, i as i64 + 1, &s.name, s.path.clone())).collect();
        let rate = media.first().map(|m| m.rate).unwrap_or_default();
        let mut video = Vec::new();
        for (i, (m, mr)) in order.iter().zip(&media).enumerate() {
            let at_us = m.offset_us.unwrap_or(0) - first;
            let at = rate.us_to_frames_round(at_us);
            let mut items = Vec::new();
            if at > 0 {
                items.push(Item::Gap(at));
            }
            items.push(Item::Clip(ClipItem { media: i, name: m.name.clone(), src_in: 0, src_out: mr.duration_frames.max(1), markers: Vec::new(), select_index: None, score: None }));
            video.push(Track { name: m.angle.clone(), items });
        }
        let drop = media.first().is_some_and(|m| m.drop_frame) && rate.supports_drop_frame();
        // Record start: the reference's start timecode, moved back to the earliest angle.
        let start_tc_frames = media.first().map(|m| m.start_tc_frames + rate.us_to_frames_round(first.min(0))).filter(|f| *f >= 0).unwrap_or(0);
        let (width, height) = media.first().map(|m| (m.width, m.height)).unwrap_or((1920, 1080));
        Ok(Timeline { name: g.name.clone(), rate, drop_frame: drop, start_tc_frames, width, height, media, video, with_audio: true })
    }
}

/// Mono mix of a clip's audio at 48 kHz.
fn mono_48k(s: &Source) -> Option<Vec<f32>> {
    let mut out: Vec<f32> = Vec::new();
    clippro_media::audio::decode_audio(&s.path, s.stream, s.info.start_us, |block| {
        let n = block.first().map(Vec::len).unwrap_or(0);
        let k = block.len().max(1) as f32;
        out.extend((0..n).map(|i| block.iter().map(|c| c[i]).sum::<f32>() / k));
        true
    })
    .ok()?;
    Some(out)
}

/// "Clip 2 …" → the clip's name.
fn name_clips(w: &str, src: &[Source]) -> String {
    let mut s = w.to_string();
    for (i, x) in src.iter().enumerate().rev() {
        s = s.replace(&format!("Clip {}", i + 1), &x.name).replace(&format!("Clips {}", i + 1), &x.name).replace(&format!("and {}", i + 1), &format!("and {}", x.name));
    }
    s
}
