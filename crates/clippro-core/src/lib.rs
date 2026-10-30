//! Application core shared by the CLI and the desktop app: project sessions, import,
//! analysis orchestration and scoring.

pub mod annotate;
pub mod import;
pub mod inspect;
pub mod multicam;
pub mod report;
pub mod review;
pub mod watch;
pub mod session;
pub mod store;

pub use inspect::Inspection;
pub use session::{ClipStatus, ImportReport, Session, SessionError, SessionOptions};
