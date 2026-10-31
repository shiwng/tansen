//! Playback crate: audio media pipeline + queue persistence primitives.
//!
//! M10 removed the legacy rodio engine (`engine.rs` + `queue.rs`): playback
//! state now lives in the app-core task engine (`app_core::playback_engine`),
//! which drives this crate's `pipeline` through the `PipelineFactory` seam.
//! The stream opener that used to live in engine.rs is part of pipeline now.

pub mod cache;
pub mod metrics;
pub mod pipeline;
pub mod probe;
pub mod remote;

pub use cache::StreamCache;
