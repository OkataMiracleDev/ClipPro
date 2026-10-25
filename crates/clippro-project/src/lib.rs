//! `.clippro` project storage (SPEC §12): SQLite schema, migrations, crash recovery,
//! typed operations and the global measurement cache.

pub mod db;
pub mod store;

pub use db::{Db, OpenReport, ProjectError, Result};
pub use store::{ChunkRow, ClipRow, MeasureCache, MediaRow, NewMedia, OverrideRow, RunParams, RunRow, RunState};
