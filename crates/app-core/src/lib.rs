//! App core: state + services shared by the desktop shell.

pub mod playback_engine;
pub mod playback_types;
pub mod state;
pub mod watcher;
pub mod yt_handle;

pub use state::{AppState, CoreError};
