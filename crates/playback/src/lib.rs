//! Playback crate: audio media pipeline + queue persistence primitives.
//!
//! The task engine replaced the legacy rodio engine (`engine.rs` +
//! `queue.rs`): playback state now lives in `app_core::playback_engine`,
//! which drives this crate's `pipeline` through the `PipelineFactory` seam.
//! The stream opener that used to live in engine.rs is part of pipeline now.

pub mod cache;
pub mod dsp;
pub mod loudness;
pub mod metrics;
pub mod pipeline;
pub mod probe;
pub mod remote;
pub mod ring;

pub use cache::StreamCache;
