//! Analysis pipeline: per-clip measurement, sections, scheduling and checkpointing (SPEC §10).

pub mod clip;
pub mod jobs;

pub use clip::{analyze_clip, AnalyzeError, AnalyzeOptions};
pub use jobs::{Event, JobId, JobSpec, JobStatus, MemStore, RunStore, Scheduler, SchedulerConfig};
