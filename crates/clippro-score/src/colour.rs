//! Log-profile → display transforms used by score-time exposure measures.
//!
//! P6.5 replaces this with vendor-published curves. Until then every profile maps through
//! unchanged (recorded in docs/PROGRESS.md "Deviations to resolve").

use clippro_types::ColourProfile;

/// Maps a stored (encoded) luma value to an approximate display-referred Rec.709 value.
pub fn to_display(_profile: ColourProfile, v: f32) -> f32 {
    v
}
