//! Application core shared by the CLI and the desktop app: project sessions, import,
//! analysis orchestration and scoring.

pub mod import;
pub mod inspect;
pub mod report;
pub mod session;
pub mod store;

pub use inspect::Inspection;
pub use session::{ClipStatus, ImportReport, Session, SessionError, SessionOptions};
