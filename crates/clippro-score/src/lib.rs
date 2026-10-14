//! Pure scoring and segmentation engine (SPEC §11, ALGORITHMS §6–7). No IO, no global state.

pub mod colour;
pub mod engine;
pub mod frame;
pub mod normalize;
pub mod preset;
pub mod segments;
pub mod settings;
pub mod submetric;

pub use engine::{ClipSelects, Engine, SelectRef, Selects, SortOrder, Summary};
pub use normalize::ClipInput;
pub use preset::{builtin, builtins, Preset};
pub use segments::{Override, OverrideKind, Segment, SegmentKind};
pub use settings::Settings;
pub use submetric::SubMetric;
