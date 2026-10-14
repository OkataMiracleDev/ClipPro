//! Analysis pipeline: per-clip measurement, sections, scheduling and checkpointing (SPEC §10).

pub mod clip;

pub use clip::{analyze_clip, AnalyzeError, AnalyzeOptions};
