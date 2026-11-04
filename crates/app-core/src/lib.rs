//! App core: state + services shared by the desktop shell.

pub mod playback_engine;
pub mod playback_types;
pub mod state;
pub mod yt_handle;
pub mod watcher;

pub use state::{AppState, CoreError};
