//! App core: state + services shared by the desktop shell.

pub mod history;
pub mod media;
pub mod persist_state;
pub mod state;
pub mod watcher;

pub use state::{AppState, CoreError};

// MediaControlEvent is part of the public surface (shell wires commands).
pub use souvlaki::{MediaControlEvent, SeekDirection};
