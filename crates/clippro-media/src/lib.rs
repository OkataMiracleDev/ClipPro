//! Media layer: FFmpeg probe, sampled video decode (CPU + D3D11VA), audio decode, metadata
//! and fingerprints (SPEC §9).

pub mod audio;
pub mod ffi;
pub mod fingerprint;
pub mod probe;
pub mod video;

pub use ffi::{MediaError, Result};
pub use probe::{probe, AudioInfo, MediaInfo, VideoInfo};
pub use video::{sample_plan, DecodeOptions, FrameView, HwMode, SampleFrames, VideoDecoder};
