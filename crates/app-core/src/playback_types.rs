//! Normalized playback types (M5): the vocabulary for playback state and
//! commands, pure and dependency-free — no I/O, no Tauri, no serde (nothing
//! on the wire uses these yet; derive serde only when an IPC consumer
//! exists).
//!
//! The model: a queue holds *entries* addressed by [`EntryId`] (an id, never
//! a positional index — indices shift under edits, ids do not). Playback
//! progress is an [`Anchor`] (position + rate), not a wall clock. [`Status`]
//! is the lifecycle the engine walks; [`validate_transition`] is the pure
//! law for walking it.

/// Identity of one queue entry. Deliberately not an index: queue edits
/// renumber positions, never identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntryId(pub u64);

/// The class of a playback failure, mirroring the engine's existing load /
/// append error paths: no output device (`Device`), an unresolved
/// `sidecar://` placeholder (`Resolve`), a decoder that would not open
/// (`Decode`), or a filesystem error (`Io`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Device,
    Resolve,
    Decode,
    Io,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Nothing loaded.
    Idle,
    /// A source is being resolved (sidecar/catalogue round trip).
    Resolving,
    /// Resolved; the decoder is opening / the buffer is filling.
    Buffering,
    /// Audible.
    Playing,
    /// Loaded but silent by request.
    Paused,
    /// The load or playback path failed; `ErrorKind` says where.
    Error(ErrorKind),
}

impl Status {
    /// The status-graph edges the engine may walk. Anything else is a bug.
    fn can_follow(self, from: Status) -> bool {
        use Status::*;
        match from {
            // Start: a local file can go straight to Playing; a remote one
            // resolves first.
            Idle => matches!(self, Resolving | Playing),
            Resolving => matches!(self, Buffering | Playing | Error(_) | Idle),
            Buffering => matches!(self, Playing | Error(_) | Idle),
            Playing => matches!(self, Paused | Buffering | Resolving | Idle | Error(_)),
            Paused => matches!(self, Playing | Resolving | Idle | Error(_)),
            Error(_) => matches!(self, Resolving | Playing | Idle),
        }
    }

    /// Playing is the one status whose clock runs.
    fn runs_clock(self) -> bool {
        matches!(self, Status::Playing)
    }
}

/// Playback progress: a position in milliseconds and the rate the clock runs
/// at (0 = stopped clock, 1 = real time). A speed-control future widens the
/// rate domain; today it is exactly 0 or 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    pub pos_ms: u64,
    pub rate: u8,
}

impl Anchor {
    pub fn paused(pos_ms: u64) -> Self {
        Self { pos_ms, rate: 0 }
    }

    pub fn playing(pos_ms: u64) -> Self {
        Self { pos_ms, rate: 1 }
    }
}

/// The full playback state the engine owns and everyone else reads.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackState {
    /// Bumped on every accepted transition; readers use it to drop stale
    /// renders instead of diffing fields.
    pub rev: u64,
    pub status: Status,
    pub entry: Option<EntryId>,
    /// Revision of the queue this playback state belongs to.
    pub queue_rev: u64,
    pub anchor: Anchor,
    /// 0.0 – 1.0.
    pub volume: f32,
}

/// A shape error: the state itself breaks a law (bad rate, bad volume),
/// independent of any transition.
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeError {
    RateNotZero { status: Status, rate: u8 },
    PlayingWithoutRate { rate: u8 },
    VolumeOutOfRange(f32),
}

impl PlaybackState {
    /// The state is internally consistent: the anchor's rate agrees with the
    /// status and the volume is in range.
    pub fn validate(&self) -> Result<(), ShapeError> {
        if self.status.runs_clock() {
            if self.anchor.rate != 1 {
                return Err(ShapeError::PlayingWithoutRate { rate: self.anchor.rate });
            }
        } else if self.anchor.rate != 0 {
            return Err(ShapeError::RateNotZero { status: self.status, rate: self.anchor.rate });
        }
        if !(0.0..=1.0).contains(&self.volume) {
            return Err(ShapeError::VolumeOutOfRange(self.volume));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Right after the current entry.
    Next,
    /// End of the queue.
    Append,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Make this entry the current one (load + start).
    Play { entry: EntryId },
    Pause,
    Resume,
    Next,
    Prev,
    /// Jump the anchor; the clock keeps its current rate.
    Seek { ms: u64 },
    SetVolume(f32),
    /// Add entries to the queue; `queue_rev` bumps, playback does not change.
    Enqueue { tracks: Vec<EntryId>, placement: Placement },
}

/// Why a transition was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum TransitionError {
    /// The status graph has no such edge.
    IllegalStatus { from: Status, to: Status },
    /// A state in the pair is internally malformed.
    Shape(ShapeError),
    /// Revisions must move forward.
    RevNotIncreasing { from: u64, to: u64 },
    // Position-regression checks need engine context; only `rev` monotonicity
    // is enforced here today.
}

/// The pure law for one playback-state change: both states must be
/// well-formed, `rev` must advance, and the status edge must exist on the
/// engine's graph. Notably it enforces the anchor rule — **the clock runs
/// (rate 1) only while `Playing`** — for both endpoints, since a transition
/// into or out of a malformed state is itself malformed.
pub fn validate_transition(
    from: &PlaybackState,
    to: &PlaybackState,
) -> Result<(), TransitionError> {
    from.validate().map_err(TransitionError::Shape)?;
    to.validate().map_err(TransitionError::Shape)?;
    if to.rev <= from.rev {
        return Err(TransitionError::RevNotIncreasing { from: from.rev, to: to.rev });
    }
    if to.status == from.status {
        return Err(TransitionError::IllegalStatus { from: from.status, to: to.status });
    }
    if !to.status.can_follow(from.status) {
        return Err(TransitionError::IllegalStatus { from: from.status, to: to.status });
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    fn state(rev: u64, status: Status, pos_ms: u64) -> PlaybackState {
        PlaybackState {
            rev,
            status,
            entry: Some(EntryId(7)),
            queue_rev: 3,
            anchor: Anchor {
                pos_ms,
                rate: if status.runs_clock() { 1 } else { 0 },
            },
            volume: 0.8,
        }
    }

    #[test]
    fn the_happy_walk_is_legal() {
        // idle → resolving → buffering → playing → paused → playing → idle
        let steps = [
            (Status::Idle, Status::Resolving),
            (Status::Resolving, Status::Buffering),
            (Status::Buffering, Status::Playing),
            (Status::Playing, Status::Paused),
            (Status::Paused, Status::Playing),
            (Status::Playing, Status::Idle),
        ];
        let mut rev = 1;
        let mut cur = state(rev, Status::Idle, 0);
        for (from, to) in steps {
            rev += 1;
            let next = state(rev, to, 1234);
            assert_eq!(validate_transition(&cur, &next), Ok(()), "{from:?} → {to:?}");
            cur = next;
        }
    }

    #[test]
    fn error_recovery_edges_are_legal() {
        // resolving → error → resolving (retry), and error → idle (give up)
        let bad = state(10, Status::Error(ErrorKind::Resolve), 0);
        let retry = state(11, Status::Resolving, 0);
        assert_eq!(validate_transition(&bad, &retry), Ok(()));
        let gave_up = state(12, Status::Idle, 0);
        assert_eq!(validate_transition(&retry, &gave_up), Ok(()));
        // a playing track can fail into Error
        let playing = state(20, Status::Playing, 5000);
        let failed = state(21, Status::Error(ErrorKind::Decode), 5000);
        assert_eq!(validate_transition(&playing, &failed), Ok(()));
    }

    #[test]
    fn illegal_status_edges_are_refused() {
        let cases = [
            (Status::Idle, Status::Paused),        // nothing is loaded
            (Status::Idle, Status::Buffering),     // must resolve or play first
            (Status::Resolving, Status::Paused),   // not started yet
            (Status::Buffering, Status::Resolving),// backwards
            (Status::Paused, Status::Buffering),   // resume, don't re-buffer
            (Status::Error(ErrorKind::Io), Status::Paused), // recover first
        ];
        for (from_s, to_s) in cases {
            let from = state(1, from_s, 0);
            let to = state(2, to_s, 0);
            assert!(
                matches!(validate_transition(&from, &to), Err(TransitionError::IllegalStatus { .. })),
                "expected {from_s:?} → {to_s:?} refused"
            );
        }
    }

    #[test]
    fn same_status_is_not_a_transition() {
        let a = state(1, Status::Playing, 100);
        let b = state(2, Status::Playing, 200);
        assert!(matches!(validate_transition(&a, &b), Err(TransitionError::IllegalStatus { .. })));
    }

    #[test]
    fn the_clock_runs_only_while_playing() {
        // Playing must carry rate 1.
        let mut playing = state(1, Status::Playing, 0);
        playing.anchor.rate = 0;
        let next = state(2, Status::Paused, 0);
        assert!(matches!(
            validate_transition(&playing, &next),
            Err(TransitionError::Shape(ShapeError::PlayingWithoutRate { rate: 0 }))
        ));

        // Nothing else may carry rate 1.
        let mut paused = state(1, Status::Paused, 0);
        paused.anchor.rate = 1;
        let next = state(2, Status::Playing, 0);
        assert!(matches!(
            validate_transition(&paused, &next),
            Err(TransitionError::Shape(ShapeError::RateNotZero { .. }))
        ));

        // Entering Playing with rate 0 is refused at the destination too.
        let paused_ok = state(1, Status::Paused, 0);
        let mut to = state(2, Status::Playing, 0);
        to.anchor.rate = 0;
        assert!(matches!(
            validate_transition(&paused_ok, &to),
            Err(TransitionError::Shape(ShapeError::PlayingWithoutRate { rate: 0 }))
        ));
    }

    #[test]
    fn revisions_must_advance_and_volume_must_be_a_gain() {
        let from = state(5, Status::Playing, 0);
        let stale = state(5, Status::Paused, 0);
        assert!(matches!(
            validate_transition(&from, &stale),
            Err(TransitionError::RevNotIncreasing { from: 5, to: 5 })
        ));

        let mut loud = state(1, Status::Playing, 0);
        loud.volume = 1.5;
        assert!(matches!(loud.validate(), Err(ShapeError::VolumeOutOfRange(v)) if v == 1.5));
    }
}
