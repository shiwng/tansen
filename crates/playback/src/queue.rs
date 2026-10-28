//! Pure queue state — no audio, fully unit-testable.
//!
//! The engine owns one `Queue` and applies `QueueOp`s to it; transition rules
//! (repeat modes, history for previous) live here so playback bugs can be
//! reproduced without an audio device.

use persistence::models::TrackRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

/// One edit to the queue. Applied by [`Queue::apply`], echoed back as the new
/// snapshot so the UI and the engine can never disagree about the order.
#[derive(Debug, Clone)]
pub enum QueueOp {
    /// Replace the whole queue and jump to `index`. `play` governs whether
    /// the engine should start sound immediately.
    Load { tracks: Vec<TrackRef>, index: usize, play: bool },
    /// Append tracks after the current position (play-next semantics when
    /// `at` is `Some(index)`).
    Insert { tracks: Vec<TrackRef>, at: Option<usize> },
    RemoveAt(usize),
    /// Reorder: move the entry at `from` to `to` (queue drag).
    Move { from: usize, to: usize },
    Clear,
    Jump(usize),
    Next,
    Prev,
    SetRepeat(Repeat),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct QueueSnapshot {
    pub tracks: Vec<TrackRef>,
    pub index: Option<usize>,
    pub repeat: Repeat,
    pub playing: bool,
    /// True when the queue has nothing left to play after the current track.
    pub at_end: bool,
}

#[derive(Debug, Default)]
pub struct Queue {
    tracks: Vec<TrackRef>,
    /// Index of the current track; `None` when nothing is loaded.
    index: Option<usize>,
    repeat: Repeat,
    /// Previous positions, for Prev behaviour that reverses Next.
    history: Vec<usize>,
}

/// Outcome the engine must act on after applying an op.
#[derive(Debug, Clone)]
pub struct QueueEffect {
    /// Track that must now be audible, if any.
    pub load: Option<(TrackRef, bool)>,
    /// True when the queue order changed and the UI must refresh.
    pub changed: bool,
}

impl Queue {
    pub fn snapshot(&self, playing: bool) -> QueueSnapshot {
        QueueSnapshot {
            tracks: self.tracks.clone(),
            index: self.index,
            repeat: self.repeat,
            playing,
            at_end: self.next_index().is_none(),
        }
    }

    pub fn current(&self) -> Option<&TrackRef> {
        self.index.and_then(|i| self.tracks.get(i))
    }

    /// The track `Next` would play, honouring repeat modes. `None` = stop.
    pub fn next_track(&self) -> Option<&TrackRef> {
        self.next_index().and_then(|i| self.tracks.get(i))
    }

    pub fn repeat(&self) -> Repeat {
        self.repeat
    }

    /// Where `Next` lands, honouring repeat modes. `None` means stop.
    fn next_index(&self) -> Option<usize> {
        let i = self.index?;
        match self.repeat {
            Repeat::One => Some(i),
            _ => {
                if i + 1 < self.tracks.len() {
                    Some(i + 1)
                } else if self.repeat == Repeat::All && !self.tracks.is_empty() {
                    Some(0)
                } else {
                    None
                }
            }
        }
    }

    /// Where `Prev` lands: the last visited position if it is the immediate
    /// neighbour, otherwise the restart of the current track (standard
    /// desktop-player behaviour).
    fn prev_index(&self) -> Option<usize> {
        let i = self.index?;
        if let Some(&last) = self.history.last() {
            if last != i {
                return Some(last);
            }
        }
        Some(i)
    }

    /// Applies an op, returning what the engine should do.
    pub fn apply(&mut self, op: QueueOp) -> QueueEffect {
        match op {
            QueueOp::Load { tracks, index, play } => {
                let load = tracks.get(index).cloned().map(|t| (t, play));
                self.tracks = tracks;
                self.index = (index < self.tracks.len()).then_some(index);
                self.history.clear();
                QueueEffect { load, changed: true }
            }
            QueueOp::Insert { tracks, at } => {
                let at = at.unwrap_or(self.index.map(|i| i + 1).unwrap_or(0));
                let at = at.min(self.tracks.len());
                // The current track must stay the current track; indexes past
                // the insertion point shift right.
                if let Some(i) = self.index {
                    if at <= i {
                        self.index = Some(i + tracks.len());
                        for h in self.history.iter_mut() {
                            if *h >= at {
                                *h += tracks.len();
                            }
                        }
                    }
                }
                self.tracks.splice(at..at, tracks);
                QueueEffect { load: None, changed: true }
            }
            QueueOp::RemoveAt(pos) => {
                if pos >= self.tracks.len() {
                    return QueueEffect { load: None, changed: false };
                }
                self.tracks.remove(pos);
                match self.index {
                    Some(i) if pos < i => self.index = Some(i - 1),
                    Some(i) if pos == i => {
                        self.index = if self.tracks.is_empty() {
                            None
                        } else {
                            Some(i.min(self.tracks.len() - 1))
                        };
                        let load = self.current().cloned().map(|t| (t, false));
                        return QueueEffect { load, changed: true };
                    }
                    _ => {}
                }
                QueueEffect { load: None, changed: true }
            }
            QueueOp::Move { from, to } => {
                if from >= self.tracks.len() || to >= self.tracks.len() || from == to {
                    return QueueEffect { load: None, changed: false };
                }
                let moved = self.tracks.remove(from);
                self.tracks.insert(to, moved);
                if let Some(i) = self.index {
                    self.index = Some(if from == i {
                        to
                    } else if from < i && to >= i {
                        i - 1
                    } else if from > i && to <= i {
                        i + 1
                    } else {
                        i
                    });
                }
                QueueEffect { load: None, changed: true }
            }
            QueueOp::Clear => {
                let had = !self.tracks.is_empty();
                self.tracks.clear();
                self.index = None;
                self.history.clear();
                QueueEffect { load: None, changed: had }
            }
            QueueOp::Jump(i) => {
                if i >= self.tracks.len() {
                    return QueueEffect { load: None, changed: false };
                }
                self.goto(i)
            }
            QueueOp::Next => match self.next_index() {
                Some(i) => self.goto(i),
                None => {
                    self.index = None;
                    QueueEffect { load: None, changed: false }
                }
            },
            QueueOp::Prev => match self.prev_index() {
                Some(i) => self.goto(i),
                None => QueueEffect { load: None, changed: false },
            },
            QueueOp::SetRepeat(r) => {
                self.repeat = r;
                QueueEffect { load: None, changed: true }
            }
        }
    }

    /// Replaces a queued track's path (remote resolve-ahead). Not a queue
    /// edit the UI needs to hear about: order, index and identity stay.
    pub fn patch_path(&mut self, index: usize, path: &str) {
        if let Some(track) = self.tracks.get_mut(index) {
            track.path = path.to_string();
        }
    }

    /// Moves playback to `i`, remembering the old position.
    fn goto(&mut self, i: usize) -> QueueEffect {
        if let Some(prev) = self.index {
            self.history.push(prev);
            if self.history.len() > 128 {
                self.history.remove(0);
            }
        }
        self.index = Some(i);
        let load = self.current().cloned().map(|t| (t, true));
        QueueEffect { load, changed: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: i64) -> TrackRef {
        TrackRef {
            id,
            title: format!("T{id}"),
            artist: "A".into(),
            album: "AL".into(),
            duration_sec: 60.0,
            art_path: None,
            path: format!("/t/{id}.flac"),
        }
    }

    fn q3() -> Queue {
        let mut q = Queue::default();
        q.apply(QueueOp::Load { tracks: vec![track(1), track(2), track(3)], index: 0, play: true });
        q
    }

    #[test]
    fn load_and_walk() {
        let mut q = q3();
        assert_eq!(q.current().unwrap().id, 1);
        assert!(q.apply(QueueOp::Next).load.is_some());
        assert_eq!(q.current().unwrap().id, 2);
        // Prev reverses Next exactly.
        let e = q.apply(QueueOp::Prev);
        assert_eq!(e.load.unwrap().0.id, 1);
    }

    #[test]
    fn prev_at_start_restarts_track() {
        let mut q = q3();
        let e = q.apply(QueueOp::Prev);
        // history is empty at index 0, so Prev restarts current.
        assert_eq!(e.load.unwrap().0.id, 1);
    }

    #[test]
    fn end_of_queue_stops_unless_repeat_all() {
        let mut q = q3();
        q.apply(QueueOp::Next);
        q.apply(QueueOp::Next);
        let e = q.apply(QueueOp::Next);
        assert!(e.load.is_none());
        assert!(q.current().is_none());

        let mut q = q3();
        q.apply(QueueOp::SetRepeat(Repeat::All));
        q.apply(QueueOp::Next);
        q.apply(QueueOp::Next);
        assert_eq!(q.apply(QueueOp::Next).load.unwrap().0.id, 1);
    }

    #[test]
    fn repeat_one_stays() {
        let mut q = q3();
        q.apply(QueueOp::SetRepeat(Repeat::One));
        assert_eq!(q.apply(QueueOp::Next).load.unwrap().0.id, 1);
    }

    #[test]
    fn insert_after_current_keeps_current() {
        let mut q = q3();
        let e = q.apply(QueueOp::Insert { tracks: vec![track(9)], at: Some(1) });
        assert!(e.changed);
        assert_eq!(q.current().unwrap().id, 1);
        assert_eq!(q.apply(QueueOp::Next).load.unwrap().0.id, 9);
        assert_eq!(q.apply(QueueOp::Next).load.unwrap().0.id, 2);
    }

    #[test]
    fn remove_current_loads_neighbour() {
        let mut q = q3();
        q.apply(QueueOp::Next); // at 2
        let e = q.apply(QueueOp::RemoveAt(1));
        assert_eq!(e.load.unwrap().0.id, 3);
        assert_eq!(q.snapshot(false).index, Some(1));
    }

    #[test]
    fn clear_drops_everything() {
        let mut q = q3();
        let e = q.apply(QueueOp::Clear);
        assert!(e.changed && e.load.is_none());
        assert_eq!(q.snapshot(false).index, None);
        assert!(q.snapshot(false).tracks.is_empty());
    }

    #[test]
    fn move_reorders_and_tracks_current() {
        let mut q = q3();
        // Move track 3 to the front: current (index 0, track 1) shifts right.
        let e = q.apply(QueueOp::Move { from: 2, to: 0 });
        assert!(e.changed);
        let snap = q.snapshot(false);
        assert_eq!(snap.tracks[0].id, 3);
        assert_eq!(snap.index, Some(1), "current entry follows the move");
        assert_eq!(q.current().unwrap().id, 1);

        // Move the current track itself.
        q.apply(QueueOp::Move { from: 1, to: 2 });
        assert_eq!(q.snapshot(false).index, Some(2));

        // Out-of-range moves are no-ops.
        let e = q.apply(QueueOp::Move { from: 5, to: 0 });
        assert!(!e.changed);
    }

    #[test]
    fn jump_is_recorded_in_history() {
        let mut q = q3();
        q.apply(QueueOp::Jump(2));
        assert_eq!(q.apply(QueueOp::Prev).load.unwrap().0.id, 1);
    }
}
