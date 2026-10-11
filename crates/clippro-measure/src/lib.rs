//! Per-sample measurement kernels (CPU reference implementation) and the temporal pass.
//! Every function here implements a section of docs/ALGORITHMS.md; see the doc comments.

pub mod calibration;
pub mod columns;
pub mod image;
pub mod motion;
pub mod sample;
pub mod spatial;
pub mod temporal;
pub mod util;

/// Version of the measurement maths. Bump whenever ALGORITHMS.md §0–§5 changes.
pub const ENGINE_VERSION: u32 = 1;
