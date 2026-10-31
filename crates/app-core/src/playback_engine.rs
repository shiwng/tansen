//! The task-based playback engine (M6): one task owns the [`PlaybackState`],
//! commands arrive over a bounded mpsc, state changes publish on a
//! `tokio::sync::watch` channel. Pure orchestration — **no real audio**: all
//! media work goes through the one seam, [`PipelineFactory`], whose test
//! implementation is a fake.
//!
//! Laws (all enforced by `playback_types` + asserted in tests):
//! - `rev` increments by one on every published change (multiple publishes
//!   per command are allowed: Play walks Resolving → Buffering → Playing).
//! - Status changes always follow the legal edge graph; refused commands
//!   publish nothing.
//! - The anchor rate is 1 only while `Playing`, 0 otherwise.

use crate::playback_types::{Anchor, Command, EntryId, ErrorKind, PlaybackState, Placement, Status};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};

/// The only seam between the playback task and the outside world. The real
/// implementation (future milestone) wraps the audio engine; tests install a
/// fake. No other I/O happens in this module.
pub trait PipelineFactory: Send + Sync + 'static {
    /// Resolve + open media for this entry. `Ok(())` means the pipeline is
    /// ready to be audible; the `ErrorKind` says which stage failed.
    fn prepare(&self, entry: EntryId) -> Result<(), ErrorKind>;

    /// The entry stops being current: tear its pipeline down.
    fn release(&self, entry: EntryId);
}

/// Handle to the playback task. Cheap to clone; the task dies when the
/// sender side is dropped everywhere.
#[derive(Clone)]
pub struct Engine {
    tx: mpsc::Sender<Command>,
    state_rx: watch::Receiver<PlaybackState>,
    journal: Arc<Mutex<VecDeque<PlaybackState>>>,
}

/// Command-queue depth. Backpressure, not drops: senders await when full.
const COMMAND_BUFFER: usize = 64;
/// Published-state history kept for observation (tests, debugging). The
/// oldest entries fall off; `watch` still carries the newest state.
const JOURNAL_CAP: usize = 4096;

impl Engine {
    /// Spawns the playback task on the current tokio runtime. The task is
    /// the sole owner of the state; callers only ever see published copies.
    pub fn start(factory: Arc<dyn PipelineFactory>) -> Self {
        let initial = PlaybackState {
            rev: 0,
            status: Status::Idle,
            entry: None,
            queue_rev: 0,
            anchor: Anchor::paused(0),
            volume: 1.0,
        };
        let (tx, cmd_rx) = mpsc::channel(COMMAND_BUFFER);
        let (state_tx, state_rx) = watch::channel(initial.clone());
        let journal = Arc::new(Mutex::new(VecDeque::from([initial.clone()])));
        tokio::spawn(run(factory, cmd_rx, state_tx, initial, journal.clone()));
        Self { tx, state_rx, journal }
    }

    /// Queue a command. Awaits under backpressure; the bounded channel never
    /// drops commands.
    pub async fn send(&self, command: Command) {
        let _ = self.tx.send(command).await;
    }

    /// Subscribe to published playback states.
    pub fn subscribe(&self) -> watch::Receiver<PlaybackState> {
        self.state_rx.clone()
    }

    /// The full publish history (bounded; oldest first). Exists because
    /// `watch` coalesces — an observer can miss intermediate states, the
    /// journal cannot.
    pub fn journal(&self) -> Vec<PlaybackState> {
        self.journal.lock().map(|j| j.iter().cloned().collect()).unwrap_or_default()
    }

    /// Waits until no new state has been published for a full poll interval.
    /// Test convenience for "the task drained its queue".
    pub async fn quiesce(&self, poll_ms: u64, max_ms: u64) {
        let mut last = self.state_rx.borrow().rev;
        let mut waited = 0u64;
        while waited < max_ms {
            tokio::time::sleep(std::time::Duration::from_millis(poll_ms)).await;
            let now = self.state_rx.borrow().rev;
            if now == last {
                return;
            }
            last = now;
            waited += poll_ms;
        }
    }
}

/// The task's private world. Nothing else touches it.
struct Core {
    state: PlaybackState,
    /// Queue storage in play order. Addressing is still by `EntryId` — this
    /// Vec is the order, not the addressing scheme.
    queue: Vec<EntryId>,
    /// The entry the queue cursor points at (`None` before the first Play).
    cursor: Option<EntryId>,
}

impl Core {
    /// Applies one command and returns every state to publish, in order, rev
    /// already advanced. Refused commands return an empty vec.
    fn apply(&mut self, factory: &dyn PipelineFactory, cmd: Command) -> Vec<PlaybackState> {
        let mut published = Vec::new();
        let mut push = |core: &mut Self, status: Option<Status>, entry: Option<Option<EntryId>>, pos_ms: Option<u64>, volume: Option<f32>, queue_bump: bool| {
            core.state.rev += 1;
            if let Some(s) = status {
                core.state.status = s;
            }
            if let Some(e) = entry {
                core.state.entry = e;
            }
            if let Some(p) = pos_ms {
                core.state.anchor.pos_ms = p;
            }
            // The rate law holds at every publish, not only on seeks: leaving
            // the previous rate in place across a status change once shipped
            // `Resolving` with `rate: 1` (caught by the random tests).
            core.state.anchor.rate = if core.state.status.runs_clock() { 1 } else { 0 };
            if let Some(v) = volume {
                core.state.volume = v;
            }
            if queue_bump {
                core.state.queue_rev += 1;
            }
            published.push(core.state.clone());
        };

        match cmd {
            Command::Play { entry } => {
                if self.state.status == Status::Resolving {
                    // Resolving → Resolving is not a legal edge: the current
                    // resolve must land first.
                    return published;
                }
                if let Some(prev) = self.cursor {
                    if prev != entry {
                        factory.release(prev);
                    }
                }
                self.cursor = Some(entry);
                push(self, Some(Status::Resolving), Some(Some(entry)), None, None, false);
                match factory.prepare(entry) {
                    Ok(()) => {
                        push(self, Some(Status::Buffering), None, None, None, false);
                        push(self, Some(Status::Playing), None, Some(self.state.anchor.pos_ms), None, false);
                    }
                    Err(kind) => push(self, Some(Status::Error(kind)), None, None, None, false),
                }
            }
            Command::Pause => {
                if self.state.status == Status::Playing {
                    push(self, Some(Status::Paused), None, Some(self.state.anchor.pos_ms), None, false);
                }
            }
            Command::Resume => {
                if self.state.status == Status::Paused {
                    push(self, Some(Status::Playing), None, Some(self.state.anchor.pos_ms), None, false);
                }
            }
            Command::Next | Command::Prev => {
                let neighbor = self.neighbor(matches!(cmd, Command::Next));
                if let Some(next_entry) = neighbor {
                    published.extend(self.apply(factory, Command::Play { entry: next_entry }));
                }
                // No neighbor (or idle): refused — nothing publishes.
            }
            Command::Seek { ms } => {
                if self.state.entry.is_some() {
                    push(self, None, None, Some(ms), None, false);
                }
            }
            Command::SetVolume(v) => {
                let clamped = v.clamp(0.0, 1.0);
                push(self, None, None, None, Some(clamped), false);
            }
            Command::Enqueue { tracks, placement } => {
                let at = self.cursor_pos_in_queue();
                match placement {
                    Placement::Next => match at {
                        Some(i) => {
                            let split = i + 1;
                            for (k, t) in tracks.into_iter().enumerate() {
                                self.queue.insert(split + k, t);
                            }
                        }
                        // Nothing current: "next" means the front.
                        None => {
                            let mut ts = tracks;
                            ts.reverse();
                            for t in ts {
                                self.queue.insert(0, t);
                            }
                        }
                    },
                    Placement::Append => self.queue.extend(tracks),
                }
                push(self, None, None, None, None, true);
            }
        }
        published
    }

    /// Queue position of the cursor, if it is still in the queue.
    fn cursor_pos_in_queue(&self) -> Option<usize> {
        let cursor = self.cursor?;
        self.queue.iter().position(|e| *e == cursor)
    }

    /// The entry after (`true`) or before (`false`) the cursor.
    fn neighbor(&self, forward: bool) -> Option<EntryId> {
        let i = self.cursor_pos_in_queue()?;
        if forward {
            self.queue.get(i + 1).copied()
        } else {
            i.checked_sub(1).and_then(|p| self.queue.get(p)).copied()
        }
    }
}

async fn run(
    factory: Arc<dyn PipelineFactory>,
    mut cmd_rx: mpsc::Receiver<Command>,
    state_tx: watch::Sender<PlaybackState>,
    initial: PlaybackState,
    journal: Arc<Mutex<VecDeque<PlaybackState>>>,
) {
    let mut core = Core { state: initial, queue: Vec::new(), cursor: None };
    while let Some(cmd) = cmd_rx.recv().await {
        for st in core.apply(factory.as_ref(), cmd) {
            if let Ok(mut j) = journal.lock() {
                j.push_back(st.clone());
                while j.len() > JOURNAL_CAP {
                    j.pop_front();
                }
            }
            let _ = state_tx.send(st);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;
    use crate::playback_types::{validate_transition, ShapeError};
    use std::collections::HashSet;

    /// Fake pipeline: succeeds for every entry except the configured
    /// failures; records every prepare/release call.
    struct FakePipelines {
        fail: HashSet<EntryId>,
        calls: Mutex<Vec<String>>,
    }

    impl FakePipelines {
        fn new(fail: &[u64]) -> Self {
            Self {
                fail: fail.iter().map(|e| EntryId(*e)).collect(),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl PipelineFactory for FakePipelines {
        fn prepare(&self, entry: EntryId) -> Result<(), ErrorKind> {
            self.calls.lock().unwrap().push(format!("prepare:{entry:?}"));
            if self.fail.contains(&entry) {
                Err(ErrorKind::Resolve)
            } else {
                Ok(())
            }
        }
        fn release(&self, entry: EntryId) {
            self.calls.lock().unwrap().push(format!("release:{entry:?}"));
        }
    }

    /// Tiny seeded LCG — "seeded random" without a dependency.
    struct Lcg(u64);
    impl Lcg {
        fn next_u64(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next_u64() % n.max(1)
        }
    }

    fn seeded_command(rng: &mut Lcg, entry_space: u64) -> Command {
        match rng.below(100) {
            0..=19 => Command::Play { entry: EntryId(rng.below(entry_space)) },
            20..=34 => Command::Pause,
            35..=49 => Command::Resume,
            50..=64 => Command::Next,
            65..=74 => Command::Prev,
            75..=84 => Command::Seek { ms: rng.below(300_000) },
            85..=89 => Command::SetVolume((rng.below(200) as f32) / 100.0),
            _ => {
                let n = rng.below(3) + 1;
                let tracks = (0..n).map(|_| EntryId(rng.below(entry_space))).collect();
                let placement = if rng.below(2) == 0 { Placement::Next } else { Placement::Append };
                Command::Enqueue { tracks, placement }
            }
        }
    }

    async fn drive(seed: u64, commands: usize, entry_space: u64) -> Vec<PlaybackState> {
        let engine = Engine::start(Arc::new(FakePipelines::new(&[0, 5])));
        let mut rng = Lcg(seed);
        for _ in 0..commands {
            engine.send(seeded_command(&mut rng, entry_space)).await;
        }
        engine.quiesce(10, 5_000).await;
        engine.journal()
    }

    fn assert_invariants(journal: &[PlaybackState]) {
        assert!(!journal.is_empty(), "journal holds at least the initial state");
        let mut prev = &journal[0];
        for st in &journal[1..] {
            // rev strictly increases on every published change
            assert!(st.rev > prev.rev, "rev went backwards: {} then {}", prev.rev, st.rev);
            // anchor rate 0 whenever not Playing (+ volume in range)
            st.validate().unwrap_or_else(|e| panic!("malformed state at rev {}: {e:?}", st.rev));
            // status transitions valid
            if st.status != prev.status {
                validate_transition(prev, st)
                    .unwrap_or_else(|e| panic!("illegal edge at rev {}: {e:?}", st.rev));
            }
            prev = st;
        }
    }

    #[tokio::test]
    async fn play_walks_the_full_lifecycle_and_sets_the_rate() {
        let engine = Engine::start(Arc::new(FakePipelines::new(&[])));
        engine.send(Command::Play { entry: EntryId(3) }).await;
        engine.quiesce(5, 2_000).await;

        let journal = engine.journal();
        let statuses: Vec<Status> = journal.iter().map(|s| s.status).collect();
        assert_eq!(
            statuses,
            vec![Status::Idle, Status::Resolving, Status::Buffering, Status::Playing]
        );
        assert_eq!(journal.last().unwrap().anchor.rate, 1);
        assert_eq!(journal.last().unwrap().entry, Some(EntryId(3)));
    }

    #[tokio::test]
    async fn refused_commands_publish_nothing() {
        let engine = Engine::start(Arc::new(FakePipelines::new(&[])));
        let rev_before = engine.subscribe().borrow().rev;
        engine.send(Command::Pause).await; // nothing is playing
        engine.send(Command::Resume).await; // not paused
        engine.send(Command::Next).await; // empty queue, idle
        engine.quiesce(5, 2_000).await;
        assert_eq!(engine.subscribe().borrow().rev, rev_before);
    }

    #[tokio::test]
    async fn factory_failure_lands_in_error_and_recovers() {
        let factory = Arc::new(FakePipelines::new(&[9]));
        let engine = Engine::start(factory.clone());
        engine.send(Command::Play { entry: EntryId(9) }).await;
        engine.quiesce(5, 2_000).await;
        let last = engine.subscribe().borrow().clone();
        assert_eq!(last.status, Status::Error(ErrorKind::Resolve));

        // Error → Resolving is a legal recovery edge.
        engine.send(Command::Play { entry: EntryId(2) }).await;
        engine.quiesce(5, 2_000).await;
        assert_eq!(engine.subscribe().borrow().status, Status::Playing);
        // The dead pipeline was released before the new one prepared.
        let calls = factory.calls.lock().unwrap();
        let release_idx = calls.iter().position(|c| c == "release:EntryId(9)").expect("released");
        let prepare_idx = calls.iter().position(|c| c == "prepare:EntryId(2)").expect("prepared");
        assert!(release_idx < prepare_idx, "release before prepare: {calls:?}");
    }

    #[tokio::test]
    async fn pause_resume_and_seek_keep_the_rate_law() {
        let engine = Engine::start(Arc::new(FakePipelines::new(&[])));
        engine.send(Command::Play { entry: EntryId(1) }).await;
        engine.send(Command::Pause).await;
        engine.send(Command::Seek { ms: 12_345 }).await;
        engine.send(Command::Resume).await;
        engine.quiesce(5, 2_000).await;

        let journal = engine.journal();
        let paused = journal.iter().rev().find(|s| s.status == Status::Paused).unwrap();
        assert_eq!(paused.anchor, Anchor::paused(12_345));
        assert_eq!(journal.last().unwrap().status, Status::Playing);
        assert_invariants(&journal);
    }

    #[tokio::test]
    async fn enqueue_bumps_queue_rev_and_next_walks_the_queue() {
        let engine = Engine::start(Arc::new(FakePipelines::new(&[])));
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(10), EntryId(11), EntryId(12)],
                placement: Placement::Append,
            })
            .await;
        engine.send(Command::Play { entry: EntryId(10) }).await;
        engine.send(Command::Next).await;
        engine.send(Command::Next).await;
        engine.send(Command::Prev).await;
        engine.quiesce(5, 2_000).await;

        let last = engine.subscribe().borrow().clone();
        assert_eq!(last.entry, Some(EntryId(11)));
        assert_eq!(last.queue_rev, 1);
        assert_invariants(&engine.journal());
    }

    #[tokio::test]
    async fn volume_is_clamped_into_range() {
        let engine = Engine::start(Arc::new(FakePipelines::new(&[])));
        engine.send(Command::SetVolume(1.5)).await;
        engine.quiesce(5, 2_000).await;
        let latest = engine.subscribe().borrow().clone();
        assert_eq!(latest.volume, 1.0);
        latest.validate().unwrap();
    }

    #[tokio::test]
    async fn seeded_random_sequences_hold_all_laws() {
        for seed in [1u64, 42, 1337] {
            let journal = drive(seed, 200, 8).await;
            assert_invariants(&journal);
        }
    }

    #[tokio::test]
    async fn malformed_states_are_reported_by_the_validator() {
        // The invariant helper catches what the task must never publish.
        let mut bad = PlaybackState {
            rev: 1,
            status: Status::Paused,
            entry: None,
            queue_rev: 0,
            anchor: Anchor { pos_ms: 0, rate: 1 },
            volume: 0.5,
        };
        assert!(matches!(bad.validate(), Err(ShapeError::RateNotZero { .. })));
        bad.anchor.rate = 0;
        bad.rev = 0;
        let next = PlaybackState { rev: 0, ..bad.clone() };
        assert!(validate_transition(&bad, &next).is_err());
    }
}
