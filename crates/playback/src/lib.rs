//! Playback crate: audio engine + pure queue model.
//!
//! Deliberately small for phase 3: local file playback, transport, volume,
//! seek, gapless advance. Crossfade (two-sink mixing), speed, skip-silence and
//! the DSP chain land in phase 6 as sinks wrapped around the same decoder.

pub mod cache;
pub mod engine;
pub mod metrics;
pub mod probe;
pub mod queue;
pub mod remote;

pub use cache::StreamCache;
pub use engine::{Command, Engine, EngineEvent};
pub use queue::{Queue, QueueOp, QueueSnapshot, Repeat};
