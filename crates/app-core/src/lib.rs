//! App core: state + services shared by the desktop shell.

pub mod media;
pub mod playback_engine;
pub mod playback_types;
pub mod state;
pub mod yt_handle;
pub mod watcher;

pub use state::{AppState, CoreError};

// MediaControlEvent is part of the public surface (shell wires commands).
pub use souvlaki::{MediaControlEvent, SeekDirection};
