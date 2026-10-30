//! Typed project operations: media, clips, analysis runs, checkpoints, measurements,
//! overrides (SPEC §12.2) and the global measurement cache (SPEC ING-8).

use crate::db::{now, Db, ProjectError, Result};
use clippro_types::{ClipId, GroupMask, MediaId, RunId, SampleRate};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Identity of an analysis result: same media content + same engine settings ⇒ same result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunParams {
    pub engine_version: u32,
    pub calibration_version: u32,
    pub rate: SampleRate,
    pub pair_sampling: bool,
    pub groups: GroupMask,
}

impl RunParams {
    pub fn key(&self) -> String {
        format!(
            "e{}-c{}-{}-p{}-g{:x}",
            self.engine_version,
            self.calibration_version,
            self.rate.label().to_lowercase().replace(' ', ""),
            self.pair_sampling as u8,
            self.groups.0
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MediaRow {
    pub id: MediaId,
    pub path: String,
    pub fingerprint: String,
    pub content_fingerprint: String,
    pub size: i64,
    pub probe_json: String,
    pub status: String,
    pub proxy_media_id: Option<MediaId>,
    pub is_proxy: bool,
    pub duration_us: i64,
    pub profile: String,
    /// Chosen audio stream (container index); `None` = FFmpeg's best stream.
    pub audio_stream: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClipRow {
    pub id: ClipId,
    pub media_id: MediaId,
    pub name: String,
    pub sort_index: i64,
    pub rating: i64,
    pub color_label: Option<String>,
    pub note: Option<String>,
    pub bin_id: Option<i64>,
}

/// A bin (folder of clips) or smart collection (`kind = smart`, with a saved query).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BinRow {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub query_json: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Queued,
    Running,
    Paused,
    Done,
    Failed,
}

impl RunState {
    fn as_str(self) -> &'static str {
        match self {
            RunState::Queued => "queued",
            RunState::Running => "running",
            RunState::Paused => "paused",
            RunState::Done => "done",
            RunState::Failed => "failed",
        }
    }
    fn parse(s: &str) -> RunState {
        match s {
            "running" => RunState::Running,
            "paused" => RunState::Paused,
            "done" => RunState::Done,
            "failed" => RunState::Failed,
            _ => RunState::Queued,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunRow {
    pub id: RunId,
    pub media_id: MediaId,
    pub key: String,
    pub state: RunState,
    pub progress: f64,
    pub error: Option<String>,
    pub active_json: Option<String>,
    pub decode_path: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChunkRow {
    pub section: u32,
    pub chunk_index: u32,
    pub first_sample: usize,
    pub count: usize,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverrideRow {
    pub id: i64,
    pub clip_id: ClipId,
    pub kind: String,
    pub start_us: i64,
    pub end_us: i64,
}

/// New media to register.
pub struct NewMedia<'a> {
    pub path: &'a Path,
    pub fingerprint: &'a str,
    pub content_fingerprint: &'a str,
    pub size: u64,
    pub probe_json: &'a str,
    pub duration_us: i64,
    pub width: u32,
    pub height: u32,
    pub fps: (i64, i64),
    pub reel: Option<&'a str>,
    pub camera: Option<&'a str>,
    pub start_tc: Option<&'a str>,
}

impl Db {
    pub fn add_media(&self, m: &NewMedia) -> Result<MediaId> {
        self.conn.execute(
            "INSERT INTO media(path, fingerprint, content_fingerprint, size, probe_json, duration_us, width, height, fps_num, fps_den, reel, camera, start_tc, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                m.path.to_string_lossy(),
                m.fingerprint,
                m.content_fingerprint,
                m.size as i64,
                m.probe_json,
                m.duration_us,
                m.width,
                m.height,
                m.fps.0,
                m.fps.1,
                m.reel,
                m.camera,
                m.start_tc,
                now()
            ],
        )?;
        self.touch()?;
        Ok(MediaId(self.conn.last_insert_rowid()))
    }

    fn media_from(r: &rusqlite::Row) -> rusqlite::Result<MediaRow> {
        Ok(MediaRow {
            id: MediaId(r.get(0)?),
            path: r.get(1)?,
            fingerprint: r.get(2)?,
            content_fingerprint: r.get(3)?,
            size: r.get(4)?,
            probe_json: r.get(5)?,
            status: r.get(6)?,
            proxy_media_id: r.get::<_, Option<i64>>(7)?.map(MediaId),
            is_proxy: r.get::<_, i64>(8)? != 0,
            duration_us: r.get::<_, Option<i64>>(9)?.unwrap_or(0),
            profile: r.get(10)?,
            audio_stream: r.get(11)?,
        })
    }

    const MEDIA_COLS: &'static str =
        "id, path, fingerprint, content_fingerprint, size, probe_json, status, proxy_media_id, is_proxy, duration_us, profile, audio_stream";

    pub fn set_audio_stream(&self, media: MediaId, stream: Option<i64>) -> Result<()> {
        self.conn.execute("UPDATE media SET audio_stream = ?1 WHERE id = ?2", params![stream, media.0])?;
        Ok(())
    }

    pub fn media_all(&self) -> Result<Vec<MediaRow>> {
        let mut st = self.conn.prepare(&format!("SELECT {} FROM media ORDER BY id", Self::MEDIA_COLS))?;
        let rows = st.query_map([], Self::media_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn media(&self, id: MediaId) -> Result<Option<MediaRow>> {
        Ok(self
            .conn
            .query_row(&format!("SELECT {} FROM media WHERE id = ?1", Self::MEDIA_COLS), params![id.0], Self::media_from)
            .optional()?)
    }

    pub fn media_by_fingerprint(&self, fp: &str) -> Result<Vec<MediaRow>> {
        let mut st =
            self.conn.prepare(&format!("SELECT {} FROM media WHERE fingerprint = ?1 OR content_fingerprint = ?1", Self::MEDIA_COLS))?;
        let rows = st.query_map(params![fp], Self::media_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_media_path(&self, id: MediaId, path: &Path, fingerprint: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE media SET path = ?2, fingerprint = ?3, status = 'ok' WHERE id = ?1",
            params![id.0, path.to_string_lossy(), fingerprint],
        )?;
        Ok(())
    }

    pub fn set_media_status(&self, id: MediaId, status: &str) -> Result<()> {
        self.conn.execute("UPDATE media SET status = ?2 WHERE id = ?1", params![id.0, status])?;
        Ok(())
    }

    pub fn set_proxy(&self, original: MediaId, proxy: Option<MediaId>) -> Result<()> {
        self.conn.execute("UPDATE media SET proxy_media_id = ?2 WHERE id = ?1", params![original.0, proxy.map(|p| p.0)])?;
        if let Some(p) = proxy {
            self.conn.execute("UPDATE media SET is_proxy = 1 WHERE id = ?1", params![p.0])?;
        }
        Ok(())
    }

    pub fn set_profile(&self, id: MediaId, profile: &str) -> Result<()> {
        self.conn.execute("UPDATE media SET profile = ?2 WHERE id = ?1", params![id.0, profile])?;
        Ok(())
    }

    pub fn add_clip(&self, media: MediaId, name: &str) -> Result<ClipId> {
        let next: i64 = self.conn.query_row("SELECT COALESCE(MAX(sort_index), -1) + 1 FROM clip", [], |r| r.get(0))?;
        self.conn.execute("INSERT INTO clip(media_id, name, sort_index) VALUES (?1, ?2, ?3)", params![media.0, name, next])?;
        self.touch()?;
        Ok(ClipId(self.conn.last_insert_rowid()))
    }

    pub fn clips(&self) -> Result<Vec<ClipRow>> {
        let mut st = self.conn.prepare("SELECT id, media_id, name, sort_index, rating, color_label, note, bin_id FROM clip ORDER BY sort_index, id")?;
        let rows = st
            .query_map([], |r| {
                Ok(ClipRow {
                    id: ClipId(r.get(0)?),
                    media_id: MediaId(r.get(1)?),
                    name: r.get(2)?,
                    sort_index: r.get(3)?,
                    rating: r.get(4)?,
                    color_label: r.get(5)?,
                    note: r.get(6)?,
                    bin_id: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn remove_clip(&self, id: ClipId) -> Result<()> {
        self.conn.execute("DELETE FROM clip WHERE id = ?1", params![id.0])?;
        Ok(())
    }

    pub fn set_clip_rating(&self, id: ClipId, rating: i64, label: Option<&str>, note: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE clip SET rating = ?2, color_label = ?3, note = ?4 WHERE id = ?1",
            params![id.0, rating.clamp(0, 5), label, note],
        )?;
        Ok(())
    }

    /// Returns the run for (media, params), creating it if needed.
    pub fn run_for(&self, media: MediaId, p: &RunParams) -> Result<RunRow> {
        let key = p.key();
        if let Some(r) = self.run_by_key(media, &key)? {
            return Ok(r);
        }
        self.conn.execute(
            "INSERT INTO analysis_run(media_id, run_key, engine_version, calibration_version, sample_rate, pair_sampling, groups_mask)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![media.0, key, p.engine_version, p.calibration_version, p.rate.label(), p.pair_sampling as i64, p.groups.0 as i64],
        )?;
        self.run_by_key(media, &key)?.ok_or_else(|| ProjectError::Other("run insert failed".into()))
    }

    fn run_from(r: &rusqlite::Row) -> rusqlite::Result<RunRow> {
        Ok(RunRow {
            id: RunId(r.get(0)?),
            media_id: MediaId(r.get(1)?),
            key: r.get(2)?,
            state: RunState::parse(&r.get::<_, String>(3)?),
            progress: r.get(4)?,
            error: r.get(5)?,
            active_json: r.get(6)?,
            decode_path: r.get(7)?,
        })
    }

    const RUN_COLS: &'static str = "id, media_id, run_key, state, progress, error, active_json, decode_path";

    pub fn run_by_key(&self, media: MediaId, key: &str) -> Result<Option<RunRow>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {} FROM analysis_run WHERE media_id = ?1 AND run_key = ?2", Self::RUN_COLS),
                params![media.0, key],
                Self::run_from,
            )
            .optional()?)
    }

    pub fn run(&self, id: RunId) -> Result<Option<RunRow>> {
        Ok(self
            .conn
            .query_row(&format!("SELECT {} FROM analysis_run WHERE id = ?1", Self::RUN_COLS), params![id.0], Self::run_from)
            .optional()?)
    }

    pub fn runs_for_media(&self, media: MediaId) -> Result<Vec<RunRow>> {
        let mut st = self.conn.prepare(&format!("SELECT {} FROM analysis_run WHERE media_id = ?1 ORDER BY id", Self::RUN_COLS))?;
        let rows = st.query_map(params![media.0], Self::run_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_run_state(&self, id: RunId, state: RunState, error: Option<&str>) -> Result<()> {
        let (started, finished) = match state {
            RunState::Running => (Some(now()), None),
            RunState::Done | RunState::Failed => (None, Some(now())),
            _ => (None, None),
        };
        self.conn.execute(
            "UPDATE analysis_run SET state = ?2, error = ?3,
               started_at = COALESCE(?4, started_at), finished_at = COALESCE(?5, finished_at) WHERE id = ?1",
            params![id.0, state.as_str(), error, started, finished],
        )?;
        Ok(())
    }

    pub fn set_run_progress(&self, id: RunId, progress: f64) -> Result<()> {
        self.conn.execute("UPDATE analysis_run SET progress = ?2 WHERE id = ?1", params![id.0, progress.clamp(0.0, 1.0)])?;
        Ok(())
    }

    pub fn set_run_meta(&self, id: RunId, active_json: &str, decode_path: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE analysis_run SET active_json = ?2, decode_path = ?3 WHERE id = ?1",
            params![id.0, active_json, decode_path],
        )?;
        Ok(())
    }

    /// Discards checkpoints and results (re-analyse).
    pub fn reset_run(&self, id: RunId) -> Result<()> {
        self.conn.execute("DELETE FROM sample_chunk WHERE run_id = ?1", params![id.0])?;
        self.conn.execute("DELETE FROM measurement_set WHERE run_id = ?1", params![id.0])?;
        self.conn.execute(
            "UPDATE analysis_run SET state = 'queued', progress = 0, error = NULL, active_json = NULL WHERE id = ?1",
            params![id.0],
        )?;
        Ok(())
    }

    pub fn save_chunk(&self, run: RunId, section: u32, chunk_index: u32, first_sample: usize, count: usize, data: &[u8]) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sample_chunk(run_id, section, chunk_index, first_sample, count, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![run.0, section, chunk_index, first_sample as i64, count as i64, data],
        )?;
        Ok(())
    }

    pub fn chunks(&self, run: RunId) -> Result<Vec<ChunkRow>> {
        let mut st = self.conn.prepare(
            "SELECT section, chunk_index, first_sample, count, data FROM sample_chunk WHERE run_id = ?1 ORDER BY section, chunk_index",
        )?;
        let rows = st
            .query_map(params![run.0], |r| {
                Ok(ChunkRow {
                    section: r.get(0)?,
                    chunk_index: r.get(1)?,
                    first_sample: r.get::<_, i64>(2)? as usize,
                    count: r.get::<_, i64>(3)? as usize,
                    data: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Stores the final measurement set and drops the now-redundant checkpoints.
    pub fn save_measurements(&mut self, run: RunId, blob: &[u8], summary_json: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO measurement_set(run_id, data, summary_json) VALUES (?1, ?2, ?3)",
            params![run.0, blob, summary_json],
        )?;
        tx.execute("DELETE FROM sample_chunk WHERE run_id = ?1", params![run.0])?;
        tx.execute("UPDATE analysis_run SET state = 'done', progress = 1, finished_at = ?2 WHERE id = ?1", params![run.0, now()])?;
        tx.commit()?;
        Ok(())
    }

    /// Audio measurements of a run (stream 0 = the analysed audio stream).
    pub fn save_audio(&self, run: RunId, blob: &[u8], summary_json: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO audio_set(run_id, stream_index, data, summary_json) VALUES (?1, 0, ?2, ?3)",
            params![run.0, blob, summary_json],
        )?;
        Ok(())
    }

    pub fn audio(&self, run: RunId) -> Result<Option<Vec<u8>>> {
        Ok(self.conn.query_row("SELECT data FROM audio_set WHERE run_id = ?1 AND stream_index = 0", params![run.0], |r| r.get(0)).optional()?)
    }

    pub fn measurements(&self, run: RunId) -> Result<Option<Vec<u8>>> {
        Ok(self.conn.query_row("SELECT data FROM measurement_set WHERE run_id = ?1", params![run.0], |r| r.get(0)).optional()?)
    }

    pub fn add_override(&self, clip: ClipId, kind: &str, start_us: i64, end_us: i64) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO override(clip_id, kind, start_us, end_us) VALUES (?1, ?2, ?3, ?4)",
            params![clip.0, kind, start_us, end_us],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn bins(&self) -> Result<Vec<BinRow>> {
        let mut st = self.conn.prepare("SELECT id, name, kind, query_json FROM bin ORDER BY kind, name COLLATE NOCASE")?;
        let rows = st.query_map([], |r| Ok(BinRow { id: r.get(0)?, name: r.get(1)?, kind: r.get(2)?, query_json: r.get(3)? }))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn add_bin(&self, name: &str, kind: &str, query_json: Option<&str>) -> Result<i64> {
        self.conn.execute("INSERT INTO bin(name, kind, query_json) VALUES (?1, ?2, ?3)", params![name, kind, query_json])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_bin(&self, id: i64, name: &str, query_json: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE bin SET name = ?1, query_json = ?2 WHERE id = ?3", params![name, query_json, id])?;
        Ok(())
    }

    /// Deletes a bin; its clips go back to the project root.
    pub fn delete_bin(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE clip SET bin_id = NULL WHERE bin_id = ?1", params![id])?;
        self.conn.execute("DELETE FROM bin WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ---------- AI (P12) ----------

    /// Stores one module's result for a media file (replacing any earlier one).
    pub fn save_ai(&self, media: MediaId, module: &str, model_id: &str, model_hash: &str, data: &[u8]) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO ai_result(media_id, module, model_id, model_hash, data, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![media.0, module, model_id, model_hash, data, now()],
        )?;
        Ok(())
    }

    /// (model_id, data) of a module's result.
    pub fn ai(&self, media: MediaId, module: &str) -> Result<Option<(String, Vec<u8>)>> {
        let mut st = self.conn.prepare("SELECT model_id, data FROM ai_result WHERE media_id = ?1 AND module = ?2")?;
        let mut rows = st.query(params![media.0, module])?;
        Ok(match rows.next()? {
            Some(r) => Some((r.get(0)?, r.get(1)?)),
            None => None,
        })
    }

    /// Media ids that have a result for `module`.
    pub fn ai_media(&self, module: &str) -> Result<Vec<MediaId>> {
        let mut st = self.conn.prepare("SELECT media_id FROM ai_result WHERE module = ?1")?;
        let rows = st.query_map(params![module], |r| Ok(MediaId(r.get(0)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn clear_ai(&self, media: MediaId) -> Result<()> {
        self.conn.execute("DELETE FROM ai_result WHERE media_id = ?1", params![media.0])?;
        self.conn.execute("DELETE FROM face_track WHERE media_id = ?1", params![media.0])?;
        self.conn.execute("DELETE FROM transcript WHERE media_id = ?1", params![media.0])?;
        self.conn.execute("DELETE FROM transcript_fts WHERE media_id = ?1", params![media.0])?;
        Ok(())
    }

    /// Replaces a media file's face tracks; returns their ids.
    pub fn set_face_tracks(&self, media: MediaId, tracks: &[Vec<u8>]) -> Result<Vec<i64>> {
        self.conn.execute("DELETE FROM face_track WHERE media_id = ?1", params![media.0])?;
        let mut ids = Vec::new();
        for t in tracks {
            self.conn.execute("INSERT INTO face_track(media_id, data) VALUES (?1, ?2)", params![media.0, t])?;
            ids.push(self.conn.last_insert_rowid());
        }
        Ok(ids)
    }

    /// (track id, media id, person id, data) of every face track.
    pub fn face_tracks(&self) -> Result<Vec<(i64, MediaId, Option<i64>, Vec<u8>)>> {
        let mut st = self.conn.prepare("SELECT id, media_id, person_id, data FROM face_track ORDER BY id")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, MediaId(r.get(1)?), r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_track_person(&self, track: i64, person: Option<i64>) -> Result<()> {
        self.conn.execute("UPDATE face_track SET person_id = ?1 WHERE id = ?2", params![person, track])?;
        Ok(())
    }

    /// (id, name, cover, embedding) of every person.
    pub fn people(&self) -> Result<Vec<(i64, String, Option<String>, Option<Vec<u8>>)>> {
        let mut st = self.conn.prepare("SELECT id, name, cover_face_ref, embedding FROM person ORDER BY id")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn add_person(&self, name: &str, cover: Option<&str>, embedding: &[u8]) -> Result<i64> {
        self.conn.execute("INSERT INTO person(name, cover_face_ref, embedding) VALUES (?1, ?2, ?3)", params![name, cover, embedding])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_person(&self, id: i64, name: &str, cover: Option<&str>, embedding: &[u8]) -> Result<()> {
        self.conn.execute("UPDATE person SET name = ?1, cover_face_ref = ?2, embedding = ?3 WHERE id = ?4", params![name, cover, embedding, id])?;
        Ok(())
    }

    pub fn rename_person(&self, id: i64, name: &str) -> Result<()> {
        self.conn.execute("UPDATE person SET name = ?1 WHERE id = ?2", params![name, id])?;
        Ok(())
    }

    /// Deletes a person; their tracks become unassigned.
    pub fn delete_person(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE face_track SET person_id = NULL WHERE person_id = ?1", params![id])?;
        self.conn.execute("DELETE FROM person WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Stores a transcript and indexes its segments for full-text search.
    pub fn save_transcript(&self, media: MediaId, language: &str, data: &[u8], segments: &[(i64, String)]) -> Result<()> {
        self.conn.execute("INSERT OR REPLACE INTO transcript(media_id, language, data) VALUES (?1, ?2, ?3)", params![media.0, language, data])?;
        self.conn.execute("DELETE FROM transcript_fts WHERE media_id = ?1", params![media.0])?;
        for (start, text) in segments {
            self.conn.execute("INSERT INTO transcript_fts(media_id, start_us, text) VALUES (?1, ?2, ?3)", params![media.0, start, text])?;
        }
        Ok(())
    }

    pub fn transcript(&self, media: MediaId) -> Result<Option<(String, Vec<u8>)>> {
        let mut st = self.conn.prepare("SELECT language, data FROM transcript WHERE media_id = ?1")?;
        let mut rows = st.query(params![media.0])?;
        Ok(match rows.next()? {
            Some(r) => Some((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get(1)?)),
            None => None,
        })
    }

    /// Full-text search over transcripts: (media, segment start µs, text), best first.
    pub fn search_transcripts(&self, query: &str, limit: usize) -> Result<Vec<(MediaId, i64, String)>> {
        // Quote each word so punctuation in the query can't break FTS syntax.
        let q: Vec<String> = query.split_whitespace().map(|w| format!("\"{}\"", w.replace('"', ""))).filter(|w| w.len() > 2).collect();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let mut st = self.conn.prepare("SELECT media_id, start_us, text FROM transcript_fts WHERE transcript_fts MATCH ?1 ORDER BY rank LIMIT ?2")?;
        let rows = st
            .query_map(params![q.join(" "), limit as i64], |r| Ok((MediaId(r.get(0)?), r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Multicam sync groups (P11.5): (id, name, method, data_json).
    pub fn sync_groups(&self) -> Result<Vec<(i64, String, String, String)>> {
        let mut st = self.conn.prepare("SELECT id, name, method, data_json FROM sync_group ORDER BY name COLLATE NOCASE, id")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Inserts a sync group (`id` None) or replaces one; returns its id.
    pub fn save_sync_group(&self, id: Option<i64>, name: &str, method: &str, data_json: &str) -> Result<i64> {
        match id {
            Some(id) => {
                self.conn.execute("UPDATE sync_group SET name = ?1, method = ?2, data_json = ?3 WHERE id = ?4", params![name, method, data_json, id])?;
                Ok(id)
            }
            None => {
                self.conn.execute("INSERT INTO sync_group(name, method, data_json) VALUES (?1, ?2, ?3)", params![name, method, data_json])?;
                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    pub fn delete_sync_group(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM sync_group WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn set_clip_bin(&self, clip: ClipId, bin: Option<i64>) -> Result<()> {
        self.conn.execute("UPDATE clip SET bin_id = ?1 WHERE id = ?2", params![bin, clip.0])?;
        Ok(())
    }

    /// Annotation rows (kind `annotation`): (id, clip, start, end, data_json).
    pub fn annotations(&self) -> Result<Vec<(i64, ClipId, i64, i64, String)>> {
        let mut st = self.conn.prepare("SELECT id, clip_id, start_us, end_us, data_json FROM override WHERE kind = 'annotation' ORDER BY id")?;
        let rows = st
            .query_map([], |r| Ok((r.get(0)?, ClipId(r.get(1)?), r.get(2)?, r.get(3)?, r.get(4)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn put_annotation(&self, id: Option<i64>, clip: ClipId, start_us: i64, end_us: i64, data_json: &str) -> Result<i64> {
        match id {
            Some(id) => {
                self.conn.execute("UPDATE override SET start_us = ?1, end_us = ?2, data_json = ?3 WHERE id = ?4 AND kind = 'annotation'", params![start_us, end_us, data_json, id])?;
                Ok(id)
            }
            None => {
                self.conn.execute("INSERT INTO override(clip_id, kind, start_us, end_us, data_json) VALUES (?1, 'annotation', ?2, ?3, ?4)", params![clip.0, start_us, end_us, data_json])?;
                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    pub fn remove_override(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM override WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn overrides(&self) -> Result<Vec<OverrideRow>> {
        let mut st = self.conn.prepare("SELECT id, clip_id, kind, start_us, end_us FROM override ORDER BY id")?;
        let rows = st
            .query_map([], |r| {
                Ok(OverrideRow { id: r.get(0)?, clip_id: ClipId(r.get(1)?), kind: r.get(2)?, start_us: r.get(3)?, end_us: r.get(4)? })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn save_preset(&self, name: &str, toml: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO preset(name, toml) VALUES (?1, ?2) ON CONFLICT(name) DO UPDATE SET toml = excluded.toml",
            params![name, toml],
        )?;
        Ok(())
    }

    pub fn presets(&self) -> Result<Vec<(String, String)>> {
        let mut st = self.conn.prepare("SELECT name, toml FROM preset ORDER BY name")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Appends a tuning-history entry, keeping the most recent `keep`.
    pub fn push_history(&self, json: &str, keep: usize) -> Result<i64> {
        self.conn.execute("INSERT INTO tuning_state(kind, json, created_at) VALUES ('history', ?1, ?2)", params![json, now()])?;
        let id = self.conn.last_insert_rowid();
        self.conn.execute(
            "DELETE FROM tuning_state WHERE kind = 'history' AND id NOT IN (SELECT id FROM tuning_state WHERE kind = 'history' ORDER BY id DESC LIMIT ?1)",
            params![keep as i64],
        )?;
        Ok(id)
    }

    /// Named tuning snapshot (SPEC TUN-10); a snapshot with the same name is replaced.
    pub fn save_snapshot(&self, name: &str, json: &str) -> Result<i64> {
        self.conn.execute("DELETE FROM tuning_state WHERE kind = 'snapshot' AND name = ?1", params![name])?;
        self.conn.execute("INSERT INTO tuning_state(kind, name, json, created_at) VALUES ('snapshot', ?1, ?2, ?3)", params![name, json, now()])?;
        Ok(self.conn.last_insert_rowid())
    }

    /// (id, name, json, created_at), oldest first.
    pub fn snapshots(&self) -> Result<Vec<(i64, String, String, String)>> {
        let mut st = self.conn.prepare("SELECT id, name, json, created_at FROM tuning_state WHERE kind = 'snapshot' ORDER BY id")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default(), r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn delete_snapshot(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM tuning_state WHERE kind = 'snapshot' AND id = ?1", params![id])?;
        Ok(())
    }

    /// The tuning in effect when the project was last used (restored on open).
    pub fn set_current_tuning(&self, json: &str) -> Result<()> {
        self.conn.execute("DELETE FROM tuning_state WHERE kind = 'current'", [])?;
        self.conn.execute("INSERT INTO tuning_state(kind, json, created_at) VALUES ('current', ?1, ?2)", params![json, now()])?;
        Ok(())
    }

    pub fn current_tuning(&self) -> Result<Option<String>> {
        let mut st = self.conn.prepare("SELECT json FROM tuning_state WHERE kind = 'current' ORDER BY id DESC LIMIT 1")?;
        let mut rows = st.query([])?;
        Ok(match rows.next()? {
            Some(r) => Some(r.get(0)?),
            None => None,
        })
    }

    pub fn history(&self) -> Result<Vec<(i64, String)>> {
        let mut st = self.conn.prepare("SELECT id, json FROM tuning_state WHERE kind = 'history' ORDER BY id")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn record_export(&self, format: &str, path: &str, settings_json: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO export_history(format, path, settings_json, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![format, path, settings_json, now()],
        )?;
        Ok(())
    }
}

/// Global measurement cache shared by all projects (SPEC ING-8), keyed by content
/// fingerprint + run key.
pub struct MeasureCache {
    conn: Connection,
    pub path: PathBuf,
}

impl MeasureCache {
    pub fn open(path: &Path) -> Result<MeasureCache> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS cache(
               fingerprint TEXT NOT NULL, run_key TEXT NOT NULL, data BLOB NOT NULL,
               size INTEGER NOT NULL, last_used TEXT NOT NULL, PRIMARY KEY(fingerprint, run_key));",
        )?;
        Ok(MeasureCache { conn, path: path.to_path_buf() })
    }

    /// Default location: `%LOCALAPPDATA%\ClipPro\cache\measure-cache.sqlite`.
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        base.join("ClipPro").join("cache").join("measure-cache.sqlite")
    }

    pub fn get(&self, fingerprint: &str, run_key: &str) -> Result<Option<Vec<u8>>> {
        let r: Option<Vec<u8>> = self
            .conn
            .query_row("SELECT data FROM cache WHERE fingerprint = ?1 AND run_key = ?2", params![fingerprint, run_key], |r| r.get(0))
            .optional()?;
        if r.is_some() {
            self.conn.execute(
                "UPDATE cache SET last_used = ?3 WHERE fingerprint = ?1 AND run_key = ?2",
                params![fingerprint, run_key, now()],
            )?;
        }
        Ok(r)
    }

    pub fn put(&self, fingerprint: &str, run_key: &str, data: &[u8]) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO cache(fingerprint, run_key, data, size, last_used) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![fingerprint, run_key, data, data.len() as i64, now()],
        )?;
        Ok(())
    }

    /// (entries, stored bytes).
    pub fn stats(&self) -> Result<(u64, u64)> {
        let (n, b): (i64, i64) = self.conn.query_row("SELECT COUNT(*), COALESCE(SUM(size), 0) FROM cache", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok((n as u64, b as u64))
    }

    /// Removes every entry and compacts the file.
    pub fn clear(&self) -> Result<u64> {
        let n = self.conn.execute("DELETE FROM cache", [])?;
        self.conn.execute_batch("VACUUM")?;
        Ok(n as u64)
    }

    /// Evicts least-recently-used entries until the cache is within `max_bytes`.
    pub fn evict_to(&self, max_bytes: u64) -> Result<usize> {
        let total: i64 = self.conn.query_row("SELECT COALESCE(SUM(size), 0) FROM cache", [], |r| r.get(0))?;
        let mut over = total - max_bytes as i64;
        if over <= 0 {
            return Ok(0);
        }
        let mut st = self.conn.prepare("SELECT fingerprint, run_key, size FROM cache ORDER BY last_used, fingerprint")?;
        let rows: Vec<(String, String, i64)> =
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
        let mut n = 0;
        for (fp, key, size) in rows {
            if over <= 0 {
                break;
            }
            self.conn.execute("DELETE FROM cache WHERE fingerprint = ?1 AND run_key = ?2", params![fp, key])?;
            over -= size;
            n += 1;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn tmp(name: &str) -> PathBuf {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-project-tests").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn media(db: &Db, path: &str, fp: &str) -> MediaId {
        db.add_media(&NewMedia {
            path: Path::new(path),
            fingerprint: fp,
            content_fingerprint: fp,
            size: 10,
            probe_json: "{}",
            duration_us: 6_000_000,
            width: 1920,
            height: 1080,
            fps: (25, 1),
            reel: Some("A001"),
            camera: None,
            start_tc: None,
        })
        .unwrap()
    }

    fn params() -> RunParams {
        RunParams { engine_version: 2, calibration_version: 2, rate: SampleRate::Standard, pair_sampling: true, groups: GroupMask::ALL_COMPUTE }
    }

    #[test]
    fn media_clips_runs_chunks_measurements() {
        let d = tmp("store");
        let mut db = Db::create(&d.join("p.clippro"), "P").unwrap();
        let m = media(&db, "D:/a.mp4", "fp1");
        let c1 = db.add_clip(m, "a").unwrap();
        let c2 = db.add_clip(m, "a (2)").unwrap();
        let clips = db.clips().unwrap();
        assert_eq!(clips.iter().map(|c| c.id).collect::<Vec<_>>(), vec![c1, c2]);
        let run = db.run_for(m, &params()).unwrap();
        assert_eq!(run.state, RunState::Queued);
        assert_eq!(db.run_for(m, &params()).unwrap().id, run.id, "run_for is idempotent");
        db.set_run_state(run.id, RunState::Running, None).unwrap();
        db.save_chunk(run.id, 0, 0, 0, 256, b"chunk0").unwrap();
        db.save_chunk(run.id, 0, 1, 256, 10, b"chunk1").unwrap();
        db.save_chunk(run.id, 1, 0, 300, 5, b"s1").unwrap();
        let ch = db.chunks(run.id).unwrap();
        assert_eq!(ch.len(), 3);
        assert_eq!(ch[1].first_sample, 256);
        db.save_measurements(run.id, b"blob", "{}").unwrap();
        assert!(db.chunks(run.id).unwrap().is_empty());
        assert_eq!(db.measurements(run.id).unwrap().as_deref(), Some(&b"blob"[..]));
        assert_eq!(db.run(run.id).unwrap().unwrap().state, RunState::Done);
        db.reset_run(run.id).unwrap();
        assert!(db.measurements(run.id).unwrap().is_none());
        // Bins: clips move in and out; deleting a bin keeps its clips.
        let b = db.add_bin("Interviews", "manual", None).unwrap();
        let sm = db.add_bin("Good", "smart", Some(r#"{"match":"all","rules":[]}"#)).unwrap();
        db.set_clip_bin(c1, Some(b)).unwrap();
        assert_eq!(db.clips().unwrap().iter().find(|c| c.id == c1).unwrap().bin_id, Some(b));
        db.update_bin(b, "People", None).unwrap();
        assert_eq!(db.bins().unwrap().iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["People", "Good"]);
        db.delete_bin(b).unwrap();
        assert_eq!(db.clips().unwrap().iter().find(|c| c.id == c1).unwrap().bin_id, None);
        assert_eq!(db.bins().unwrap().len(), 1);
        db.delete_bin(sm).unwrap();
        // AI results, faces, people, transcripts and search.
        db.save_ai(m, "clip", "clip-b32-vision", "abc", b"emb").unwrap();
        assert_eq!(db.ai(m, "clip").unwrap(), Some(("clip-b32-vision".to_string(), b"emb".to_vec())));
        assert_eq!(db.ai_media("clip").unwrap(), vec![m]);
        let tids = db.set_face_tracks(m, &[b"t1".to_vec(), b"t2".to_vec()]).unwrap();
        let p = db.add_person("Sarah", None, b"e").unwrap();
        db.set_track_person(tids[0], Some(p)).unwrap();
        assert_eq!(db.face_tracks().unwrap()[0].2, Some(p));
        db.rename_person(p, "Sarah K").unwrap();
        assert_eq!(db.people().unwrap()[0].1, "Sarah K");
        db.save_transcript(m, "en", b"{}", &[(0, "The quick brown fox".into()), (5_000_000, "jumps over the dog".into())]).unwrap();
        let hits = db.search_transcripts("brown fox", 10).unwrap();
        assert_eq!(hits, vec![(m, 0, "The quick brown fox".to_string())]);
        assert!(db.search_transcripts("\"", 10).unwrap().is_empty());
        db.delete_person(p).unwrap();
        assert_eq!(db.face_tracks().unwrap()[0].2, None);
        db.clear_ai(m).unwrap();
        assert!(db.ai(m, "clip").unwrap().is_none() && db.face_tracks().unwrap().is_empty() && db.transcript(m).unwrap().is_none());
        // Sync groups: insert, replace, delete.
        let g = db.save_sync_group(None, "Cam A+B", "audio", "{}").unwrap();
        assert_eq!(db.save_sync_group(Some(g), "Scene 4", "timecode", "{\"x\":1}").unwrap(), g);
        assert_eq!(db.sync_groups().unwrap(), vec![(g, "Scene 4".to_string(), "timecode".to_string(), "{\"x\":1}".to_string())]);
        db.delete_sync_group(g).unwrap();
        assert!(db.sync_groups().unwrap().is_empty());
        let o = db.add_override(c1, "force_in", 1_000_000, 2_000_000).unwrap();
        assert_eq!(db.overrides().unwrap().len(), 1);
        db.remove_override(o).unwrap();
        assert!(db.overrides().unwrap().is_empty());
        for i in 0..10 {
            db.push_history(&format!("{{\"i\":{i}}}"), 5).unwrap();
        }
        let h = db.history().unwrap();
        assert_eq!(h.len(), 5);
        assert!(h[0].1.contains("\"i\":5"));
        assert_eq!(db.media_by_fingerprint("fp1").unwrap().len(), 1);
        db.close().unwrap();
        std::fs::remove_dir_all(&d).unwrap();
        let _ = std::fs::remove_dir(d.parent().unwrap());
    }

    #[test]
    fn run_keys_differ_by_settings() {
        let a = params();
        let mut b = a.clone();
        b.rate = SampleRate::Draft;
        let mut c = a.clone();
        c.groups = GroupMask(0b0111);
        assert_ne!(a.key(), b.key());
        assert_ne!(a.key(), c.key());
        assert_eq!(a.key(), params().key());
    }

    #[test]
    fn measure_cache_lru() {
        let d = tmp("cache");
        let c = MeasureCache::open(&d.join("cache.sqlite")).unwrap();
        c.put("fp", "k", &[1u8; 100]).unwrap();
        c.put("fp2", "k", &[2u8; 100]).unwrap();
        assert_eq!(c.get("fp", "k").unwrap().unwrap().len(), 100);
        assert!(c.get("fp", "other").unwrap().is_none());
        assert_eq!(c.evict_to(150).unwrap(), 1);
        assert_eq!(c.stats().unwrap(), (1, 100));
        assert_eq!(c.clear().unwrap(), 1);
        assert_eq!(c.stats().unwrap(), (0, 0));
        drop(c);
        std::fs::remove_dir_all(&d).unwrap();
        let _ = std::fs::remove_dir(d.parent().unwrap());
    }
}
