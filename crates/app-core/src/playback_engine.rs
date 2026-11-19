//! The task-based playback engine: one task owns the [`PlaybackState`],
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

use crate::playback_types::{
    Anchor, Command, EntryId, ErrorKind, Placement, PlaybackState, Status,
};
use playback::pipeline::StreamStatus;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};

/// Transport + gain actions for the pipeline as a whole. The factory
/// maps them onto whatever entry is current on its side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Transport {
    Play,
    Pause,
    Seek {
        ms: u64,
    },
    Volume(f32),
    /// Drop the current output stream; the next load reopens whatever
    /// the OS now considers the default device.
    ReopenOutput,
}

/// The only seam between the playback task and the outside world. The real
/// implementation wraps the rodio playback; tests install a fake. No other
/// I/O happens in this module.
pub trait PipelineFactory: Send + Sync + 'static {
    /// Resolve + open media for this entry. `Ok(())` means the pipeline is
    /// ready to be audible; the `ErrorKind` says which stage failed.
    /// Opens media for `entry`. `key` is the remote TrackKey when this is a
    /// source-backed entry; the factory resolves through it.
    fn prepare(&self, entry: EntryId, key: Option<&sources::TrackKey>) -> Result<(), ErrorKind>;

    /// The entry stops being current: tear its pipeline down.
    fn release(&self, entry: EntryId);

    /// Drive transport/gain on the pipeline.
    fn transport(&self, action: Transport);

    /// Delivered position of the current media, if any — the truth the
    /// end-of-track deadline is confirmed against. Milliseconds.
    fn position_ms(&self) -> Option<u64>;

    /// Duration of the current media, if known. `None` disables
    /// auto-advance for that entry.
    fn duration_ms(&self) -> Option<u64>;

    /// Subscribe to the current stream's fill status, when the pipeline
    /// has a prefetching reader. `None` = local files only, no mid-stream
    /// status.
    fn status_watch(&self) -> Option<watch::Receiver<StreamStatus>> {
        None
    }

    /// Crossfade length in ms; 0 when the user chose gapless/off.
    fn crossfade_ms(&self) -> u64 {
        0
    }

    /// True when the next entry should be preloaded into the same sink for a
    /// seamless handoff (crossfade 0).
    fn gapless(&self) -> bool {
        false
    }

    /// Resolve and preload `entry` without making it audible. The default
    /// refuses: factories without preload support keep the normal path.
    fn preload_next(
        &self,
        _entry: EntryId,
        _key: Option<&sources::TrackKey>,
    ) -> Result<(), ErrorKind> {
        Err(ErrorKind::Decode)
    }

    /// Promote the preloaded entry. `crossfade_ms` 0 = gapless handoff.
    fn adopt_preloaded(&self, _crossfade_ms: u64) -> Result<(), ErrorKind> {
        Err(ErrorKind::Decode)
    }

    /// Throw away an un-adopted preload (queue change, seek, skip).
    fn discard_preloaded(&self) {}

    /// Drop the pinned resolution for this entry (URL cache, negative
    /// cache, track-key map) before a re-resolve retry.
    fn invalidate(&self, _entry: EntryId) {}

    /// Warm the resolver cache for a future entry, off the audio path.
    /// Default no-op; returns the future the engine runs in the background
    /// (concurrency 1). The shell's real factory resolves through its
    /// resolver, so a later `prepare` hits the URL cache instead of the
    /// network.
    fn prefetch_resolve(
        &self,
        _entry: EntryId,
        _key: Option<&sources::TrackKey>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        Box::pin(std::future::ready(()))
    }
}

/// How close to the reported duration counts as "the track ended".
const END_SLACK_MS: u64 = 250;

/// Event-driven resolve-ahead of the next queue entry. Runs only on a
/// queue/cursor change (Play/Next/Prev/Enqueue/auto-advance) — never on a
/// timer — and never while paused or idle. Local entries have no source key
/// and are skipped (they resolve live and cheaply at prepare). One in-flight
/// prefetch at a time; a changed next-entry aborts the stale one.
fn maybe_prefetch(
    core: &Core,
    factory: &Arc<dyn PipelineFactory>,
    prefetch: &mut Option<(EntryId, tokio::task::JoinHandle<()>)>,
) {
    if matches!(core.state.status, Status::Idle | Status::Paused) {
        return;
    }
    let Some(next) = core.neighbor(true) else {
        return;
    };
    if core.remote_keys.get(&next).is_none() {
        return;
    }
    if matches!(prefetch, Some((e, _)) if *e == next) {
        return;
    }
    if let Some((_, h)) = prefetch.take() {
        h.abort();
    }
    let key = core.remote_keys.get(&next).cloned();
    let fut = factory.prefetch_resolve(next, key.as_ref());
    *prefetch = Some((next, tokio::spawn(fut)));
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
/// Upper bound for one Enqueue. Above the largest legitimate use
/// (a full-library "play all" or the restore mirror; libraries are
/// documented in the thousands of tracks).
pub const MAX_ENQUEUE_TRACKS: usize = 10_000;
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
        Self {
            tx,
            state_rx,
            journal,
        }
    }

    /// Queue a command. Awaits under backpressure; the bounded channel never
    /// drops commands.
    pub async fn send(&self, command: Command) {
        let _ = self.tx.send(command).await;
    }

    /// Queue a command from a sync context (OS media-key handler). `false`
    /// when the channel is full or the task is gone.
    pub fn try_send(&self, command: Command) -> bool {
        self.tx.try_send(command).is_ok()
    }

    /// Subscribe to published playback states.
    pub fn subscribe(&self) -> watch::Receiver<PlaybackState> {
        self.state_rx.clone()
    }

    /// The full publish history (bounded; oldest first). Exists because
    /// `watch` coalesces — an observer can miss intermediate states, the
    /// journal cannot.
    pub fn journal(&self) -> Vec<PlaybackState> {
        self.journal
            .lock()
            .map(|j| j.iter().cloned().collect())
            .unwrap_or_default()
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
    /// Remote (source) identities for entries. Engine-side only; the
    /// factory resolves through these keys. `PlaybackState` stays small.
    remote_keys: std::collections::HashMap<EntryId, sources::TrackKey>,
    /// The entry the queue cursor points at (`None` before the first Play).
    cursor: Option<EntryId>,
    /// Runtime repeat mode (auto-advance + Next-at-tail).
    repeat: crate::playback_types::RepeatMode,
}

impl Core {
    /// Applies one command and returns every state to publish, in order, rev
    /// already advanced. Refused commands return an empty vec.
    fn apply(&mut self, factory: &dyn PipelineFactory, cmd: Command) -> Vec<PlaybackState> {
        let mut published = Vec::new();
        let mut push = |core: &mut Self,
                        status: Option<Status>,
                        entry: Option<Option<EntryId>>,
                        pos_ms: Option<u64>,
                        volume: Option<f32>,
                        queue_bump: bool| {
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
            Command::Play { entry, remote } => {
                let mut entry = entry;
                if let Some((kind, id)) = remote {
                    let key = sources::TrackKey::new(
                        match kind.as_str() {
                            "youtube" => sources::SourceKind::YouTube,
                            "addon" => sources::SourceKind::Addon,
                            "subsonic" => sources::SourceKind::Subsonic,
                            "radio" => sources::SourceKind::Radio,
                            _ => sources::SourceKind::Local,
                        },
                        id,
                    )
                    .map_err(|e| e.to_string());
                    if let Ok(key) = key {
                        self.remote_keys.insert(entry, key.clone());
                    }
                }
                if self.state.status == Status::Resolving {
                    // Resolving → Resolving is not a legal edge: the current
                    // resolve must land first.
                    return published;
                }
                // A track-level prepare failure (Resolve/Decode) skips
                // forward so one broken entry never stalls the queue. The
                // walk is a monotonic index advance — immune to duplicate
                // EntryIds in the queue — and bounded by queue.len(). A
                // Device failure stops (nothing else will play).
                let mut idx = self.queue.iter().position(|e| *e == entry);
                loop {
                    if let Some(prev) = self.cursor {
                        if prev != entry {
                            factory.release(prev);
                        }
                    }
                    let cursor_moves = self.cursor != Some(entry);
                    self.cursor = Some(entry);
                    // Any queue change bumps queue_rev: body (Enqueue) and the
                    // cursor (Play/Next/Prev land here). The body itself never
                    // enters PlaybackState.
                    push(
                        self,
                        Some(Status::Resolving),
                        Some(Some(entry)),
                        None,
                        None,
                        cursor_moves,
                    );
                    match factory.prepare(entry, self.remote_keys.get(&entry)) {
                        Ok(()) => {
                            push(self, Some(Status::Buffering), None, None, None, false);
                            push(
                                self,
                                Some(Status::Playing),
                                None,
                                Some(self.state.anchor.pos_ms),
                                None,
                                false,
                            );
                            break;
                        }
                        Err(kind) => {
                            push(self, Some(Status::Error(kind)), None, None, None, false);
                            if matches!(kind, ErrorKind::Device) {
                                break;
                            }
                            match idx {
                                Some(i) => match self.queue.get(i + 1).copied() {
                                    Some(next) => {
                                        idx = Some(i + 1);
                                        entry = next;
                                    }
                                    None => break, // queue exhausted: stay in Error
                                },
                                None => break, // entry not in queue: cannot walk
                            }
                        }
                    }
                }
            }
            Command::Pause => {
                if self.state.status == Status::Playing {
                    push(
                        self,
                        Some(Status::Paused),
                        None,
                        Some(self.state.anchor.pos_ms),
                        None,
                        false,
                    );
                    factory.transport(Transport::Pause);
                }
            }
            Command::Resume => {
                if self.state.status == Status::Paused {
                    push(
                        self,
                        Some(Status::Playing),
                        None,
                        Some(self.state.anchor.pos_ms),
                        None,
                        false,
                    );
                    factory.transport(Transport::Play);
                }
            }
            Command::Next | Command::Prev => {
                let neighbor = self.neighbor(matches!(cmd, Command::Next));
                if let Some(next_entry) = neighbor {
                    let remote = self.remote_keys.get(&next_entry).map(|k| {
                        let kind = match k.source {
                            sources::SourceKind::YouTube => "youtube",
                            sources::SourceKind::Addon => "addon",
                            sources::SourceKind::Subsonic => "subsonic",
                            sources::SourceKind::Local => "local",
                            sources::SourceKind::Radio => "radio",
                        };
                        (kind.to_string(), k.id.clone())
                    });
                    published.extend(self.apply(
                        factory,
                        Command::Play {
                            entry: next_entry,
                            remote,
                        },
                    ));
                }
                // No neighbor (or idle): refused — nothing publishes.
            }
            Command::Seek { ms } => {
                if self.state.entry.is_some() {
                    push(self, None, None, Some(ms), None, false);
                    factory.transport(Transport::Seek { ms });
                }
            }
            Command::SetVolume(v) => {
                // NaN survives `clamp` (IEEE totalOrder), and JSON
                // cannot carry it, so the only path here is engine-internal.
                // Ignore the command rather than risking a volume jump.
                if !v.is_finite() {
                    tracing::warn!(value = v, "non-finite volume ignored");
                    return published;
                }
                let clamped = v.clamp(0.0, 1.0);
                push(self, None, None, None, Some(clamped), false);
                factory.transport(Transport::Volume(clamped));
            }
            Command::RebuildOutput => {
                // Handled by the run loop (needs the factory); a stray
                // enqueue here is ignored.
            }
            Command::SetRepeat(mode) => {
                // Engine-local preference: no PlaybackState field changes, so
                // nothing is published (a rev bump with identical state would
                // be noise).
                self.repeat = mode;
            }
            Command::ClearQueue => {
                if let Some(cur) = self.cursor {
                    factory.release(cur);
                }
                let active = !matches!(self.state.status, Status::Idle);
                self.queue.clear();
                self.cursor = None;
                // Remote key mappings are queue state too — a cleared
                // queue must not let a reused EntryId resolve as remote.
                self.remote_keys.clear();
                if active {
                    // Every active status has a legal edge to Idle.
                    push(self, Some(Status::Idle), Some(None), Some(0), None, true);
                } else {
                    // Already idle: the body still changed, so publish.
                    push(self, None, Some(None), None, None, true);
                }
            }
            Command::Enqueue {
                tracks,
                remote,
                placement,
            } => {
                // `tracks` carries every entry id in order — local
                // library ids and remote row ids alike; `remote` maps the
                // remote subset to TrackKeys. Remote-only ids (legacy
                // callers that never list them in `tracks`) are appended
                // after the locals so the queue body contains them too.
                let mut entries = tracks;
                let remote_only: Vec<EntryId> = remote
                    .iter()
                    .filter(|(id, _)| !entries.contains(id))
                    .map(|(id, _)| *id)
                    .collect();
                if entries.len() + remote_only.len() > MAX_ENQUEUE_TRACKS {
                    // Refuse without touching anything: no mutation, no rev,
                    // no queue_rev.
                    tracing::warn!(
                        len = entries.len() + remote_only.len(),
                        cap = MAX_ENQUEUE_TRACKS,
                        "oversized enqueue refused"
                    );
                    return published;
                }
                for (id, (kind, key_id)) in remote {
                    let key = sources::TrackKey::new(
                        match kind.as_str() {
                            "youtube" => sources::SourceKind::YouTube,
                            "addon" => sources::SourceKind::Addon,
                            "subsonic" => sources::SourceKind::Subsonic,
                            "radio" => sources::SourceKind::Radio,
                            _ => sources::SourceKind::Local,
                        },
                        key_id,
                    );
                    if let Ok(key) = key {
                        self.remote_keys.insert(id, key);
                    }
                }
                entries.extend(remote_only);
                let at = self.cursor_pos_in_queue();
                match placement {
                    Placement::Next => match at {
                        Some(i) => {
                            let split = i + 1;
                            for (k, t) in entries.into_iter().enumerate() {
                                self.queue.insert(split + k, t);
                            }
                        }
                        // Nothing current: "next" means the front.
                        None => {
                            entries.reverse();
                            for t in entries {
                                self.queue.insert(0, t);
                            }
                        }
                    },
                    Placement::Append => self.queue.extend(entries),
                }
                push(self, None, None, None, None, true);
            }
            Command::Remove { entry } => {
                let Some(index) = self.queue.iter().position(|e| *e == entry) else {
                    return published;
                };
                if self.cursor == Some(entry) {
                    return published;
                }
                self.queue.remove(index);
                self.remote_keys.remove(&entry);
                push(self, None, None, None, None, true);
            }
            Command::Move { entry, to } => {
                let Some(from) = self.queue.iter().position(|e| *e == entry) else {
                    return published;
                };
                if self.cursor == Some(entry) || self.queue.is_empty() {
                    return published;
                }
                let to = to.min(self.queue.len() - 1);
                if from == to {
                    return published;
                }
                let item = self.queue.remove(from);
                self.queue.insert(to, item);
                push(self, None, None, None, None, true);
            }
        }
        published
    }

    /// A mid-stream status change (underflow/recovery) with no command
    /// behind it: position kept, rate recomputed, rev bumped.
    fn apply_status(&mut self, status: Status) -> Vec<PlaybackState> {
        self.state.rev += 1;
        self.state.status = status;
        self.state.anchor.rate = if status.runs_clock() { 1 } else { 0 };
        vec![self.state.clone()]
    }

    /// The queue is exhausted after this entry: fall to Idle. Playing → Idle
    /// is a legal edge; nothing current remains.
    fn finish(&mut self) -> Vec<PlaybackState> {
        self.state.rev += 1;
        self.state.status = Status::Idle;
        self.state.entry = None;
        self.state.anchor.rate = 0;
        vec![self.state.clone()]
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

    /// The entry auto-advance should move to. Repeat one is handled by the
    /// caller (restart), and returns None here so it is never preloaded.
    fn auto_next_entry(&self) -> Option<EntryId> {
        if self.repeat == crate::playback_types::RepeatMode::One {
            return None;
        }
        let i = self.cursor_pos_in_queue()?;
        self.queue.get(i + 1).copied().or_else(|| {
            if self.repeat == crate::playback_types::RepeatMode::All {
                self.queue.first().copied()
            } else {
                None
            }
        })
    }

    /// The pipeline already switched (crossfade/gapless preload): move the
    /// cursor and anchor to the adopted entry without re-preparing it.
    fn switch_to(&mut self, entry: EntryId) -> Vec<PlaybackState> {
        self.cursor = Some(entry);
        self.state.rev += 1;
        self.state.entry = Some(entry);
        self.state.anchor.pos_ms = 0;
        self.state.anchor.rate = if self.state.status.runs_clock() { 1 } else { 0 };
        vec![self.state.clone()]
    }

    /// Repeat one: the same entry from the top. The caller seeks the
    /// pipeline; the anchor restarts.
    fn restart_current(&mut self) -> Vec<PlaybackState> {
        self.state.rev += 1;
        self.state.anchor.pos_ms = 0;
        self.state.anchor.rate = if self.state.status.runs_clock() { 1 } else { 0 };
        vec![self.state.clone()]
    }
}

async fn run(
    factory: Arc<dyn PipelineFactory>,
    mut cmd_rx: mpsc::Receiver<Command>,
    state_tx: watch::Sender<PlaybackState>,
    initial: PlaybackState,
    journal: Arc<Mutex<VecDeque<PlaybackState>>>,
) {
    let mut core = Core {
        state: initial,
        queue: Vec::new(),
        remote_keys: std::collections::HashMap::new(),
        cursor: None,
        repeat: crate::playback_types::RepeatMode::Off,
    };
    // The ONE outstanding end-of-track deadline (tokio Instant). None = not
    // armed. Re-armed from the anchor after every command, confirmed against
    // frames (position_ms) when it fires, never periodic.
    #[expect(
        unused_assignments,
        reason = "the loop top recomputes the deadline before the first read"
    )]
    let mut deadline: Option<tokio::time::Instant> = None;
    // The pipeline's fill status, when the factory has one.
    let mut status_rx = factory.status_watch();
    // The one re-resolve retry per user-initiated load. Cleared when a
    // Play/Next/Prev starts a fresh resolution; consumed by a mid-stream
    // UrlDead; a second UrlDead for the same entry then fails outright.
    let mut re_resolve_used: Option<EntryId> = None;
    // At most one resolve-ahead in flight, for the entry it is warming.
    let mut prefetch: Option<(EntryId, tokio::task::JoinHandle<()>)> = None;
    // The entry preloaded into the pipeline but not yet adopted (gapless).
    let mut preloaded: Option<EntryId> = None;
    loop {
        // Arm/re-arm: when Playing with a known duration, one shot at the
        // remaining time. With crossfade/gapless, arm slightly earlier so the
        // next stream is decoded before the handoff.
        let next_for_transition = core.auto_next_entry();
        let crossfade = factory.crossfade_ms();
        let lead = if next_for_transition.is_some() && preloaded != next_for_transition {
            if crossfade > 0 {
                crossfade
            } else if factory.gapless() {
                400
            } else {
                0
            }
        } else {
            0
        };
        deadline = if core.state.status == Status::Playing {
            factory.duration_ms().map(|d| {
                let rest = d.saturating_sub(core.state.anchor.pos_ms);
                let wait = if lead > 0 && rest > lead + 50 {
                    rest - lead
                } else {
                    rest
                };
                tokio::time::Instant::now() + std::time::Duration::from_millis(wait)
            })
        } else {
            None
        };

        tokio::select! {
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else { break };
                if matches!(cmd, Command::Play { .. } | Command::Next | Command::Prev) {
                    re_resolve_used = None; // a fresh resolution resets the budget
                }
                if let Command::RebuildOutput = cmd {
                    // Device change / wake. Rebuild the pipeline at the
                    // current position and keep the user's play state.
                    if matches!(core.state.status, Status::Playing | Status::Paused) {
                    if let Some(entry) = core.state.entry {
                        let was_playing = core.state.status == Status::Playing;
                        let pos = factory
                            .position_ms()
                            .unwrap_or(core.state.anchor.pos_ms);
                        publish(&core.apply_status(Status::Resolving), &state_tx, &journal);
                        factory.transport(Transport::ReopenOutput);
                        match factory.prepare(entry, core.remote_keys.get(&entry)) {
                            Ok(()) => {
                                factory.transport(Transport::Seek { ms: pos });
                                core.state.anchor.pos_ms = pos;
                                publish(&core.apply_status(Status::Buffering), &state_tx, &journal);
                                publish(&core.apply_status(Status::Playing), &state_tx, &journal);
                                if !was_playing {
                                    factory.transport(Transport::Pause);
                                    publish(&core.apply_status(Status::Paused), &state_tx, &journal);
                                }
                            }
                            Err(kind) => {
                                publish(&core.apply_status(Status::Error(kind)), &state_tx, &journal);
                            }
                        }
                    }
                    }
                    continue;
                }
                // Any command that can change what plays next invalidates a
                // preloaded stream. (The pipeline would also drop it, but the
                // engine's marker must not outlive it.)
                if !matches!(
                    cmd,
                    Command::SetVolume(_)
                        | Command::SetRepeat(_)
                        | Command::Pause
                        | Command::Resume
                ) {
                    factory.discard_preloaded();
                    preloaded = None;
                }
                publish(&core.apply(factory.as_ref(), cmd), &state_tx, &journal);
                maybe_prefetch(&core, &factory, &mut prefetch);
            }
            // Mid-stream stall → Buffering; recovery → Playing. Legal
            // edges both ways (Playing → Buffering → Playing), anchor and
            // rate recomputed by `push`.
            _status_changed = async {
                match status_rx.as_mut() {
                    Some(rx) => rx.changed().await,
                    None => std::future::pending().await,
                }
            }, if status_rx.is_some() => {
                let fill = status_rx.as_ref().unwrap().borrow().clone();
                match (fill, core.state.status) {
                    (StreamStatus::Starved, Status::Playing) => {
                        publish(&core.apply_status(Status::Buffering), &state_tx, &journal);
                    }
                    (StreamStatus::Refilled, Status::Buffering) => {
                        publish(&core.apply_status(Status::Playing), &state_tx, &journal);
                    }
                    // The URL died mid-stream (403/410 or past expiry).
                    // Re-resolve and rebuild at the current position — once.
                    (StreamStatus::UrlDead, Status::Playing | Status::Paused) => {
                        let Some(entry) = core.state.entry else { continue };
                        if re_resolve_used == Some(entry) {
                            publish(
                                &core.apply_status(Status::Error(ErrorKind::Resolve)),
                                &state_tx,
                                &journal,
                            );
                            continue;
                        }
                        re_resolve_used = Some(entry);
                        // Drop the cached (dead) resolution before the
                        // one retry so the fresh resolve cannot hit it.
                        factory.invalidate(entry);
                        let pos = factory
                            .position_ms()
                            .unwrap_or(core.state.anchor.pos_ms);
                        publish(&core.apply_status(Status::Resolving), &state_tx, &journal);
                        match factory.prepare(entry, core.remote_keys.get(&entry)) {
                            Ok(()) => {
                                // Rebuild at the position where the stream died.
                                factory.transport(Transport::Seek { ms: pos });
                                core.state.anchor.pos_ms = pos;
                                publish(&core.apply_status(Status::Buffering), &state_tx, &journal);
                                publish(&core.apply_status(Status::Playing), &state_tx, &journal);
                            }
                            Err(kind) => {
                                publish(&core.apply_status(Status::Error(kind)), &state_tx, &journal);
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(deadline.unwrap_or_else(tokio::time::Instant::now)), if deadline.is_some() => {
                let (Some(d), Some(p)) = (factory.duration_ms(), factory.position_ms()) else {
                    // No position truth available: stand down rather than
                    // guess (the loop head re-arms on the next command).
                    continue;
                };
                let end = p.saturating_add(END_SLACK_MS) >= d;
                let next = core.auto_next_entry();

                if !end {
                    // Pre-end window: build the next stream before the
                    // handoff. A crossfade is promoted immediately (the fade
                    // spans the remaining time); a gapless preload waits for
                    // the real end.
                    if let Some(next) = next {
                        if preloaded != Some(next) {
                            let key = core.remote_keys.get(&next).cloned();
                            if factory.preload_next(next, key.as_ref()).is_ok() {
                                preloaded = Some(next);
                            }
                        }
                        if preloaded == Some(next) && crossfade > 0 {
                            if factory.adopt_preloaded(crossfade).is_ok() {
                                preloaded = None;
                                publish(&core.switch_to(next), &state_tx, &journal);
                                maybe_prefetch(&core, &factory, &mut prefetch);
                            } else {
                                // Crossfade could not start: let the normal
                                // end-of-track path take over.
                                factory.discard_preloaded();
                                preloaded = None;
                            }
                        }
                    }
                    continue;
                }

                // Track ended.
                if core.repeat == crate::playback_types::RepeatMode::One {
                    factory.transport(Transport::Seek { ms: 0 });
                    publish(&core.restart_current(), &state_tx, &journal);
                    continue;
                }
                let Some(next) = next else {
                    publish(&core.finish(), &state_tx, &journal);
                    continue;
                };
                if preloaded == Some(next) && factory.adopt_preloaded(0).is_ok() {
                    preloaded = None;
                    publish(&core.switch_to(next), &state_tx, &journal);
                    maybe_prefetch(&core, &factory, &mut prefetch);
                    continue;
                }
                preloaded = None;
                let remote = core.remote_keys.get(&next).map(|k| {
                    let kind = match k.source {
                        sources::SourceKind::YouTube => "youtube",
                        sources::SourceKind::Addon => "addon",
                        sources::SourceKind::Subsonic => "subsonic",
                        sources::SourceKind::Local => "local",
                        sources::SourceKind::Radio => "radio",
                    };
                    (kind.to_string(), k.id.clone())
                });
                publish(
                    &core.apply(factory.as_ref(), Command::Play { entry: next, remote }),
                    &state_tx,
                    &journal,
                );
                maybe_prefetch(&core, &factory, &mut prefetch);
            }
        }
    }
}

/// Push published states to the journal + the watch channel.
fn publish(
    states: &[PlaybackState],
    state_tx: &watch::Sender<PlaybackState>,
    journal: &Mutex<VecDeque<PlaybackState>>,
) {
    for st in states {
        if let Ok(mut j) = journal.lock() {
            j.push_back(st.clone());
            while j.len() > JOURNAL_CAP {
                j.pop_front();
            }
        }
        let _ = state_tx.send(st.clone());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;
    use crate::playback_types::{validate_transition, ShapeError};
    use std::collections::HashSet;
    use std::time::Duration;

    /// Fake pipeline: succeeds for every entry except the configured
    /// failures; records every prepare/release call.
    struct FakePipelines {
        fail: HashSet<EntryId>,
        calls: Mutex<Vec<String>>,
        /// The TrackKey each prepare received (remote entries).
        keys: Mutex<Vec<(EntryId, Option<sources::TrackKey>)>>,
        /// Prefetch lifecycle ("start:EntryId(..)" / "done:EntryId(..)").
        prefetches: Arc<Mutex<Vec<String>>>,
        /// Truth the end-of-track deadline confirms against.
        clock: Mutex<Clock>,
        /// Mid-stream fill status the engine subscribes to.
        status: watch::Sender<StreamStatus>,
    }

    #[derive(Default)]
    struct Clock {
        position_ms: Option<u64>,
        duration_ms: Option<u64>,
    }

    impl FakePipelines {
        fn new(fail: &[u64]) -> Self {
            Self {
                fail: fail.iter().map(|e| EntryId(*e)).collect(),
                calls: Mutex::new(Vec::new()),
                keys: Mutex::new(Vec::new()),
                prefetches: Arc::new(Mutex::new(Vec::new())),
                clock: Mutex::new(Clock::default()),
                status: watch::channel(StreamStatus::Refilled).0,
            }
        }
        fn set_media(&self, position_ms: Option<u64>, duration_ms: Option<u64>) {
            let mut c = self.clock.lock().unwrap();
            c.position_ms = position_ms;
            c.duration_ms = duration_ms;
        }
    }

    impl PipelineFactory for FakePipelines {
        fn prepare(
            &self,
            entry: EntryId,
            key: Option<&sources::TrackKey>,
        ) -> Result<(), ErrorKind> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("prepare:{entry:?}"));
            self.keys.lock().unwrap().push((entry, key.cloned()));
            if self.fail.contains(&entry) {
                Err(ErrorKind::Resolve)
            } else {
                Ok(())
            }
        }
        fn release(&self, entry: EntryId) {
            self.calls
                .lock()
                .unwrap()
                .push(format!("release:{entry:?}"));
        }
        fn transport(&self, action: Transport) {
            self.calls
                .lock()
                .unwrap()
                .push(format!("transport:{action:?}"));
        }
        fn position_ms(&self) -> Option<u64> {
            self.clock.lock().ok().and_then(|c| c.position_ms)
        }
        fn duration_ms(&self) -> Option<u64> {
            self.clock.lock().ok().and_then(|c| c.duration_ms)
        }
        fn status_watch(&self) -> Option<watch::Receiver<StreamStatus>> {
            Some(self.status.subscribe())
        }
        fn invalidate(&self, entry: EntryId) {
            self.calls
                .lock()
                .unwrap()
                .push(format!("invalidate:{entry:?}"));
        }
        fn prefetch_resolve(
            &self,
            entry: EntryId,
            _key: Option<&sources::TrackKey>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
            self.prefetches
                .lock()
                .unwrap()
                .push(format!("start:{entry:?}"));
            let log = self.prefetches.clone();
            Box::pin(async move {
                tokio::time::sleep(Duration::from_secs(1000)).await;
                log.lock().unwrap().push(format!("done:{entry:?}"));
            })
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

    /// A random remote key (source kind + id) from a small pool, with a
    /// row id far from the local entry space so the two never collide.
    fn seeded_remote(rng: &mut Lcg) -> (EntryId, (String, String)) {
        let source = if rng.below(2) == 0 {
            "youtube"
        } else {
            "addon"
        };
        let id = format!("{source}-vid-{}", rng.below(10_000));
        (
            EntryId(rng.below(1_000) + 100_000),
            (source.to_string(), id),
        )
    }

    fn seeded_command(rng: &mut Lcg, entry_space: u64) -> Command {
        match rng.below(100) {
            0..=14 => Command::Play {
                entry: EntryId(rng.below(entry_space)),
                remote: None,
            },
            15..=24 => {
                let (entry, key) = seeded_remote(rng);
                Command::Play {
                    entry,
                    remote: Some(key),
                }
            }
            25..=34 => Command::Pause,
            35..=44 => Command::Resume,
            45..=54 => Command::Next,
            55..=62 => Command::Prev,
            63..=72 => Command::Seek {
                ms: rng.below(300_000),
            },
            73..=80 => Command::SetVolume((rng.below(200) as f32) / 100.0),
            81..=89 => {
                let n = rng.below(3) + 1;
                let tracks = (0..n).map(|_| EntryId(rng.below(entry_space))).collect();
                let placement = if rng.below(2) == 0 {
                    Placement::Next
                } else {
                    Placement::Append
                };
                Command::Enqueue {
                    tracks,
                    remote: vec![],
                    placement,
                }
            }
            _ => {
                let n = rng.below(3) + 1;
                let mut tracks = (0..n)
                    .map(|_| EntryId(rng.below(entry_space)))
                    .collect::<Vec<_>>();
                let remote = (0..rng.below(2))
                    .map(|_| seeded_remote(rng))
                    .collect::<Vec<_>>();
                tracks.extend(remote.iter().map(|(e, _)| *e));
                let placement = if rng.below(2) == 0 {
                    Placement::Next
                } else {
                    Placement::Append
                };
                Command::Enqueue {
                    tracks,
                    remote,
                    placement,
                }
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
        assert!(
            !journal.is_empty(),
            "journal holds at least the initial state"
        );
        let mut prev = &journal[0];
        for st in &journal[1..] {
            // rev strictly increases on every published change
            assert!(
                st.rev > prev.rev,
                "rev went backwards: {} then {}",
                prev.rev,
                st.rev
            );
            // anchor rate 0 whenever not Playing (+ volume in range)
            st.validate()
                .unwrap_or_else(|e| panic!("malformed state at rev {}: {e:?}", st.rev));
            // a queue revision never moves backwards
            assert!(
                st.queue_rev >= prev.queue_rev,
                "queue_rev went backwards: {} then {}",
                prev.queue_rev,
                st.queue_rev
            );
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
        engine
            .send(Command::Play {
                entry: EntryId(3),
                remote: None,
            })
            .await;
        engine.quiesce(5, 2_000).await;

        let journal = engine.journal();
        let statuses: Vec<Status> = journal.iter().map(|s| s.status).collect();
        assert_eq!(
            statuses,
            vec![
                Status::Idle,
                Status::Resolving,
                Status::Buffering,
                Status::Playing
            ]
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
        engine
            .send(Command::Play {
                entry: EntryId(9),
                remote: None,
            })
            .await;
        engine.quiesce(5, 2_000).await;
        let last = engine.subscribe().borrow().clone();
        assert_eq!(last.status, Status::Error(ErrorKind::Resolve));

        // Error → Resolving is a legal recovery edge.
        engine
            .send(Command::Play {
                entry: EntryId(2),
                remote: None,
            })
            .await;
        engine.quiesce(5, 2_000).await;
        assert_eq!(engine.subscribe().borrow().status, Status::Playing);
        // The dead pipeline was released before the new one prepared.
        let calls = factory.calls.lock().unwrap();
        let release_idx = calls
            .iter()
            .position(|c| c == "release:EntryId(9)")
            .expect("released");
        let prepare_idx = calls
            .iter()
            .position(|c| c == "prepare:EntryId(2)")
            .expect("prepared");
        assert!(
            release_idx < prepare_idx,
            "release before prepare: {calls:?}"
        );
    }

    #[tokio::test]
    async fn pause_resume_and_seek_keep_the_rate_law() {
        let engine = Engine::start(Arc::new(FakePipelines::new(&[])));
        engine
            .send(Command::Play {
                entry: EntryId(1),
                remote: None,
            })
            .await;
        engine.send(Command::Pause).await;
        engine.send(Command::Seek { ms: 12_345 }).await;
        engine.send(Command::Resume).await;
        engine.quiesce(5, 2_000).await;

        let journal = engine.journal();
        let paused = journal
            .iter()
            .rev()
            .find(|s| s.status == Status::Paused)
            .unwrap();
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
                remote: vec![],
                placement: Placement::Append,
            })
            .await;
        engine
            .send(Command::Play {
                entry: EntryId(10),
                remote: None,
            })
            .await;
        engine.send(Command::Next).await;
        engine.send(Command::Next).await;
        engine.send(Command::Prev).await;
        engine.quiesce(5, 2_000).await;

        let last = engine.subscribe().borrow().clone();
        assert_eq!(last.entry, Some(EntryId(11)));
        // Queue semantics: every queue change bumps queue_rev — the Enqueue (1)
        // plus each cursor move (Play, Next, Next, Prev) = 5.
        assert_eq!(last.queue_rev, 5);
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

    // mixed local/remote identity in the queue model

    #[tokio::test]
    async fn seeded_random_mixed_local_remote_sequences_hold_all_laws() {
        // The same generator now emits remote Play/Enqueue commands
        // (source-keyed entries from a disjoint id space) alongside local
        // ones. The invariants are unchanged: rev monotonic, rate law, legal
        // edges, queue_rev monotonic.
        for seed in [7u64, 99, 2024] {
            let journal = drive(seed, 250, 8).await;
            assert_invariants(&journal);
        }
    }

    #[tokio::test]
    async fn play_remote_passes_the_track_key_to_prepare() {
        let fake = Arc::new(FakePipelines::new(&[]));
        let engine = Engine::start(fake.clone());
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(9001)],
                remote: vec![(EntryId(9001), ("youtube".into(), "vid-abc".into()))],
                placement: Placement::Append,
            })
            .await;
        engine
            .send(Command::Play {
                entry: EntryId(9001),
                remote: Some(("youtube".into(), "vid-abc".into())),
            })
            .await;
        flush().await;
        assert_eq!(engine.subscribe().borrow().entry, Some(EntryId(9001)));
        let keys = fake.keys.lock().unwrap();
        let prepared = keys
            .iter()
            .find(|(e, _)| *e == EntryId(9001))
            .expect("prepared");
        assert_eq!(
            prepared.1.as_ref().map(|k| k.source),
            Some(sources::SourceKind::YouTube)
        );
        assert_eq!(prepared.1.as_ref().map(|k| k.id.as_str()), Some("vid-abc"));
        assert_invariants(&engine.journal());
    }

    #[tokio::test]
    async fn remote_entries_walk_with_next_and_carry_keys() {
        let fake = Arc::new(FakePipelines::new(&[]));
        let engine = Engine::start(fake.clone());
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(9001), EntryId(9002)],
                remote: vec![
                    (EntryId(9001), ("youtube".into(), "v1".into())),
                    (EntryId(9002), ("addon".into(), "a2".into())),
                ],
                placement: Placement::Append,
            })
            .await;
        engine
            .send(Command::Play {
                entry: EntryId(9001),
                remote: Some(("youtube".into(), "v1".into())),
            })
            .await;
        flush().await;
        engine.send(Command::Next).await;
        flush().await;
        assert_eq!(engine.subscribe().borrow().entry, Some(EntryId(9002)));
        let keys = fake.keys.lock().unwrap();
        let last = keys.last().expect("prepared");
        assert_eq!(last.0, EntryId(9002));
        assert_eq!(
            last.1.as_ref().map(|k| k.source),
            Some(sources::SourceKind::Addon)
        );
        assert_eq!(last.1.as_ref().map(|k| k.id.as_str()), Some("a2"));
        assert_invariants(&engine.journal());
    }

    #[tokio::test]
    async fn clear_queue_drops_remote_keys() {
        let fake = Arc::new(FakePipelines::new(&[]));
        let engine = Engine::start(fake.clone());
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(9001)],
                remote: vec![(EntryId(9001), ("youtube".into(), "v1".into()))],
                placement: Placement::Append,
            })
            .await;
        engine.send(Command::ClearQueue).await;
        // Reuse the row id as a local entry: prepare must see no stale key.
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(9001)],
                remote: vec![],
                placement: Placement::Append,
            })
            .await;
        engine
            .send(Command::Play {
                entry: EntryId(9001),
                remote: None,
            })
            .await;
        flush().await;
        assert_eq!(engine.subscribe().borrow().entry, Some(EntryId(9001)));
        let keys = fake.keys.lock().unwrap();
        let last = keys.last().expect("prepared");
        assert!(last.1.is_none(), "stale remote key cleared: {last:?}");
    }

    // the one-shot end-of-track deadline, paused tokio clock

    /// Deterministic drain for the current-thread runtime — no time sleeps
    /// (virtual time would fire the very deadline under test).
    async fn flush() {
        for _ in 0..64 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn track_end_auto_advances_to_the_next_entry() {
        let fake = Arc::new(FakePipelines::new(&[]));
        let engine = Engine::start(fake.clone());
        fake.set_media(Some(0), Some(1000));
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(1), EntryId(2)],
                remote: vec![],
                placement: Placement::Append,
            })
            .await;
        engine
            .send(Command::Play {
                entry: EntryId(1),
                remote: None,
            })
            .await;
        flush().await;
        assert_eq!(engine.subscribe().borrow().status, Status::Playing);
        assert_eq!(engine.subscribe().borrow().entry, Some(EntryId(1)));

        // The track really ends: the frames confirm at the deadline.
        fake.set_media(Some(1000), Some(1000));
        tokio::time::advance(std::time::Duration::from_millis(1000)).await;
        flush().await;
        let last = engine.subscribe().borrow().clone();
        assert_eq!(last.status, Status::Playing);
        assert_eq!(last.entry, Some(EntryId(2)), "auto-advanced");
        assert_invariants(&engine.journal());
    }

    #[tokio::test(start_paused = true)]
    async fn exhausted_queue_falls_to_idle() {
        let fake = Arc::new(FakePipelines::new(&[]));
        let engine = Engine::start(fake.clone());
        fake.set_media(Some(0), Some(1000));
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(5)],
                remote: vec![],
                placement: Placement::Append,
            })
            .await;
        engine
            .send(Command::Play {
                entry: EntryId(5),
                remote: None,
            })
            .await;
        flush().await;

        fake.set_media(Some(1000), Some(1000));
        tokio::time::advance(std::time::Duration::from_millis(1000)).await;
        flush().await;
        let last = engine.subscribe().borrow().clone();
        assert_eq!(last.status, Status::Idle);
        assert_eq!(last.entry, None);
        assert_invariants(&engine.journal());
    }

    #[tokio::test(start_paused = true)]
    async fn pause_cancels_the_deadline() {
        let fake = Arc::new(FakePipelines::new(&[]));
        let engine = Engine::start(fake.clone());
        fake.set_media(Some(0), Some(1000));
        engine
            .send(Command::Play {
                entry: EntryId(1),
                remote: None,
            })
            .await;
        flush().await;
        engine.send(Command::Pause).await;
        flush().await;
        let rev_before = engine.subscribe().borrow().rev;

        // Even with the frames at the end, nothing may fire: paused.
        fake.set_media(Some(1000), Some(1000));
        tokio::time::advance(std::time::Duration::from_secs(10)).await;
        flush().await;
        let last = engine.subscribe().borrow().clone();
        assert_eq!(last.rev, rev_before, "no publish after pause");
        assert_eq!(last.status, Status::Paused);
    }

    #[tokio::test(start_paused = true)]
    async fn seek_cancels_and_the_deadline_rearms_from_the_new_anchor() {
        let fake = Arc::new(FakePipelines::new(&[]));
        let engine = Engine::start(fake.clone());
        fake.set_media(Some(0), Some(1000));
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(1), EntryId(2)],
                remote: vec![],
                placement: Placement::Append,
            })
            .await;
        engine
            .send(Command::Play {
                entry: EntryId(1),
                remote: None,
            })
            .await;
        flush().await;
        engine.send(Command::Seek { ms: 600 }).await; // cancel + re-arm: rest = 400 ms
        flush().await;
        let rev_after_seek = engine.subscribe().borrow().rev;

        // The pre-seek deadline (t0+1000) must not govern; nothing at t0+300.
        tokio::time::advance(std::time::Duration::from_millis(300)).await;
        flush().await;
        assert_eq!(
            engine.subscribe().borrow().rev,
            rev_after_seek,
            "no premature fire"
        );

        fake.set_media(Some(1000), Some(1000));
        tokio::time::advance(std::time::Duration::from_millis(400)).await;
        flush().await;
        let last = engine.subscribe().borrow().clone();
        assert_eq!(
            last.entry,
            Some(EntryId(2)),
            "advanced on the re-armed deadline"
        );
        assert_invariants(&engine.journal());
    }

    #[tokio::test(start_paused = true)]
    async fn fire_with_a_stale_position_rearms_instead_of_advancing() {
        let fake = Arc::new(FakePipelines::new(&[]));
        let engine = Engine::start(fake.clone());
        fake.set_media(Some(0), Some(1000));
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(1), EntryId(2)],
                remote: vec![],
                placement: Placement::Append,
            })
            .await;
        engine
            .send(Command::Play {
                entry: EntryId(1),
                remote: None,
            })
            .await;
        flush().await;
        let rev_playing = engine.subscribe().borrow().rev;

        // The deadline fires but the frames say the track is nowhere near
        // done (seek happened on the media side): re-arm, publish nothing.
        tokio::time::advance(std::time::Duration::from_millis(1000)).await;
        flush().await;
        assert_eq!(
            engine.subscribe().borrow().rev,
            rev_playing,
            "re-arm publishes nothing"
        );
        assert_eq!(engine.subscribe().borrow().entry, Some(EntryId(1)));

        fake.set_media(Some(1000), Some(1000));
        tokio::time::advance(std::time::Duration::from_millis(1000)).await;
        flush().await;
        assert_eq!(
            engine.subscribe().borrow().entry,
            Some(EntryId(2)),
            "advanced on the second fire"
        );
        assert_invariants(&engine.journal());
    }

    #[tokio::test]
    async fn clear_queue_stops_and_empties_then_the_queue_rebuilds() {
        let engine = Engine::start(Arc::new(FakePipelines::new(&[])));
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(1), EntryId(2)],
                remote: vec![],
                placement: Placement::Append,
            })
            .await;
        engine
            .send(Command::Play {
                entry: EntryId(1),
                remote: None,
            })
            .await;
        flush().await;
        engine.send(Command::ClearQueue).await;
        flush().await;
        let last = engine.subscribe().borrow().clone();
        assert_eq!(last.status, Status::Idle);
        assert_eq!(last.entry, None);

        // A fresh queue works afterwards: Enqueue + Play + Next.
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(7), EntryId(8)],
                remote: vec![],
                placement: Placement::Append,
            })
            .await;
        engine
            .send(Command::Play {
                entry: EntryId(7),
                remote: None,
            })
            .await;
        engine.send(Command::Next).await;
        flush().await;
        assert_eq!(engine.subscribe().borrow().entry, Some(EntryId(8)));
        assert_invariants(&engine.journal());
    }

    // Hardening

    #[tokio::test]
    async fn oversized_enqueue_is_refused_without_touching_state() {
        let engine = Engine::start(Arc::new(FakePipelines::new(&[])));
        let before = engine.subscribe().borrow().clone();

        // One over-cap command: refused entirely.
        engine
            .send(Command::Enqueue {
                tracks: (0..=MAX_ENQUEUE_TRACKS)
                    .map(|i| EntryId(i as u64))
                    .collect(),
                remote: vec![],
                placement: Placement::Append,
            })
            .await;
        // A normal command still lands afterwards.
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(1)],
                remote: vec![],
                placement: Placement::Append,
            })
            .await;
        flush().await;

        let after = engine.subscribe().borrow().clone();
        assert_eq!(
            after.queue_rev,
            before.queue_rev + 1,
            "only the small enqueue counted"
        );
        assert_eq!(after.rev, before.rev + 1, "over-cap published nothing");
        assert_invariants(&engine.journal());
    }

    #[tokio::test]
    async fn exactly_at_cap_enqueue_succeeds() {
        let engine = Engine::start(Arc::new(FakePipelines::new(&[])));
        engine
            .send(Command::Enqueue {
                tracks: (0..MAX_ENQUEUE_TRACKS).map(|i| EntryId(i as u64)).collect(),
                remote: vec![],
                placement: Placement::Append,
            })
            .await;
        flush().await;
        assert_eq!(engine.subscribe().borrow().queue_rev, 1, "at-cap accepted");
    }

    #[tokio::test]
    async fn non_finite_volume_is_ignored_finite_clamped() {
        let engine = Engine::start(Arc::new(FakePipelines::new(&[])));
        let start = engine.subscribe().borrow().clone(); // volume 1.0

        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            engine.send(Command::SetVolume(bad)).await;
        }
        flush().await;
        let after_bad = engine.subscribe().borrow().clone();
        assert_eq!(after_bad.rev, start.rev, "non-finite publishes nothing");
        assert_eq!(after_bad.volume, start.volume, "volume untouched");

        for (input, expected) in [
            (-0.5f32, 0.0),
            (0.0, 0.0),
            (0.5, 0.5),
            (1.0, 1.0),
            (1.5, 1.0),
        ] {
            engine.send(Command::SetVolume(input)).await;
            flush().await;
            let now = engine.subscribe().borrow().clone();
            assert_eq!(now.volume, expected, "SetVolume({input})");
        }
        assert_invariants(&engine.journal());
    }

    // mid-stream stall → Buffering; recovery → Playing

    #[tokio::test]
    async fn stream_stall_publishes_buffering_and_refill_restores_playing() {
        let fake = Arc::new(FakePipelines::new(&[]));
        let engine = Engine::start(fake.clone());
        engine
            .send(Command::Play {
                entry: EntryId(1),
                remote: None,
            })
            .await;
        flush().await;
        assert_eq!(engine.subscribe().borrow().status, Status::Playing);
        let rev_playing = engine.subscribe().borrow().rev;

        // The fetch task starves: Buffering, anchor kept, rate 0.
        fake.status.send(StreamStatus::Starved).unwrap();
        flush().await;
        let starved = engine.subscribe().borrow().clone();
        assert_eq!(starved.status, Status::Buffering);
        assert_eq!(starved.anchor.rate, 0, "the clock stops while buffering");
        assert_eq!(starved.entry, Some(EntryId(1)));

        // Recovery: Playing again, rate 1.
        fake.status.send(StreamStatus::Refilled).unwrap();
        flush().await;
        let recovered = engine.subscribe().borrow().clone();
        assert_eq!(recovered.status, Status::Playing);
        assert_eq!(recovered.anchor.rate, 1);
        assert_eq!(
            recovered.anchor.pos_ms, starved.anchor.pos_ms,
            "position kept"
        );
        assert_invariants(&engine.journal());
        let _ = rev_playing;
    }

    #[tokio::test]
    async fn status_noise_outside_the_relevant_states_is_ignored() {
        let fake = Arc::new(FakePipelines::new(&[]));
        let engine = Engine::start(fake.clone());
        flush().await; // let the engine task subscribe to the status watch
                       // Idle: a Starved with nothing loaded publishes nothing.
        fake.status.send(StreamStatus::Starved).unwrap();
        flush().await;
        let after = engine.subscribe().borrow().clone();
        assert_eq!(after.status, Status::Idle);
        assert_eq!(after.rev, 0, "no publishes while idle");
    }

    // device change / sleep-wake → rebuild at anchor

    #[tokio::test]
    async fn rebuild_output_while_playing_resumes_at_anchor() {
        let fake = Arc::new(FakePipelines::new(&[]));
        fake.set_media(Some(7000), Some(60_000)); // died 7 s in
        let engine = Engine::start(fake.clone());
        engine
            .send(Command::Play {
                entry: EntryId(1),
                remote: None,
            })
            .await;
        flush().await;
        assert_eq!(engine.subscribe().borrow().status, Status::Playing);

        engine.send(Command::RebuildOutput).await;
        flush().await;

        let last = engine.subscribe().borrow().clone();
        assert_eq!(last.status, Status::Playing, "rebuild + resume");
        assert_eq!(last.entry, Some(EntryId(1)));
        assert_eq!(last.anchor.pos_ms, 7000, "resumed at the anchor");
        assert_eq!(last.anchor.rate, 1);
        let calls = fake.calls.lock().unwrap();
        assert!(
            calls.iter().any(|c| c.contains("ReopenOutput")),
            "stream reopened: {calls:?}"
        );
        assert!(calls
            .iter()
            .any(|c| c.contains("Seek") && c.contains("7000")));
        assert_invariants(&engine.journal());
    }

    #[tokio::test]
    async fn rebuild_output_while_paused_stays_paused_at_anchor() {
        let fake = Arc::new(FakePipelines::new(&[]));
        fake.set_media(Some(3000), Some(60_000));
        let engine = Engine::start(fake.clone());
        engine
            .send(Command::Play {
                entry: EntryId(1),
                remote: None,
            })
            .await;
        engine.send(Command::Pause).await;
        flush().await;
        assert_eq!(engine.subscribe().borrow().status, Status::Paused);

        engine.send(Command::RebuildOutput).await;
        flush().await;

        let last = engine.subscribe().borrow().clone();
        assert_eq!(last.status, Status::Paused, "user pause kept");
        assert_eq!(last.anchor.pos_ms, 3000, "position at the anchor");
        assert_eq!(last.anchor.rate, 0);
        assert_invariants(&engine.journal());
    }

    #[tokio::test]
    async fn rebuild_output_while_idle_is_a_noop() {
        let fake = Arc::new(FakePipelines::new(&[]));
        let engine = Engine::start(fake.clone());
        flush().await;
        let before = engine.subscribe().borrow().clone();
        engine.send(Command::RebuildOutput).await;
        flush().await;
        let after = engine.subscribe().borrow().clone();
        assert_eq!(after.rev, before.rev, "idle publishes nothing");
        assert_eq!(after.status, Status::Idle);
    }

    #[tokio::test]
    async fn url_dead_re_resolves_rebuilds_at_position() {
        let fake = Arc::new(FakePipelines::new(&[]));
        fake.set_media(Some(5000), Some(60_000)); // died 5 s in
        let engine = Engine::start(fake.clone());
        engine
            .send(Command::Play {
                entry: EntryId(1),
                remote: None,
            })
            .await;
        flush().await;
        assert_eq!(engine.subscribe().borrow().status, Status::Playing);

        fake.status.send(StreamStatus::UrlDead).unwrap();
        flush().await;

        let last = engine.subscribe().borrow().clone();
        assert_eq!(last.status, Status::Playing, "re-resolved");
        assert_eq!(last.entry, Some(EntryId(1)));
        assert_eq!(last.anchor.pos_ms, 5000, "rebuilt at the death position");
        assert_eq!(last.anchor.rate, 1);
        // The walk went through Resolving → Buffering → Playing.
        let statuses: Vec<Status> = engine.journal().iter().map(|s| s.status).collect();
        let window = &statuses[statuses.len() - 3..];
        assert_eq!(
            window,
            &[Status::Resolving, Status::Buffering, Status::Playing]
        );
        // The transport seek restored the position.
        let calls = fake.calls.lock().unwrap();
        assert!(
            calls
                .iter()
                .any(|c| c.contains("Seek") && c.contains("5000")),
            "transport seek at the death position: {calls:?}"
        );
        assert_invariants(&engine.journal());
    }

    #[tokio::test]
    async fn second_url_dead_without_a_fresh_load_errors() {
        let fake = Arc::new(FakePipelines::new(&[]));
        fake.set_media(Some(5000), Some(60_000));
        let engine = Engine::start(fake.clone());
        engine
            .send(Command::Play {
                entry: EntryId(1),
                remote: None,
            })
            .await;
        flush().await;

        // First UrlDead: the one retry — and the cached resolution must be
        // invalidated first.
        fake.status.send(StreamStatus::UrlDead).unwrap();
        flush().await;
        assert_eq!(engine.subscribe().borrow().status, Status::Playing);
        {
            let calls = fake.calls.lock().unwrap();
            assert!(
                calls.iter().any(|c| c.contains("invalidate")),
                "invalidate called before retry: {calls:?}"
            );
        }

        // Second UrlDead on the same entry: budget spent → Error.
        fake.status.send(StreamStatus::UrlDead).unwrap();
        flush().await;
        let last = engine.subscribe().borrow().clone();
        assert_eq!(last.status, Status::Error(ErrorKind::Resolve));
        assert_invariants(&engine.journal());

        // A fresh Play resets the budget: UrlDead retries again.
        engine
            .send(Command::Play {
                entry: EntryId(1),
                remote: None,
            })
            .await;
        flush().await;
        assert_eq!(engine.subscribe().borrow().status, Status::Playing);
        fake.status.send(StreamStatus::UrlDead).unwrap();
        flush().await;
        assert_eq!(
            engine.subscribe().borrow().status,
            Status::Playing,
            "retry budget renewed"
        );
    }

    // mixed local/remote queue model

    #[tokio::test]
    async fn mixed_local_remote_commands_hold_all_invariants() {
        let fake = Arc::new(FakePipelines::new(&[]));
        let engine = Engine::start(fake.clone());
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(1), EntryId(2)],
                remote: vec![(EntryId(9001), ("youtube".into(), "vid-abc".into()))],
                placement: Placement::Append,
            })
            .await;
        engine
            .send(Command::Play {
                entry: EntryId(9001),
                remote: Some(("youtube".into(), "vid-abc".into())),
            })
            .await;
        flush().await;
        let state = engine.subscribe().borrow().clone();
        assert_eq!(state.status, Status::Playing);
        assert_eq!(state.entry, Some(EntryId(9001)));
        assert_invariants(&engine.journal());
        let calls = fake.calls.lock().unwrap();
        assert!(
            calls.iter().any(|c| c.contains("prepare:EntryId(9001)")),
            "{calls:?}"
        );
    }

    #[tokio::test]
    async fn over_cap_enqueue_with_remote_is_refused() {
        let engine = Engine::start(Arc::new(FakePipelines::new(&[])));
        let before = engine.subscribe().borrow().clone();
        engine
            .send(Command::Enqueue {
                tracks: (0..MAX_ENQUEUE_TRACKS).map(|i| EntryId(i as u64)).collect(),
                remote: vec![(EntryId(9_999_999), ("youtube".into(), "x".into()))],
                placement: Placement::Append,
            })
            .await;
        flush().await;
        let after = engine.subscribe().borrow().clone();
        assert_eq!(after.rev, before.rev, "over-cap published nothing");
    }

    // resolve-ahead

    #[tokio::test]
    async fn prefetch_warms_the_next_remote_entry_after_play() {
        let fake = Arc::new(FakePipelines::new(&[]));
        let engine = Engine::start(fake.clone());
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(9001), EntryId(9002)],
                remote: vec![
                    (EntryId(9001), ("youtube".into(), "v1".into())),
                    (EntryId(9002), ("youtube".into(), "v2".into())),
                ],
                placement: Placement::Append,
            })
            .await;
        engine
            .send(Command::Play {
                entry: EntryId(9001),
                remote: Some(("youtube".into(), "v1".into())),
            })
            .await;
        flush().await;
        let pf = fake.prefetches.lock().unwrap();
        assert!(
            pf.iter().any(|l| l == "start:EntryId(9002)"),
            "next remote entry prefetched: {pf:?}"
        );
    }

    #[tokio::test]
    async fn no_prefetch_while_paused_or_for_local_entries() {
        let fake = Arc::new(FakePipelines::new(&[]));
        let engine = Engine::start(fake.clone());
        // Local entries have no source keys: nothing to warm.
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(1), EntryId(2)],
                remote: vec![],
                placement: Placement::Append,
            })
            .await;
        engine
            .send(Command::Play {
                entry: EntryId(1),
                remote: None,
            })
            .await;
        engine.send(Command::Pause).await;
        // A queue change while paused must not spawn a prefetch.
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(9003)],
                remote: vec![(EntryId(9003), ("youtube".into(), "v3".into()))],
                placement: Placement::Append,
            })
            .await;
        flush().await;
        assert!(
            fake.prefetches.lock().unwrap().is_empty(),
            "no prefetch while paused/local"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn queue_change_cancels_a_stale_prefetch() {
        let fake = Arc::new(FakePipelines::new(&[]));
        let engine = Engine::start(fake.clone());
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(9001), EntryId(9002)],
                remote: vec![
                    (EntryId(9001), ("youtube".into(), "v1".into())),
                    (EntryId(9002), ("youtube".into(), "v2".into())),
                ],
                placement: Placement::Append,
            })
            .await;
        engine
            .send(Command::Play {
                entry: EntryId(9001),
                remote: Some(("youtube".into(), "v1".into())),
            })
            .await;
        flush().await; // prefetch of 9002 in flight (sleeping)
                       // Insert fresh entries right after the current one: next becomes 9003.
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(9003), EntryId(9004)],
                remote: vec![
                    (EntryId(9003), ("youtube".into(), "v3".into())),
                    (EntryId(9004), ("youtube".into(), "v4".into())),
                ],
                placement: Placement::Next,
            })
            .await;
        flush().await; // aborts stale 9002, starts 9003
        tokio::time::advance(Duration::from_secs(1000)).await;
        flush().await;
        let pf = fake.prefetches.lock().unwrap();
        assert!(
            pf.iter().any(|l| l == "start:EntryId(9002)"),
            "stale prefetch started: {pf:?}"
        );
        assert!(
            pf.iter().any(|l| l == "start:EntryId(9003)"),
            "replaced prefetch started: {pf:?}"
        );
        assert!(
            pf.iter().any(|l| l == "done:EntryId(9003)"),
            "new prefetch completed: {pf:?}"
        );
        assert!(
            !pf.iter().any(|l| l == "done:EntryId(9002)"),
            "stale prefetch was cancelled: {pf:?}"
        );
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
        assert!(matches!(
            bad.validate(),
            Err(ShapeError::RateNotZero { .. })
        ));
        bad.anchor.rate = 0;
        bad.rev = 0;
        let next = PlaybackState {
            rev: 0,
            ..bad.clone()
        };
        assert!(validate_transition(&bad, &next).is_err());
    }

    // Skip-walk regression: duplicate-id infinite loop

    /// Adjacent duplicate failing ids must terminate, not loop forever.
    /// Old code: neighbor() first-match returns same index → infinite loop.
    /// New code: monotonic idx walk → exhausts to EntryId(1), then Playing.
    #[tokio::test]
    async fn skip_walk_adjacent_duplicate_failing_ids_terminate() {
        let engine = Engine::start(Arc::new(FakePipelines::new(&[5])));
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(5), EntryId(5), EntryId(5), EntryId(1)],
                remote: vec![],
                placement: Placement::Append,
            })
            .await;
        let result = tokio::time::timeout(Duration::from_secs(2), async {
            engine
                .send(Command::Play {
                    entry: EntryId(5),
                    remote: None,
                })
                .await;
            engine.quiesce(10, 1_000).await;
        })
        .await;
        assert!(result.is_ok(), "infinite loop detected: timed out");
        let last = engine.subscribe().borrow().clone();
        assert_eq!(last.entry, Some(EntryId(1)));
        assert_eq!(last.status, Status::Playing);
        assert_invariants(&engine.journal());
    }

    /// All-failing queue must exhaust cleanly (Error, not Playing, not hung).
    #[tokio::test]
    async fn skip_walk_all_failing_queue_exhausts() {
        let engine = Engine::start(Arc::new(FakePipelines::new(&[5])));
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(5), EntryId(5), EntryId(5)],
                remote: vec![],
                placement: Placement::Append,
            })
            .await;
        let result = tokio::time::timeout(Duration::from_secs(2), async {
            engine
                .send(Command::Play {
                    entry: EntryId(5),
                    remote: None,
                })
                .await;
            engine.quiesce(10, 1_000).await;
        })
        .await;
        assert!(result.is_ok(), "infinite loop detected: timed out");
        let last = engine.subscribe().borrow().clone();
        assert!(
            matches!(last.status, Status::Error(_)),
            "exhausted queue stays in Error"
        );
        assert_invariants(&engine.journal());
    }

    /// Separated duplicates: [5, 7, 5, 1] with fail = {5, 7}.
    /// Old code: after 5 fails at idx 0, cursor = 5; cursor_pos_in_queue()
    /// returns idx 0 again; neighbor returns queue[1] = 7; 7 fails; cursor = 7;
    /// cursor_pos_in_queue() returns idx 1; neighbor returns queue[2] = 5;
    /// 5 fails; cursor = 5; cursor_pos_in_queue() returns idx 0 again → loops.
    /// New code: idx 0 → 1 (7, fail) → 2 (5, fail) → 3 (1, ok). Terminates.
    #[tokio::test]
    async fn skip_walk_lands_at_correct_index_not_first_name_match() {
        let result = tokio::time::timeout(Duration::from_secs(2), async {
            let engine = Engine::start(Arc::new(FakePipelines::new(&[5, 7])));
            engine
                .send(Command::Enqueue {
                    tracks: vec![EntryId(5), EntryId(7), EntryId(5), EntryId(1)],
                    remote: vec![],
                    placement: Placement::Append,
                })
                .await;
            engine
                .send(Command::Play {
                    entry: EntryId(5),
                    remote: None,
                })
                .await;
            engine.quiesce(10, 1_000).await;
            engine
        })
        .await;
        assert!(result.is_ok(), "infinite loop detected: timed out");
        let engine = result.unwrap();
        let last = engine.subscribe().borrow().clone();
        assert_eq!(
            last.entry,
            Some(EntryId(1)),
            "monotonic walk reaches idx 3, not stuck on idx 0/1"
        );
        assert_eq!(last.status, Status::Playing);
        assert_invariants(&engine.journal());
    }

    /// Multiple consecutive non-duplicate failures then a good entry.
    #[tokio::test]
    async fn skip_walk_multiple_distinct_failures_then_success() {
        // fail = {0, 5}; queue = [0, 5, 0, 1]; should land on EntryId(1).
        let engine = Engine::start(Arc::new(FakePipelines::new(&[0, 5])));
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(0), EntryId(5), EntryId(0), EntryId(1)],
                remote: vec![],
                placement: Placement::Append,
            })
            .await;
        engine
            .send(Command::Play {
                entry: EntryId(0),
                remote: None,
            })
            .await;
        engine.quiesce(10, 2_000).await;
        let last = engine.subscribe().borrow().clone();
        assert_eq!(last.entry, Some(EntryId(1)));
        assert_eq!(last.status, Status::Playing);
        assert_invariants(&engine.journal());
    }

    /// Device error stops the walk immediately regardless of what follows.
    #[tokio::test]
    async fn skip_walk_device_error_stops_immediately() {
        struct AlwaysDevice;
        impl PipelineFactory for AlwaysDevice {
            fn prepare(&self, _: EntryId, _: Option<&sources::TrackKey>) -> Result<(), ErrorKind> {
                Err(ErrorKind::Device)
            }
            fn release(&self, _: EntryId) {}
            fn transport(&self, _: Transport) {}
            fn position_ms(&self) -> Option<u64> {
                None
            }
            fn duration_ms(&self) -> Option<u64> {
                None
            }
        }
        let engine = Engine::start(Arc::new(AlwaysDevice));
        engine
            .send(Command::Enqueue {
                tracks: vec![EntryId(1), EntryId(2), EntryId(3)],
                remote: vec![],
                placement: Placement::Append,
            })
            .await;
        engine
            .send(Command::Play {
                entry: EntryId(1),
                remote: None,
            })
            .await;
        engine.quiesce(10, 2_000).await;
        let last = engine.subscribe().borrow().clone();
        assert_eq!(last.status, Status::Error(ErrorKind::Device));
        assert_eq!(
            last.entry,
            Some(EntryId(1)),
            "stopped at first entry, did not skip"
        );
        let journal = engine.journal();
        assert_eq!(
            journal
                .iter()
                .filter(|s| s.status == Status::Resolving)
                .count(),
            1,
            "only one Resolving: Device stops the walk"
        );
        assert_invariants(&journal);
    }

    /// Seeded random sequences must complete within a wall-clock deadline.
    /// Without this guard a regression reintroducing the loop silently consumes
    /// memory for the full quiesce timeout (5 s per seed) before appearing to
    /// pass on a truncated journal.
    #[tokio::test]
    async fn seeded_random_sequences_complete_within_deadline() {
        for seed in [1u64, 42, 1337, 7, 99, 2024] {
            let result = tokio::time::timeout(Duration::from_secs(5), drive(seed, 250, 8)).await;
            assert!(
                result.is_ok(),
                "seed {seed}: timed out — possible infinite loop"
            );
            assert_invariants(&result.unwrap());
        }
    }
}
