//! `.clippro` project database (SPEC §12): SQLite in WAL mode, forward-only migrations,
//! backup before migrating, clean-shutdown marker for crash recovery.

use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

pub const APPLICATION_ID: i32 = 0x434C_5052; // "CLPR"
pub const SCHEMA_VERSION: i32 = 2;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("database: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0} is not a ClipPro project")]
    NotAProject(String),
    #[error("project was written by a newer ClipPro (schema {0})")]
    TooNew(i32),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ProjectError>;

/// Migration `i` upgrades schema `i` to `i + 1`.
const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE project(
  id INTEGER PRIMARY KEY CHECK (id = 1),
  name TEXT NOT NULL,
  created_at TEXT NOT NULL,
  modified_at TEXT NOT NULL,
  app_version TEXT NOT NULL,
  settings_json TEXT NOT NULL DEFAULT '{}',
  clean_shutdown INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE media(
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL,
  fingerprint TEXT NOT NULL,
  content_fingerprint TEXT NOT NULL DEFAULT '',
  size INTEGER NOT NULL,
  mtime INTEGER NOT NULL DEFAULT 0,
  probe_json TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'ok',
  proxy_media_id INTEGER REFERENCES media(id),
  is_proxy INTEGER NOT NULL DEFAULT 0,
  reel TEXT, camera TEXT, start_tc TEXT,
  fps_num INTEGER, fps_den INTEGER, duration_us INTEGER, width INTEGER, height INTEGER,
  profile TEXT NOT NULL DEFAULT 'rec709',
  added_at TEXT NOT NULL
);
CREATE INDEX media_fp ON media(fingerprint);
CREATE INDEX media_cfp ON media(content_fingerprint);
CREATE TABLE bin(
  id INTEGER PRIMARY KEY,
  parent_id INTEGER REFERENCES bin(id),
  name TEXT NOT NULL,
  kind TEXT NOT NULL DEFAULT 'manual',
  query_json TEXT
);
CREATE TABLE clip(
  id INTEGER PRIMARY KEY,
  media_id INTEGER NOT NULL REFERENCES media(id) ON DELETE CASCADE,
  bin_id INTEGER REFERENCES bin(id),
  name TEXT NOT NULL,
  sort_index INTEGER NOT NULL DEFAULT 0,
  rating INTEGER NOT NULL DEFAULT 0,
  color_label TEXT,
  note TEXT
);
CREATE TABLE analysis_run(
  id INTEGER PRIMARY KEY,
  media_id INTEGER NOT NULL REFERENCES media(id) ON DELETE CASCADE,
  run_key TEXT NOT NULL,
  engine_version INTEGER NOT NULL,
  calibration_version INTEGER NOT NULL,
  sample_rate TEXT NOT NULL,
  pair_sampling INTEGER NOT NULL,
  groups_mask INTEGER NOT NULL,
  decode_path TEXT NOT NULL DEFAULT '',
  active_json TEXT,
  started_at TEXT, finished_at TEXT,
  state TEXT NOT NULL DEFAULT 'queued',
  progress REAL NOT NULL DEFAULT 0,
  error TEXT
);
CREATE UNIQUE INDEX run_media_key ON analysis_run(media_id, run_key);
CREATE TABLE sample_chunk(
  run_id INTEGER NOT NULL REFERENCES analysis_run(id) ON DELETE CASCADE,
  section INTEGER NOT NULL,
  chunk_index INTEGER NOT NULL,
  first_sample INTEGER NOT NULL,
  count INTEGER NOT NULL,
  data BLOB NOT NULL,
  PRIMARY KEY(run_id, section, chunk_index)
);
CREATE TABLE measurement_set(
  run_id INTEGER PRIMARY KEY REFERENCES analysis_run(id) ON DELETE CASCADE,
  data BLOB NOT NULL,
  summary_json TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE audio_set(
  run_id INTEGER NOT NULL REFERENCES analysis_run(id) ON DELETE CASCADE,
  stream_index INTEGER NOT NULL,
  data BLOB NOT NULL,
  summary_json TEXT NOT NULL DEFAULT '{}',
  PRIMARY KEY(run_id, stream_index)
);
CREATE TABLE ai_result(
  media_id INTEGER NOT NULL REFERENCES media(id) ON DELETE CASCADE,
  module TEXT NOT NULL, model_id TEXT NOT NULL, model_hash TEXT NOT NULL,
  data BLOB NOT NULL, created_at TEXT NOT NULL,
  PRIMARY KEY(media_id, module)
);
CREATE TABLE person(id INTEGER PRIMARY KEY, name TEXT NOT NULL, cover_face_ref TEXT, embedding BLOB);
CREATE TABLE face_track(id INTEGER PRIMARY KEY, media_id INTEGER NOT NULL REFERENCES media(id) ON DELETE CASCADE, person_id INTEGER REFERENCES person(id), data BLOB NOT NULL);
CREATE TABLE transcript(media_id INTEGER PRIMARY KEY REFERENCES media(id) ON DELETE CASCADE, language TEXT, data BLOB NOT NULL);
CREATE VIRTUAL TABLE transcript_fts USING fts5(media_id UNINDEXED, start_us UNINDEXED, text);
CREATE TABLE override(
  id INTEGER PRIMARY KEY,
  clip_id INTEGER NOT NULL REFERENCES clip(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  start_us INTEGER NOT NULL,
  end_us INTEGER NOT NULL,
  data_json TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE preset(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, builtin INTEGER NOT NULL DEFAULT 0, toml TEXT NOT NULL);
CREATE TABLE tuning_state(id INTEGER PRIMARY KEY, kind TEXT NOT NULL DEFAULT 'history', name TEXT, json TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE TABLE timeline_import(id INTEGER PRIMARY KEY, source_path TEXT NOT NULL, format TEXT NOT NULL, data_json TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE TABLE sync_group(id INTEGER PRIMARY KEY, name TEXT NOT NULL, method TEXT NOT NULL, data_json TEXT NOT NULL);
CREATE TABLE export_history(id INTEGER PRIMARY KEY, format TEXT NOT NULL, path TEXT NOT NULL, settings_json TEXT NOT NULL, created_at TEXT NOT NULL);
"#, r#"
-- v2 (P10.2): the audio stream analysed and played for a media item (NULL = the best one).
ALTER TABLE media ADD COLUMN audio_stream INTEGER;
"#];

pub fn now() -> String {
    // RFC 3339-like UTC timestamp without external crates.
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, dd) = civil_from_days(days);
    format!("{y:04}-{m:02}-{dd:02}T{:02}:{:02}:{:02}Z", rem / 3600, (rem / 60) % 60, rem % 60)
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// What happened when a project was opened.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenReport {
    /// The previous session didn't close cleanly (crash or kill); the last committed state was kept.
    pub recovered: bool,
    /// Schema version migrated from, if a migration ran (a backup was written first).
    pub migrated_from: Option<i32>,
    pub backup: Option<PathBuf>,
}

pub struct Db {
    pub conn: Connection,
    pub path: PathBuf,
}

fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

fn migrate(conn: &mut Connection, from: i32) -> Result<()> {
    for v in from..SCHEMA_VERSION {
        let tx = conn.transaction()?;
        tx.execute_batch(MIGRATIONS[v as usize])?;
        tx.pragma_update(None, "user_version", v + 1)?;
        tx.commit()?;
    }
    Ok(())
}

impl Db {
    /// Creates a new project file (fails if it exists).
    pub fn create(path: &Path, name: &str) -> Result<Db> {
        if path.exists() {
            return Err(ProjectError::Other(format!("{} already exists", path.display())));
        }
        let mut conn = Connection::open(path)?;
        configure(&conn)?;
        conn.pragma_update(None, "application_id", APPLICATION_ID)?;
        migrate(&mut conn, 0)?;
        let t = now();
        conn.execute(
            "INSERT INTO project(id, name, created_at, modified_at, app_version, clean_shutdown) VALUES (1, ?1, ?2, ?2, ?3, 0)",
            params![name, t, env!("CARGO_PKG_VERSION")],
        )?;
        Ok(Db { conn, path: path.to_path_buf() })
    }

    /// Opens an existing project, migrating (after a backup) and detecting unclean shutdown.
    pub fn open(path: &Path) -> Result<(Db, OpenReport)> {
        if !path.exists() {
            return Err(ProjectError::Other(format!("{} doesn't exist", path.display())));
        }
        let mut report = OpenReport::default();
        let mut conn = Connection::open(path)?;
        let app: i32 = conn.pragma_query_value(None, "application_id", |r| r.get(0))?;
        if app != APPLICATION_ID {
            return Err(ProjectError::NotAProject(path.display().to_string()));
        }
        let ver: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if ver > SCHEMA_VERSION {
            return Err(ProjectError::TooNew(ver));
        }
        configure(&conn)?;
        if ver < SCHEMA_VERSION {
            // Checkpoint the WAL so the copy is complete, then back up.
            conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")?;
            let backup = path.with_extension(format!("clippro.bak-v{ver}"));
            conn.execute("VACUUM INTO ?1", params![backup.to_string_lossy()])?;
            migrate(&mut conn, ver)?;
            report.migrated_from = Some(ver);
            report.backup = Some(backup);
        }
        let has_row: Option<i64> = conn.query_row("SELECT id FROM project WHERE id = 1", [], |r| r.get(0)).optional()?;
        if has_row.is_none() {
            // Files from before the project row existed (schema 0): create it.
            let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Project".into());
            let t = now();
            conn.execute(
                "INSERT INTO project(id, name, created_at, modified_at, app_version, clean_shutdown) VALUES (1, ?1, ?2, ?2, ?3, 1)",
                params![name, t, env!("CARGO_PKG_VERSION")],
            )?;
        }
        let clean: i64 = conn.query_row("SELECT clean_shutdown FROM project WHERE id = 1", [], |r| r.get(0))?;
        report.recovered = clean == 0;
        conn.execute("UPDATE project SET clean_shutdown = 0 WHERE id = 1", [])?;
        Ok((Db { conn, path: path.to_path_buf() }, report))
    }

    /// Marks a clean shutdown and checkpoints the WAL into the main file.
    pub fn close(self) -> Result<()> {
        self.conn.execute("UPDATE project SET clean_shutdown = 1, modified_at = ?1 WHERE id = 1", params![now()])?;
        self.conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")?;
        Ok(())
    }

    /// Writes a consistent copy of the project to `dest` (Save As).
    pub fn save_as(&self, dest: &Path) -> Result<()> {
        if dest.exists() {
            std::fs::remove_file(dest)?;
        }
        self.conn.execute("VACUUM INTO ?1", params![dest.to_string_lossy()])?;
        let c = Connection::open(dest)?;
        c.execute("UPDATE project SET clean_shutdown = 1 WHERE id = 1", [])?;
        Ok(())
    }

    pub fn touch(&self) -> Result<()> {
        self.conn.execute("UPDATE project SET modified_at = ?1 WHERE id = 1", params![now()])?;
        Ok(())
    }

    pub fn name(&self) -> Result<String> {
        Ok(self.conn.query_row("SELECT name FROM project WHERE id = 1", [], |r| r.get(0))?)
    }

    pub fn settings_json(&self) -> Result<String> {
        Ok(self.conn.query_row("SELECT settings_json FROM project WHERE id = 1", [], |r| r.get(0))?)
    }

    pub fn set_settings_json(&self, json: &str) -> Result<()> {
        self.conn.execute("UPDATE project SET settings_json = ?1, modified_at = ?2 WHERE id = 1", params![json, now()])?;
        Ok(())
    }

    pub fn integrity_ok(&self) -> Result<bool> {
        let r: String = self.conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        Ok(r == "ok")
    }

    pub fn schema_version(&self) -> Result<i32> {
        Ok(self.conn.pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    pub fn one<T: rusqlite::types::FromSql>(&self, sql: &str, p: impl rusqlite::Params) -> Result<Option<T>> {
        Ok(self.conn.query_row(sql, p, |r| r.get(0)).optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn tmpdir(name: &str) -> PathBuf {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-project-tests").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn create_reopen_and_markers() {
        let d = tmpdir("create");
        let p = d.join("a.clippro");
        let db = Db::create(&p, "Test").unwrap();
        db.set_settings_json(r#"{"x":1}"#).unwrap();
        db.close().unwrap();
        let (db, rep) = Db::open(&p).unwrap();
        assert!(!rep.recovered);
        assert_eq!(db.name().unwrap(), "Test");
        assert_eq!(db.settings_json().unwrap(), r#"{"x":1}"#);
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        // Drop without close → next open reports recovery.
        drop(db);
        let (db, rep) = Db::open(&p).unwrap();
        assert!(rep.recovered);
        assert!(db.integrity_ok().unwrap());
        let copy = d.join("copy.clippro");
        db.save_as(&copy).unwrap();
        db.close().unwrap();
        let (c, rep) = Db::open(&copy).unwrap();
        assert!(!rep.recovered);
        assert_eq!(c.name().unwrap(), "Test");
        c.close().unwrap();
        std::fs::remove_dir_all(&d).unwrap();
        let _ = std::fs::remove_dir(d.parent().unwrap());
    }

    #[test]
    fn rejects_foreign_files() {
        let d = tmpdir("foreign");
        let p = d.join("x.sqlite");
        Connection::open(&p).unwrap().execute_batch("CREATE TABLE t(a);").unwrap();
        assert!(matches!(Db::open(&p), Err(ProjectError::NotAProject(_))));
        std::fs::remove_dir_all(&d).unwrap();
        let _ = std::fs::remove_dir(d.parent().unwrap());
    }

    #[test]
    fn migrates_v0_with_backup() {
        // A v0 file: CLPR application id but no schema yet.
        let d = tmpdir("migrate");
        let p = d.join("old.clippro");
        {
            let c = Connection::open(&p).unwrap();
            c.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
            c.execute_batch("CREATE TABLE legacy(a); INSERT INTO legacy VALUES (42);").unwrap();
        }
        let (db, rep) = Db::open(&p).unwrap();
        assert_eq!(rep.migrated_from, Some(0));
        let backup = p.with_extension("clippro.bak-v0");
        assert!(backup.exists(), "backup not written");
        assert_eq!(rep.backup.as_deref(), Some(backup.as_path()));
        assert_eq!(db.name().unwrap(), "old");
        db.close().unwrap();
        // The backup still holds the original v0 content.
        let b = Connection::open(&backup).unwrap();
        let bv: i32 = b.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(bv, 0);
        drop(b);
        let c = Connection::open(&p).unwrap();
        let v: i32 = c.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        let legacy: i64 = c.query_row("SELECT a FROM legacy", [], |r| r.get(0)).unwrap();
        assert_eq!(legacy, 42);
        drop(c);
        std::fs::remove_dir_all(&d).unwrap();
        let _ = std::fs::remove_dir(d.parent().unwrap());
    }

    #[test]
    fn dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_000), (2024, 10, 4));
        assert!(now().starts_with("20"));
    }
}
