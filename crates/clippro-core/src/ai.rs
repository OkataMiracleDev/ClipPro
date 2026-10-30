//! The AI pass and its results in the project (SPEC §18, PLAN P12): per clip, faces and CLIP at
//! 2 Hz from a colour decode, a Whisper transcript and speaker turns from the audio; then People
//! across the project, Content sub-metrics and flags for scoring, sentence snap points,
//! transcript and visual search, take grouping and best-take filtering.
//!
//! AI never changes the Focus/Exposure/Motion/Audio scores; it only adds the Content group,
//! its flags (No face, Flub), snapping and filters.

use crate::session::{Result, Session, SessionError};
use crate::store::lock;
use clippro_ai::clip::{fuse_face_size, Clip, Scene, ShotType};
use clippro_ai::faces::{cluster, FaceEngine, CLUSTER_DISTANCE};
use clippro_ai::speakers::{SpeakerEngine, Turn};
use clippro_media::MediaInfo;
use clippro_score::{Selects, Settings, SubMetric};
use clippro_transcribe::Transcript;
use clippro_types::{ClipId, FlagKind, MediaId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// AI sample interval (2 Hz).
pub const STEP_US: i64 = 500_000;
const DECODE_WIDTH: u32 = 640;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiOptions {
    pub faces: bool,
    pub clip: bool,
    pub transcripts: bool,
    pub speakers: bool,
    /// Whisper model id (models.json), e.g. "whisper-small".
    pub whisper: String,
    /// Transcript language; None detects it.
    pub language: Option<String>,
    /// CPU only, repeatable results.
    pub deterministic: bool,
}

impl Default for AiOptions {
    fn default() -> Self {
        AiOptions { faces: false, clip: false, transcripts: false, speakers: false, whisper: "whisper-small".into(), language: None, deterministic: false }
    }
}

impl AiOptions {
    pub fn any(&self) -> bool {
        self.faces || self.clip || self.transcripts || self.speakers
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceLite {
    pub bbox: [f32; 4],
    pub score: f32,
    pub sharpness: f32,
    pub size: f32,
    /// Index into the media's face tracks.
    pub track: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VisionSample {
    pub t_us: i64,
    /// None when the faces module didn't run.
    pub faces: Option<Vec<FaceLite>>,
    pub scene: Option<Scene>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceTrackData {
    /// (sample time µs, bbox) per appearance.
    pub appearances: Vec<(i64, [f32; 4])>,
    pub embedding: Vec<f32>,
}

/// Everything the AI pass stored for one media file.
#[derive(Clone, Debug, Default)]
pub struct AiData {
    pub vision: Vec<VisionSample>,
    /// CLIP image embeddings, one per vision sample (empty if CLIP didn't run).
    pub embeddings: Vec<Vec<f32>>,
    pub transcript: Option<Transcript>,
    pub speakers: Vec<Turn>,
    /// Person id per face track (in track order).
    pub track_people: Vec<Option<i64>>,
    /// Changes whenever any of the above does (scoring cache key).
    pub version: u64,
}

impl AiData {
    pub fn is_empty(&self) -> bool {
        self.vision.is_empty() && self.transcript.is_none() && self.speakers.is_empty()
    }

    /// Nearest vision sample to `t` within half a step.
    fn sample_at(&self, t: i64) -> Option<usize> {
        let i = self.vision.partition_point(|s| s.t_us < t);
        [i.checked_sub(1), Some(i)].into_iter().flatten().filter(|&k| k < self.vision.len()).min_by_key(|&k| (self.vision[k].t_us - t).abs()).filter(|&k| (self.vision[k].t_us - t).abs() <= STEP_US / 2 + 1)
    }

    /// People seen in [a, b).
    pub fn people_in(&self, a: i64, b: i64) -> Vec<i64> {
        let mut v: Vec<i64> = self
            .vision
            .iter()
            .filter(|s| s.t_us >= a && s.t_us < b)
            .flat_map(|s| s.faces.iter().flatten())
            .filter_map(|f| f.track.and_then(|t| self.track_people.get(t).copied().flatten()))
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// The most likely shot type over [a, b) (mode of per-sample argmax).
    pub fn shot_in(&self, a: i64, b: i64) -> Option<(ShotType, f32)> {
        let mut counts = [0usize; 7];
        let mut n = 0;
        for s in self.vision.iter().filter(|s| s.t_us >= a && s.t_us < b) {
            let Some(sc) = &s.scene else { continue };
            let largest = s.faces.iter().flatten().map(|f| f.size).fold(0f32, f32::max);
            let p = fuse_face_size(&sc.shot, largest);
            if let Some(k) = (0..p.len()).max_by(|&i, &j| p[i].total_cmp(&p[j])) {
                counts[k] += 1;
                n += 1;
            }
        }
        let k = (0..7).max_by_key(|&k| counts[k])?;
        (n > 0).then(|| (ShotType::ALL[k], counts[k] as f32 / n as f32))
    }

    /// Mean CLIP embedding over [a, b), unit length.
    pub fn embedding_in(&self, a: i64, b: i64) -> Option<Vec<f32>> {
        if self.embeddings.is_empty() {
            return None;
        }
        let mut m = vec![0f32; self.embeddings[0].len()];
        let mut n = 0;
        for (s, e) in self.vision.iter().zip(&self.embeddings) {
            if s.t_us >= a && s.t_us < b {
                m.iter_mut().zip(e).for_each(|(x, y)| *x += y);
                n += 1;
            }
        }
        if n == 0 {
            return None;
        }
        let z = m.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
        m.iter_mut().for_each(|x| *x /= z);
        Some(m)
    }
}

fn encode_embeddings(e: &[Vec<f32>]) -> Vec<u8> {
    let d = e.first().map(Vec::len).unwrap_or(0) as u32;
    let mut v = Vec::with_capacity(8 + e.len() * d as usize * 4);
    v.extend((e.len() as u32).to_le_bytes());
    v.extend(d.to_le_bytes());
    for x in e.iter().flatten() {
        v.extend(x.to_le_bytes());
    }
    v
}

fn decode_embeddings(b: &[u8]) -> Vec<Vec<f32>> {
    if b.len() < 8 {
        return Vec::new();
    }
    let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
    let d = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
    if b.len() < 8 + n * d * 4 || d == 0 {
        return Vec::new();
    }
    b[8..8 + n * d * 4].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect::<Vec<_>>().chunks(d).map(<[f32]>::to_vec).collect()
}

fn model_hash(id: &str) -> String {
    clippro_ai::models::model(id).map(|m| m.sha256).unwrap_or_default()
}

/// Stages reported by `run_ai`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiStage {
    Vision,
    Transcript,
    Speakers,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PersonInfo {
    pub id: i64,
    pub name: String,
    pub clips: Vec<ClipId>,
    /// A frame to show: (clip, time µs, bbox in 640-wide decode pixels).
    pub cover: Option<(ClipId, i64, [f32; 4])>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub clip: ClipId,
    pub start_us: i64,
    pub end_us: i64,
    pub text: String,
    pub score: f32,
}

/// Per-clip AI summary for filters and grouping (TML-5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClipAi {
    pub clip: ClipId,
    pub people: Vec<i64>,
    pub shot: Option<ShotType>,
    pub transcript: bool,
    pub language: Option<String>,
    pub speakers: usize,
    pub faces_done: bool,
    pub clip_done: bool,
}

fn word_set(t: &str) -> Vec<String> {
    t.split_whitespace().map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()).filter(|w| !w.is_empty()).collect()
}

/// Jaccard similarity of word 3-grams (ALGORITHMS §8.4 J3).
pub fn j3(a: &str, b: &str) -> Option<f32> {
    let grams = |t: &str| -> std::collections::HashSet<String> { word_set(t).windows(3).map(|w| w.join(" ")).collect() };
    let (x, y) = (grams(a), grams(b));
    if x.is_empty() || y.is_empty() {
        return None;
    }
    let inter = x.intersection(&y).count() as f32;
    Some(inter / x.union(&y).count() as f32)
}

/// Take similarity, full variant (ALGORITHMS §8.4): 0.5·cos(CLIP) + 0.3·J3(text) +
/// 0.2·(1 − hamming/64), dropping missing terms and renormalising.
pub fn take_similarity(clip: Option<f32>, text: Option<f32>, phash_hamming: Option<u32>) -> f32 {
    let terms = [(0.5, clip), (0.3, text), (0.2, phash_hamming.map(|h| 1.0 - h as f32 / 64.0))];
    let (mut s, mut w) = (0.0, 0.0);
    for (wt, v) in terms {
        if let Some(v) = v {
            s += wt * v;
            w += wt;
        }
    }
    if w > 0.0 { s / w } else { 0.0 }
}

impl Session {
    fn media_of(&self, clip: ClipId) -> Result<(MediaId, PathBuf, MediaInfo)> {
        let db = lock(&self.db);
        let m = self.clip_media(&db, clip)?;
        let info: MediaInfo = serde_json::from_str(&m.probe_json).map_err(|e| SessionError::Other(e.to_string()))?;
        Ok((m.id, PathBuf::from(&m.path), info))
    }

    /// Analysed clips that still need a module `opts` asks for.
    pub fn ai_pending(&self, opts: &AiOptions) -> Result<Vec<ClipId>> {
        let db = lock(&self.db);
        let have = |m: &str| db.ai_media(m).unwrap_or_default();
        let (vision, emb, speakers) = (have("vision"), have("clip_emb"), have("speakers"));
        let mut out = Vec::new();
        let statuses = {
            drop(db);
            self.clip_statuses()?
        };
        let db = lock(&self.db);
        for c in statuses.into_iter().filter(|c| c.state == clippro_project::RunState::Done && !c.missing) {
            let m = c.media;
            let need_faces = opts.faces && !vision.contains(&m);
            let need_clip = opts.clip && !emb.contains(&m);
            let need_tr = opts.transcripts && db.transcript(m).ok().flatten().is_none();
            let need_sp = opts.speakers && !speakers.contains(&m);
            if need_faces || need_clip || need_tr || need_sp {
                out.push(c.clip);
            }
        }
        Ok(out)
    }

    /// Runs the requested modules on one clip and stores the results. `progress(stage, 0–1)`.
    pub fn run_ai(&self, clip: ClipId, opts: &AiOptions, cancel: &AtomicBool, mut progress: impl FnMut(AiStage, f32)) -> Result<()> {
        let (media, path, info) = self.media_of(clip)?;
        let err = SessionError::Other;
        let cancelled = || cancel.load(std::sync::atomic::Ordering::Relaxed);
        if opts.faces || opts.clip {
            // Colour frames at 2 Hz from the proxy when there is one.
            let (vpath, vinfo) = self.preview_source(media)?.unwrap_or((path.clone(), info.clone()));
            let mut faces = if opts.faces { Some(FaceEngine::load(opts.deterministic).map_err(err)?) } else { None };
            let mut clipm = if opts.clip { Some(Clip::load(opts.deterministic).map_err(err)?) } else { None };
            let mut dec = clippro_media::VideoDecoder::open(&vpath, &vinfo, &self.opts.analyze.decode).map_err(|e| SessionError::Other(e.to_string()))?;
            let n = ((vinfo.duration_us.max(1) + STEP_US - 1) / STEP_US) as usize;
            let mut samples: Vec<VisionSample> = Vec::with_capacity(n);
            let mut all_faces: Vec<Vec<clippro_ai::faces::Face>> = Vec::with_capacity(n);
            let mut embeddings: Vec<Vec<f32>> = Vec::new();
            let mut batch: Vec<Vec<f32>> = Vec::new();
            for k in 0..n {
                if cancelled() {
                    return Err(SessionError::Other("cancelled".into()));
                }
                let t = (k as i64 * STEP_US).min(vinfo.duration_us.saturating_sub(1).max(0));
                let Some(f) = dec.frame_near(t).map_err(|e| SessionError::Other(e.to_string()))? else { break };
                let (w, h, rgb) = dec.to_preview(&f, DECODE_WIDTH, clippro_media::video::PreviewFormat::Rgb24).map_err(|e| SessionError::Other(e.to_string()))?;
                let (w, h) = (w as usize, h as usize);
                let fs = match faces.as_mut() {
                    Some(e) => Some(e.detect(&rgb, w, h).map_err(err)?),
                    None => None,
                };
                if clipm.is_some() {
                    batch.push(clippro_ai::clip::preprocess(&rgb, w, h));
                    if batch.len() == 8 {
                        embeddings.extend(clipm.as_mut().map(|c| c.embed_images(&batch)).transpose().map_err(err)?.unwrap_or_default());
                        batch.clear();
                    }
                }
                samples.push(VisionSample {
                    t_us: t,
                    faces: fs.as_ref().map(|v| v.iter().map(|x| FaceLite { bbox: x.bbox, score: x.score, sharpness: x.sharpness, size: x.size, track: None }).collect()),
                    scene: None,
                });
                all_faces.push(fs.unwrap_or_default());
                progress(AiStage::Vision, (k + 1) as f32 / n as f32);
            }
            if let Some(c) = clipm.as_mut() {
                if !batch.is_empty() {
                    embeddings.extend(c.embed_images(&batch).map_err(err)?);
                }
                for (s, e) in samples.iter_mut().zip(&embeddings) {
                    s.scene = Some(c.scene(e));
                }
            }
            if opts.faces {
                let tracks = clippro_ai::faces::tracks(&all_faces);
                let mut rows = Vec::new();
                for (ti, t) in tracks.iter().enumerate() {
                    for &(s, f) in &t.faces {
                        if let Some(x) = samples[s].faces.as_mut().and_then(|v| v.get_mut(f)) {
                            x.track = Some(ti);
                        }
                    }
                    let data = FaceTrackData { appearances: t.faces.iter().map(|&(s, f)| (samples[s].t_us, all_faces[s][f].bbox)).collect(), embedding: t.embedding.clone() };
                    rows.push(serde_json::to_vec(&data).unwrap_or_default());
                }
                lock(&self.db).set_face_tracks(media, &rows)?;
            }
            let db = lock(&self.db);
            let json = serde_json::to_vec(&samples).unwrap_or_default();
            let ids = [opts.faces.then_some("yunet-2023mar+sface-2021dec"), opts.clip.then_some("clip-b32-vision")].into_iter().flatten().collect::<Vec<_>>().join("+");
            db.save_ai(media, "vision", &ids, &model_hash("clip-b32-vision"), &json)?;
            if opts.clip {
                db.save_ai(media, "clip_emb", "clip-b32-vision", &model_hash("clip-b32-vision"), &encode_embeddings(&embeddings))?;
            }
        }
        let mut transcript: Option<Transcript> = None;
        if opts.transcripts || opts.speakers {
            let audio = mono_16k(&path, &info, self.opts.analyze.audio_stream)?;
            if opts.transcripts && !audio.is_empty() {
                progress(AiStage::Transcript, 0.0);
                let model = clippro_ai::models::installed_path(&opts.whisper).ok_or_else(|| SessionError::Other(format!("the {} model isn't installed", opts.whisper)))?;
                let threads = std::thread::available_parallelism().map(|n| (n.get() / 2).max(1)).unwrap_or(2);
                let t = clippro_transcribe::transcribe(&model, &audio, opts.language.as_deref(), threads, Arc::new(AtomicBool::new(cancelled()))).map_err(err)?;
                let segs: Vec<(i64, String)> = t.segments.iter().map(|s| (s.start_us, s.text.clone())).collect();
                lock(&self.db).save_transcript(media, &t.language, &serde_json::to_vec(&t).unwrap_or_default(), &segs)?;
                transcript = Some(t);
                progress(AiStage::Transcript, 1.0);
            }
            if opts.speakers && !audio.is_empty() {
                progress(AiStage::Speakers, 0.0);
                let t = match transcript.take() {
                    Some(t) => Some(t),
                    None => lock(&self.db).transcript(media)?.and_then(|(_, d)| serde_json::from_slice::<Transcript>(&d).ok()),
                };
                // Speech regions: transcript words merged across short gaps, else everything.
                let mut speech: Vec<(i64, i64)> = Vec::new();
                for w in t.iter().flat_map(|t| &t.words) {
                    match speech.last_mut() {
                        Some(s) if w.start_us - s.1 < 600_000 => s.1 = s.1.max(w.end_us),
                        _ => speech.push((w.start_us, w.end_us)),
                    }
                }
                if speech.is_empty() {
                    speech.push((0, audio.len() as i64 * 1_000_000 / 16_000));
                }
                let mut eng = SpeakerEngine::load(opts.deterministic).map_err(err)?;
                let turns = eng.turns(&audio, &speech).map_err(err)?;
                lock(&self.db).save_ai(media, "speakers", "ecapa-voxceleb-512", &model_hash("ecapa-voxceleb-512"), &serde_json::to_vec(&turns).unwrap_or_default())?;
                progress(AiStage::Speakers, 1.0);
            }
        }
        lock(&self.ai_cache).remove(&media.0);
        if opts.faces {
            self.recluster_people()?;
        }
        Ok(())
    }

    /// The stored AI results of a media file (cached).
    pub fn ai_data(&self, media: MediaId) -> Result<Arc<AiData>> {
        if let Some(d) = lock(&self.ai_cache).get(&media.0) {
            return Ok(d.clone());
        }
        let db = lock(&self.db);
        let mut d = AiData::default();
        if let Some((_, b)) = db.ai(media, "vision")? {
            d.vision = serde_json::from_slice(&b).unwrap_or_default();
        }
        if let Some((_, b)) = db.ai(media, "clip_emb")? {
            d.embeddings = decode_embeddings(&b);
        }
        if let Some((_, b)) = db.transcript(media)? {
            d.transcript = serde_json::from_slice(&b).ok();
        }
        if let Some((_, b)) = db.ai(media, "speakers")? {
            d.speakers = serde_json::from_slice(&b).unwrap_or_default();
        }
        d.track_people = db.face_tracks()?.into_iter().filter(|t| t.1 == media).map(|t| t.2).collect();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&(d.vision.len(), d.embeddings.len(), d.speakers.len(), &d.track_people), &mut h);
        if let Some(t) = &d.transcript {
            std::hash::Hash::hash(&(t.words.len(), t.words.first().map(|w| w.start_us), t.words.last().map(|w| w.end_us)), &mut h);
        }
        d.version = std::hash::Hasher::finish(&h);
        let d = Arc::new(d);
        lock(&self.ai_cache).insert(media.0, d.clone());
        Ok(d)
    }

    /// Re-clusters every face track in the project into People, keeping existing people (and
    /// their names) by matching cluster centroids to their stored embeddings.
    pub fn recluster_people(&self) -> Result<()> {
        let db = lock(&self.db);
        let tracks = db.face_tracks()?;
        let data: Vec<FaceTrackData> = tracks.iter().map(|t| serde_json::from_slice(&t.3).unwrap_or(FaceTrackData { appearances: Vec::new(), embedding: Vec::new() })).collect();
        let embs: Vec<Vec<f32>> = data.iter().map(|d| d.embedding.clone()).collect();
        let labels = cluster(&embs, CLUSTER_DISTANCE);
        let people = db.people()?;
        let pemb: Vec<(i64, String, Vec<f32>)> = people.iter().map(|p| (p.0, p.1.clone(), p.3.as_deref().map(|b| b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()).unwrap_or_default())).collect();
        let n_clusters = labels.iter().max().map(|m| m + 1).unwrap_or(0);
        let mut used: Vec<i64> = Vec::new();
        let mut next_name = people.len() + 1;
        for c in 0..n_clusters {
            let members: Vec<usize> = (0..labels.len()).filter(|&i| labels[i] == c).collect();
            let d = embs.first().map(Vec::len).unwrap_or(0);
            let mut cen = vec![0f32; d];
            for &i in &members {
                cen.iter_mut().zip(&embs[i]).for_each(|(a, b)| *a += b);
            }
            let z = cen.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
            cen.iter_mut().for_each(|x| *x /= z);
            let bytes: Vec<u8> = cen.iter().flat_map(|x| x.to_le_bytes()).collect();
            let cover = members.iter().max_by(|&&a, &&b| data[a].appearances.len().cmp(&data[b].appearances.len())).map(|&i| format!("track:{}", tracks[i].0));
            let existing = pemb
                .iter()
                .filter(|p| !used.contains(&p.0) && p.2.len() == d)
                .map(|p| (p.0, p.1.clone(), 1.0 - p.2.iter().zip(&cen).map(|(a, b)| a * b).sum::<f32>()))
                .filter(|p| p.2 <= CLUSTER_DISTANCE)
                .min_by(|a, b| a.2.total_cmp(&b.2));
            let pid = match existing {
                Some((id, name, _)) => {
                    db.update_person(id, &name, cover.as_deref(), &bytes)?;
                    id
                }
                None => {
                    let id = db.add_person(&format!("Person {next_name}"), cover.as_deref(), &bytes)?;
                    next_name += 1;
                    id
                }
            };
            used.push(pid);
            for &i in &members {
                db.set_track_person(tracks[i].0, Some(pid))?;
            }
        }
        // Unnamed people with no faces left are removed; named ones stay.
        for p in people.iter().filter(|p| !used.contains(&p.0) && p.1.starts_with("Person ")) {
            db.delete_person(p.0)?;
        }
        drop(db);
        lock(&self.ai_cache).clear();
        Ok(())
    }

    pub fn people(&self) -> Result<Vec<PersonInfo>> {
        let statuses = self.clip_statuses()?;
        let db = lock(&self.db);
        let tracks = db.face_tracks()?;
        let mut out: Vec<PersonInfo> = db.people()?.into_iter().map(|p| PersonInfo { id: p.0, name: p.1, clips: Vec::new(), cover: None }).collect();
        for p in &mut out {
            let mine: Vec<&(i64, MediaId, Option<i64>, Vec<u8>)> = tracks.iter().filter(|t| t.2 == Some(p.id)).collect();
            for t in &mine {
                for c in statuses.iter().filter(|c| c.media == t.1) {
                    if !p.clips.contains(&c.clip) {
                        p.clips.push(c.clip);
                    }
                }
            }
            if let Some(best) = mine.iter().filter_map(|t| serde_json::from_slice::<FaceTrackData>(&t.3).ok().map(|d| (t.1, d))).max_by_key(|(_, d)| d.appearances.len()) {
                if let (Some(c), Some(a)) = (statuses.iter().find(|c| c.media == best.0), best.1.appearances.get(best.1.appearances.len() / 2)) {
                    p.cover = Some((c.clip, a.0, a.1));
                }
            }
        }
        out.retain(|p| !p.clips.is_empty() || !p.name.starts_with("Person "));
        Ok(out)
    }

    pub fn rename_person(&self, id: i64, name: &str) -> Result<()> {
        lock(&self.db).rename_person(id, name.trim())?;
        Ok(())
    }

    /// Moves every face of `from` to `into` and removes `from`.
    pub fn merge_people(&self, into: i64, from: i64) -> Result<()> {
        let db = lock(&self.db);
        for t in db.face_tracks()?.into_iter().filter(|t| t.2 == Some(from)) {
            db.set_track_person(t.0, Some(into))?;
        }
        db.delete_person(from)?;
        drop(db);
        lock(&self.ai_cache).clear();
        Ok(())
    }

    /// Per-clip AI summary.
    pub fn clip_ai(&self) -> Result<Vec<ClipAi>> {
        let statuses = self.clip_statuses()?;
        let mut out = Vec::new();
        for c in statuses {
            let d = self.ai_data(c.media)?;
            let end = i64::MAX;
            out.push(ClipAi {
                clip: c.clip,
                people: d.people_in(0, end),
                shot: d.shot_in(0, end).map(|s| s.0),
                transcript: d.transcript.is_some(),
                language: d.transcript.as_ref().map(|t| t.language.clone()).filter(|l| !l.is_empty()),
                speakers: d.speakers.iter().map(|t| t.speaker).collect::<std::collections::HashSet<_>>().len(),
                faces_done: d.vision.iter().any(|s| s.faces.is_some()),
                clip_done: !d.embeddings.is_empty(),
            });
        }
        Ok(out)
    }

    /// Full-text transcript search.
    pub fn search_transcripts(&self, query: &str) -> Result<Vec<SearchHit>> {
        let statuses = self.clip_statuses()?;
        let hits = lock(&self.db).search_transcripts(query, 200)?;
        let mut out = Vec::new();
        for (m, start, text) in hits {
            let d = self.ai_data(m)?;
            let end = d.transcript.as_ref().and_then(|t| t.segments.iter().find(|s| s.start_us == start)).map(|s| s.end_us).unwrap_or(start + 3_000_000);
            for c in statuses.iter().filter(|c| c.media == m) {
                out.push(SearchHit { clip: c.clip, start_us: start, end_us: end, text: text.clone(), score: 1.0 });
            }
        }
        Ok(out)
    }

    /// Semantic visual search (AI-5): CLIP text vs stored image embeddings, best moments first.
    pub fn search_visual(&self, query: &str, deterministic: bool, limit: usize) -> Result<Vec<SearchHit>> {
        let q = {
            let mut g = lock(&self.clip_model);
            if g.is_none() {
                *g = Some(Clip::load(deterministic).map_err(SessionError::Other)?);
            }
            g.as_mut().map(|c| c.embed_texts(&[query])).transpose().map_err(SessionError::Other)?.and_then(|v| v.into_iter().next()).unwrap_or_default()
        };
        let mut hits = Vec::new();
        for c in self.clip_statuses()? {
            let d = self.ai_data(c.media)?;
            // Runs of consecutive samples, scored by their best match.
            let sims: Vec<f32> = d.embeddings.iter().map(|e| clippro_ai::clip::cosine(e, &q)).collect();
            let Some(best) = sims.iter().cloned().fold(None, |m: Option<f32>, x| Some(m.map_or(x, |m| m.max(x)))) else { continue };
            let cut = best - 0.02;
            let mut k = 0;
            while k < sims.len() {
                if sims[k] >= cut {
                    let a = k;
                    while k < sims.len() && sims[k] >= cut {
                        k += 1;
                    }
                    let s = sims[a..k].iter().cloned().fold(f32::MIN, f32::max);
                    hits.push(SearchHit { clip: c.clip, start_us: d.vision[a].t_us, end_us: d.vision[k - 1].t_us + STEP_US, text: c.name.clone(), score: s });
                }
                k += 1;
            }
        }
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        hits.truncate(limit);
        Ok(hits)
    }

    /// Content columns, AI flags and snap points for a clip, aligned to its samples.
    pub(crate) fn ai_inputs(&self, media: MediaId, time_us: &[i64], shot_types: &[String]) -> (Vec<(SubMetric, Vec<f32>)>, Option<Vec<u32>>, Option<(Vec<i64>, Vec<i64>)>) {
        let Ok(d) = self.ai_data(media) else { return (Vec::new(), None, None) };
        if d.is_empty() {
            return (Vec::new(), None, None);
        }
        let mut cols = Vec::new();
        let n = time_us.len();
        let idx: Vec<Option<usize>> = time_us.iter().map(|t| d.sample_at(*t)).collect();
        let f = &self.cal.focus;
        let faces_ran = d.vision.iter().any(|s| s.faces.is_some());
        let mut flags = vec![0u32; n];
        if faces_ran {
            let mut focus = vec![f32::NAN; n];
            let mut size = vec![f32::NAN; n];
            for (k, i) in idx.iter().enumerate() {
                let Some(fs) = i.and_then(|i| d.vision[i].faces.as_ref()) else { continue };
                if fs.is_empty() {
                    flags[k] |= FlagKind::NoFace.bit();
                    continue;
                }
                let best = fs.iter().map(|x| 1.0 / (1.0 + (-((x.sharpness - (f.sharp_s0 + 0.3)) / f.sharp_k)).exp())).fold(0f32, f32::max);
                focus[k] = best;
                let s = fs.iter().map(|x| x.size).fold(0f32, f32::max);
                let t = ((s - 0.005) / (0.15 - 0.005)).clamp(0.0, 1.0);
                size[k] = t * t * (3.0 - 2.0 * t);
            }
            cols.push((SubMetric::FaceFocus, focus));
            cols.push((SubMetric::FaceSize, size));
        }
        if d.vision.iter().any(|s| s.scene.is_some()) {
            let want: Vec<usize> = shot_types.iter().filter_map(|s| ShotType::ALL.iter().position(|t| format!("{t:?}").eq_ignore_ascii_case(&s.replace(['_', '-', ' '], "")) || t.label().eq_ignore_ascii_case(s))).collect();
            let mut aes = vec![f32::NAN; n];
            let mut shot = vec![f32::NAN; n];
            for (k, i) in idx.iter().enumerate() {
                let Some(s) = i.map(|i| &d.vision[i]) else { continue };
                let Some(sc) = &s.scene else { continue };
                let t = ((sc.aesthetic - 3.5) / 3.5).clamp(0.0, 1.0);
                aes[k] = t * t * (3.0 - 2.0 * t);
                if !want.is_empty() {
                    let largest = s.faces.iter().flatten().map(|x| x.size).fold(0f32, f32::max);
                    let p = fuse_face_size(&sc.shot, largest);
                    shot[k] = want.iter().map(|&w| p[w]).sum::<f32>().min(1.0);
                }
            }
            cols.push((SubMetric::Aesthetic, aes));
            if !want.is_empty() {
                cols.push((SubMetric::ShotTypeMatch, shot));
            }
        }
        let mut snaps = None;
        if let Some(t) = &d.transcript {
            let half = time_us.windows(2).map(|w| (w[1] - w[0]) / 2).next().unwrap_or(125_000);
            for (a, b) in clippro_transcribe::flubs(t) {
                for (k, tk) in time_us.iter().enumerate() {
                    if *tk + half > a && *tk - half < b {
                        flags[k] |= FlagKind::Flub.bit();
                    }
                }
            }
            snaps = Some(clippro_transcribe::boundaries(t));
        }
        let any_flag = flags.iter().any(|f| *f != 0);
        (cols, any_flag.then_some(flags), snaps)
    }

    /// Take groups over the current selects (ALGORITHMS §8.4, full variant when CLIP or
    /// transcripts are present): (clip index, segment index) → group number.
    pub fn take_groups(&self, sel: &Selects) -> Result<Vec<((usize, usize), usize)>> {
        let statuses = self.clip_statuses()?;
        let inputs = self.clip_inputs()?;
        let mut items: Vec<((usize, usize), ClipId, i64, i64, Option<Vec<f32>>, Option<String>, Option<u64>, f32)> = Vec::new();
        for (ci, c) in sel.clips.iter().enumerate() {
            let Some(st) = statuses.iter().find(|s| s.clip == c.clip) else { continue };
            let d = self.ai_data(st.media)?;
            let ms = inputs.iter().find(|i| i.id == c.clip).map(|i| i.ms.clone());
            for (si, s) in c.segments.iter().enumerate() {
                let text = d.transcript.as_ref().map(|t| t.words_in(s.start_us, s.end_us).map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")).filter(|t| !t.is_empty());
                let ph = ms.as_ref().and_then(|m| m.phash.get((s.first_sample + s.last_sample) / 2).copied());
                items.push(((ci, si), c.clip, s.start_us, s.end_us, d.embedding_in(s.start_us, s.end_us), text, ph, s.mean));
            }
        }
        let n = items.len();
        let sim = |i: usize, j: usize| -> Option<f32> {
            let (a, b) = (&items[i], &items[j]);
            if a.1 == b.1 && (a.2 - b.2).abs() <= 30_000_000 {
                return None;
            }
            let c = match (&a.4, &b.4) {
                (Some(x), Some(y)) => Some(clippro_ai::clip::cosine(x, y)),
                _ => None,
            };
            let t = match (&a.5, &b.5) {
                (Some(x), Some(y)) => j3(x, y),
                _ => None,
            };
            let h = match (a.6, b.6) {
                (Some(x), Some(y)) => Some((x ^ y).count_ones()),
                _ => None,
            };
            Some(take_similarity(c, t, h))
        };
        // Average-linkage agglomerative clustering at similarity ≥ 0.80, on a similarity matrix
        // updated in place (sum and count per cluster pair; a pair that may not group blocks).
        let mut sum = vec![0f32; n * n];
        let mut cnt = vec![0u32; n * n];
        let mut blocked = vec![false; n * n];
        for i in 0..n {
            for j in i + 1..n {
                match sim(i, j) {
                    Some(v) => {
                        sum[i * n + j] = v;
                        sum[j * n + i] = v;
                        cnt[i * n + j] = 1;
                        cnt[j * n + i] = 1;
                    }
                    None => {
                        blocked[i * n + j] = true;
                        blocked[j * n + i] = true;
                    }
                }
            }
        }
        let mut members: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
        let mut alive: Vec<bool> = vec![true; n];
        loop {
            let mut best: Option<(f32, usize, usize)> = None;
            for i in (0..n).filter(|&i| alive[i]) {
                for j in (i + 1..n).filter(|&j| alive[j]) {
                    let k = i * n + j;
                    if blocked[k] || cnt[k] == 0 {
                        continue;
                    }
                    let avg = sum[k] / cnt[k] as f32;
                    if avg >= 0.80 && best.is_none_or(|b| avg > b.0) {
                        best = Some((avg, i, j));
                    }
                }
            }
            let Some((_, i, j)) = best else { break };
            alive[j] = false;
            let moved = std::mem::take(&mut members[j]);
            members[i].extend(moved);
            for k in (0..n).filter(|&k| alive[k] && k != i) {
                let (ik, jk) = (i * n + k, j * n + k);
                let (s2, c2, b2) = (sum[ik] + sum[jk], cnt[ik] + cnt[jk], blocked[ik] || blocked[jk]);
                for x in [ik, k * n + i] {
                    sum[x] = s2;
                    cnt[x] = c2;
                    blocked[x] = b2;
                }
            }
        }
        let mut out = Vec::new();
        for (gi, g) in members.iter().filter(|g| !g.is_empty()).enumerate() {
            for &i in g {
                out.push((items[i].0, gi));
            }
        }
        Ok(out)
    }

    /// Take groups with ranks (1 = best: highest mean, then longer): (clip, select start µs,
    /// group, rank, group size). Only groups of two or more are returned.
    pub fn take_ranking(&self, sel: &Selects) -> Result<Vec<(ClipId, i64, usize, usize, usize)>> {
        let groups = self.take_groups(sel)?;
        let mut by: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
        for &(k, g) in &groups {
            by.entry(g).or_default().push(k);
        }
        let mut out = Vec::new();
        for (g, mut v) in by.into_iter().filter(|x| x.1.len() > 1) {
            let seg = |k: &(usize, usize)| &sel.clips[k.0].segments[k.1];
            v.sort_by(|a, b| seg(b).mean.total_cmp(&seg(a).mean).then(seg(b).len_us().cmp(&seg(a).len_us())));
            let size = v.len();
            for (r, k) in v.iter().enumerate() {
                out.push((sel.clips[k.0].clip, seg(k).start_us, g, r + 1, size));
            }
        }
        out.sort_by_key(|x| (x.2, x.3));
        Ok(out)
    }

    /// Best take only (TUN-13): drops every select that isn't the best of its take group
    /// (highest mean; ties go to the longer one).
    pub fn apply_best_take(&self, sel: &mut Selects) -> Result<()> {
        let groups = self.take_groups(sel)?;
        let mut best: HashMap<usize, (usize, usize)> = HashMap::new();
        for &((ci, si), g) in &groups {
            let s = &sel.clips[ci].segments[si];
            let better = |o: &(usize, usize)| {
                let t = &sel.clips[o.0].segments[o.1];
                (s.mean, s.len_us()) > (t.mean, t.len_us())
            };
            match best.get(&g) {
                Some(o) if !better(o) => {}
                _ => {
                    best.insert(g, (ci, si));
                }
            }
        }
        let keep: std::collections::HashSet<(usize, usize)> = best.values().copied().collect();
        for (ci, c) in sel.clips.iter_mut().enumerate() {
            let mut si = 0;
            c.segments.retain(|_| {
                let k = keep.contains(&(ci, si));
                si += 1;
                k
            });
        }
        sel.summary.segment_count = sel.clips.iter().map(|c| c.segments.len()).sum();
        sel.summary.selects_us = sel.clips.iter().flat_map(|c| &c.segments).map(|x| x.len_us()).sum();
        sel.summary.percent = if sel.summary.footage_us > 0 { 100.0 * sel.summary.selects_us as f32 / sel.summary.footage_us as f32 } else { 0.0 };
        Ok(())
    }

    pub fn transcript(&self, clip: ClipId) -> Result<Option<Transcript>> {
        let (m, _, _) = self.media_of(clip)?;
        Ok(self.ai_data(m)?.transcript.clone())
    }

    /// Drops a clip's AI results (re-run them).
    pub fn clear_ai(&self, clip: ClipId) -> Result<()> {
        let (m, _, _) = self.media_of(clip)?;
        lock(&self.db).clear_ai(m)?;
        lock(&self.ai_cache).remove(&m.0);
        Ok(())
    }
}

/// Best thumbnail sample of samples `a..b` (ALGORITHMS §8.7): the argmax of
/// `F · (0.5 + 0.5·face_in_focus, when faces ran) · (1 − 0.5·motion_blur)`, ignoring the outer 10%
/// of the span. Falls back to `a` when nothing scores.
pub fn best_thumbnail(scores: &[f32], flags: &[u32], face_focus: Option<&[f32]>, a: usize, b: usize) -> usize {
    let b = b.min(scores.len());
    if a >= b {
        return a;
    }
    let edge = (b - a) / 10;
    let (lo, hi) = if b - a > 2 * edge + 1 { (a + edge, b - edge) } else { (a, b) };
    let blur = FlagKind::MotionBlur.bit();
    let value = |k: usize| {
        let f = scores[k];
        if !f.is_finite() {
            return f32::NAN;
        }
        let face = face_focus.map(|v| 0.5 + 0.5 * v.get(k).copied().filter(|x| x.is_finite()).unwrap_or(0.0)).unwrap_or(1.0);
        let mb = if flags.get(k).is_some_and(|x| x & blur != 0) { 0.5 } else { 1.0 };
        f * face * mb
    };
    (lo..hi).filter(|&k| value(k).is_finite()).max_by(|&i, &j| value(i).total_cmp(&value(j)).then(j.cmp(&i))).unwrap_or(a)
}

/// Mono 16 kHz audio of a file (empty when it has none).
fn mono_16k(path: &std::path::Path, info: &MediaInfo, index: Option<usize>) -> Result<Vec<f32>> {
    let mut out = Vec::new();
    let opts = clippro_media::audio::AudioOptions { index, video_start_us: info.start_us, from_us: None, out_rate: 16_000 };
    let r = clippro_media::audio::decode_audio_with(path, &opts, |block| {
        let n = block.first().map(Vec::len).unwrap_or(0);
        let k = block.len().max(1) as f32;
        out.extend((0..n).map(|i| block.iter().map(|c| c[i]).sum::<f32>() / k));
        true
    });
    match r {
        Ok(_) => Ok(out),
        Err(clippro_media::MediaError::NoStream(_)) => Ok(Vec::new()),
        Err(e) => Err(SessionError::Other(e.to_string())),
    }
}

/// Settings note: `Settings::best_take_only` is applied by `Session::score`.
pub fn wants_best_take(s: &Settings) -> bool {
    s.best_take_only
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similarity_terms_and_j3() {
        assert!((take_similarity(Some(0.9), None, None) - 0.9).abs() < 1e-6);
        assert!((take_similarity(Some(1.0), Some(0.0), Some(0)) - 0.7).abs() < 1e-6);
        assert_eq!(take_similarity(None, None, None), 0.0);
        let j = j3("the quick brown fox jumps", "the quick brown fox runs").unwrap();
        assert!(j > 0.3 && j < 1.0, "{j}");
        assert!(j3("a b", "c d").is_none());
        let e = vec![vec![1.0, 0.0], vec![0.0, 1.0]];
        assert_eq!(decode_embeddings(&encode_embeddings(&e)), e);
    }

    #[test]
    fn thumbnail_prefers_sharp_faces_away_from_edges() {
        let scores = vec![0.99, 0.6, 0.7, 0.7, 0.65, 0.6, 0.6, 0.6, 0.6, 0.6, 0.99];
        assert_eq!(best_thumbnail(&scores, &[], None, 0, 11), 2, "edges ignored; first of ties");
        let face = vec![f32::NAN, 0.1, 0.1, 0.2, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        assert_eq!(best_thumbnail(&scores, &[], Some(&face), 0, 11), 4);
        let mut flags = vec![0u32; 11];
        flags[4] = FlagKind::MotionBlur.bit();
        assert_eq!(best_thumbnail(&scores, &flags, Some(&face), 0, 11), 3);
        assert_eq!(best_thumbnail(&[f32::NAN; 3], &[], None, 0, 3), 0);
    }

    #[test]
    fn ai_data_lookups() {
        let mut d = AiData::default();
        for k in 0..6 {
            let faces = if k < 4 { vec![FaceLite { bbox: [0.0; 4], score: 0.9, sharpness: -2.0, size: 0.2, track: Some(0) }] } else { vec![] };
            let mut shot = vec![0.0; 7];
            shot[if k < 4 { 2 } else { 0 }] = 1.0;
            d.vision.push(VisionSample { t_us: k * STEP_US, faces: Some(faces), scene: Some(Scene { shot, outdoor: 0.5, night: 0.1, aesthetic: 6.0 }) });
            d.embeddings.push(vec![1.0, k as f32]);
        }
        d.track_people = vec![Some(7)];
        assert_eq!(d.sample_at(760_000), Some(2));
        assert_eq!(d.sample_at(10_000_000), None);
        assert_eq!(d.people_in(0, 1_000_000), vec![7]);
        assert!(d.people_in(2_000_000, 3_000_000).is_empty());
        assert_eq!(d.shot_in(0, 2_000_000).map(|s| s.0), Some(ShotType::CloseUp));
        assert_eq!(d.shot_in(2_000_000, 3_000_000).map(|s| s.0), Some(ShotType::Wide));
        assert!(d.embedding_in(0, 1_000_000).is_some());
    }
}
