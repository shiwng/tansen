//! OS media-key integration (macOS Now Playing, Windows SMTC, Linux MPRIS).
//!
//! Created on the main thread (souvlaki's macOS backend requires it) and then
//! updated from any thread. All failures degrade to "no media keys" — the
//! player must never refuse to start because the OS integration is unhappy.

use persistence::models::TrackRef;
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type CommandFn = Arc<dyn Fn(MediaControlEvent) + Send + Sync>;

pub struct Media {
    inner: Mutex<Option<MediaControls>>,
    /// Freshest known position (seconds) fed by the event forwarder.
    position: Mutex<f64>,
    playing: Mutex<bool>,
    last_push: Mutex<Option<Instant>>,
    on_command: Mutex<Option<CommandFn>>,
}

impl Media {
    /// Attach OS controls; `on_command` receives play/pause/next/etc. Must be
    /// called on the main thread on macOS.
    pub fn attach(&self, on_command: impl Fn(MediaControlEvent) + Send + Sync + 'static) {
        let config = PlatformConfig {
            dbus_name: "bitchord",
            display_name: "BitChord Desktop",
            hwnd: None,
        };
        match MediaControls::new(config) {
            Ok(mut controls) => {
                let cmd: CommandFn = Arc::new(on_command);
                if controls
                    .attach({
                        let cmd = cmd.clone();
                        move |ev| cmd(ev)
                    })
                    .is_ok()
                {
                    *self.on_command.lock().unwrap() = Some(cmd);
                    *self.inner.lock().unwrap() = Some(controls);
                } else {
                    tracing::warn!("media controls attach failed");
                }
            }
            Err(e) => tracing::warn!(error = %e, "media controls unavailable"),
        }
    }

    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
            position: Mutex::new(0.0),
            playing: Mutex::new(false),
            last_push: Mutex::new(None),
            on_command: Mutex::new(None),
        }
    }

    pub fn update_track(&self, track: Option<&TrackRef>) {
        match track {
            Some(t) => {
                if let Some(c) = self.inner.lock().unwrap().as_mut() {
                    let _ = c.set_metadata(MediaMetadata {
                        title: Some(&t.title),
                        artist: Some(&t.artist),
                        album: Some(&t.album),
                        duration: (t.duration_sec > 0.0).then(|| Duration::from_secs_f64(t.duration_sec)),
                        cover_url: None,
                    });
                }
            }
            None => {
                if let Some(c) = self.inner.lock().unwrap().as_mut() {
                    let _ = c.set_playback(MediaPlayback::Stopped);
                }
            }
        }
    }

    pub fn set_position(&self, sec: f64) {
        *self.position.lock().unwrap() = sec;
    }

    /// Latest known playback position, for relative-seek media-key events.
    pub fn position(&self) -> f64 {
        *self.position.lock().unwrap()
    }

    pub fn set_playing(&self, playing: bool) {
        *self.playing.lock().unwrap() = playing;
        self.push_playback();
    }

    pub fn update_playback(&self, playing: bool, position: Option<f64>) {
        let was_playing = *self.playing.lock().unwrap();
        *self.playing.lock().unwrap() = playing;
        if let Some(p) = position {
            *self.position.lock().unwrap() = p;
        }
        // OS playback state is pushed at 2 Hz or on a play/pause flip; the
        // engine ticks at 10 Hz and set_playback is not free.
        let mut last = self.last_push.lock().unwrap();
        let due = playing != was_playing || last.map(|t| t.elapsed() >= Duration::from_millis(500)).unwrap_or(true);
        if due {
            *last = Some(Instant::now());
            drop(last);
            self.push_playback();
        }
    }

    fn push_playback(&self) {
        let playing = *self.playing.lock().unwrap();
        let pos = Duration::from_secs_f64(self.position.lock().unwrap().max(0.0));
        if let Some(c) = self.inner.lock().unwrap().as_mut() {
            let state = if playing {
                MediaPlayback::Playing { progress: Some(MediaPosition(pos)) }
            } else {
                MediaPlayback::Paused { progress: Some(MediaPosition(pos)) }
            };
            let _ = c.set_playback(state);
        }
    }
}

impl Default for Media {
    fn default() -> Self {
        Self::new()
    }
}
