//! S11: autoplay. An app-crate task — never engine logic — that appends
//! related tracks when the queue tail is playing and the setting is on.
//!
//! Event-driven only: the loop awaits the PlaybackState watch and the
//! forwarded-command channel; no timers, nothing periodic, nothing runs
//! while paused beyond handling the events already queued.
//!
//! The engine's queue is task-local (sole-owner invariant), so the mirror
//! here reconstructs only what autoplay needs from inputs the shell already
//! sees: every forwarded `Command` (queue body) and every published
//! `PlaybackState` (cursor — media keys and auto-advance included).

use app_core::playback_engine::MAX_ENQUEUE_TRACKS;
use app_core::playback_types::{Command, EntryId, Placement, PlaybackState, Status};
use sources::yt::DiscoveryTrack;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

/// The autoplay setting key (SQLite `settings`; default off).
pub const AUTOPLAY_SETTING: &str = "autoplay";

type BoxFut<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>;

/// Managed: the shell forwards every engine command here so the task's
/// mirror sees the queue body the way the engine receives it.
pub struct AutoplayCmds(pub mpsc::UnboundedSender<Command>);

/// Managed: the autoplay setting as the task reads it (mirrors SQLite).
pub struct AutoplayFlag(pub Arc<std::sync::atomic::AtomicBool>);

/// One autoplay row pushed to the frontend's queue mirror (S11). The
/// frontend only refreshes ids it already knows, so appended rows must be
/// announced.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoplayRow {
    pub engine_id: u64,
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_sec: Option<f64>,
    pub thumbnail_url: Option<String>,
}

/// App-crate replica of the engine's observable queue state: the body from
/// forwarded commands, the cursor from the watch. Play never touches the
/// body (the frontend's "play now" is ClearQueue + Enqueue + Play), so only
/// Enqueue/ClearQueue mutate it — exactly as the engine's arms do.
#[derive(Default)]
pub struct QueueMirror {
    order: Vec<EntryId>,
    /// EntryId → (source kind, source track id) for remote entries.
    keys: HashMap<EntryId, (String, String)>,
    /// The playing entry, from the published state.
    cursor: Option<EntryId>,
}

impl QueueMirror {
    /// Applies one forwarded command the way the engine mutates its queue
    /// body (Enqueue placement + the remote-only append; ClearQueue).
    pub fn apply(&mut self, cmd: &Command) {
        match cmd {
            Command::Enqueue { tracks, remote, placement } => {
                let mut entries: Vec<EntryId> = tracks.clone();
                for (id, _) in remote {
                    // The engine appends remote-only ids after the locals.
                    if !entries.contains(id) {
                        entries.push(*id);
                    }
                }
                for (id, (kind, key_id)) in remote {
                    self.keys.insert(*id, (kind.clone(), key_id.clone()));
                }
                match placement {
                    Placement::Next => match self
                        .cursor
                        .and_then(|c| self.order.iter().position(|e| *e == c))
                    {
                        Some(i) => {
                            for (k, t) in entries.into_iter().enumerate() {
                                self.order.insert(i + 1 + k, t);
                            }
                        }
                        // Nothing current: "next" means the front (engine:
                        // reversed inserts at 0 = original order at front).
                        None => {
                            let mut front = entries;
                            front.reverse();
                            for t in front {
                                self.order.insert(0, t);
                            }
                        }
                    },
                    Placement::Append => self.order.extend(entries),
                }
            }
            Command::ClearQueue => {
                self.order.clear();
                self.keys.clear();
                self.cursor = None;
            }
            _ => {}
        }
    }

    pub fn set_cursor(&mut self, entry: Option<EntryId>) {
        self.cursor = entry;
    }

    /// True when `entry` is both the last body entry and the cursor.
    pub fn is_tail(&self, entry: EntryId) -> bool {
        self.order.last() == Some(&entry) && self.cursor == Some(entry)
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// The playing entry's (source kind, source track id), when remote.
    pub fn playing_key(&self) -> Option<(String, String)> {
        let c = self.cursor?;
        self.keys.get(&c).cloned()
    }

    /// Every remote source id already in the queue (dedup set).
    pub fn known_ids(&self) -> HashSet<String> {
        self.keys.values().map(|(_, id)| id.clone()).collect()
    }

    /// Registers the task's own autoplay enqueues so the mirror stays the
    /// one source of tail/dedup truth for the session.
    pub fn extend_autoplay(&mut self, minted: &[(EntryId, String)]) {
        for (id, key_id) in minted {
            self.order.push(*id);
            self.keys.insert(*id, ("youtube".to_string(), key_id.clone()));
        }
    }
}

/// Dedups related rows against everything already queued and caps to the
/// remaining enqueue capacity (within the engine's MAX_ENQUEUE_TRACKS).
pub fn pick_related(
    rows: Vec<DiscoveryTrack>,
    known: &mut HashSet<String>,
    remaining: usize,
) -> Vec<DiscoveryTrack> {
    let mut out = Vec::new();
    for row in rows {
        if out.len() >= remaining {
            break;
        }
        if !known.insert(row.key.id.clone()) {
            continue;
        }
        out.push(row);
    }
    out
}

/// The task's seams as closures — the shell builds the networked ones,
/// tests build fakes. Not a trait.
pub struct AutoplayDeps {
    /// The autoplay setting, checked at every trigger.
    pub enabled: Arc<std::sync::atomic::AtomicBool>,
    /// Related rows for (source kind, source track id). Errors mean
    /// "nothing to add" — best-effort, never a user-facing failure.
    pub related:
        Arc<dyn Fn(String, String) -> BoxFut<Result<Vec<DiscoveryTrack>, String>> + Send + Sync>,
    /// Enqueues the rows (engine + frontend notify) and returns the minted
    /// (EntryId, source id) pairs the mirror must adopt.
    pub enqueue: Arc<dyn Fn(Vec<DiscoveryTrack>) -> BoxFut<Vec<(EntryId, String)>> + Send + Sync>,
}

/// The trigger, shared by both arms: Playing at the queue tail, setting on,
/// once per entry. `await`s the related fetch; the caller must not hold
/// locks across it.
async fn consider_fetch(
    st: &PlaybackState,
    mirror: &mut QueueMirror,
    deps: &AutoplayDeps,
    fetched_for: &mut Option<EntryId>,
) {
    use std::sync::atomic::Ordering::Relaxed;
    if st.status != Status::Playing {
        return; // paused/idle events do nothing beyond cursor updates
    }
    let Some(entry) = st.entry else { return };
    if !mirror.is_tail(entry) || *fetched_for == Some(entry) || !deps.enabled.load(Relaxed) {
        return;
    }
    *fetched_for = Some(entry);
    let Some((kind, key_id)) = mirror.playing_key() else {
        return; // local tail entry: no related fetch
    };
    if kind != "youtube" {
        return; // the radio endpoint is a sidecar read
    }
    let rows = (deps.related)(kind, key_id).await.unwrap_or_default();
    let mut known = mirror.known_ids();
    let remaining = MAX_ENQUEUE_TRACKS.saturating_sub(mirror.len());
    let picked = pick_related(rows, &mut known, remaining);
    if picked.is_empty() {
        return;
    }
    let minted = (deps.enqueue)(picked).await;
    if !minted.is_empty() {
        mirror.extend_autoplay(&minted);
    }
}

/// The autoplay loop. Wakes only on watch publications and forwarded
/// commands; every branch completes before the next await.
pub async fn run_autoplay(
    mut state_rx: watch::Receiver<PlaybackState>,
    mut cmds: mpsc::UnboundedReceiver<Command>,
    deps: AutoplayDeps,
) {
    let mut mirror = QueueMirror::default();
    // One related fetch per queue entry: the entry the last fetch armed for.
    let mut fetched_for: Option<EntryId> = None;
    loop {
        tokio::select! {
            changed = state_rx.changed() => {
                if changed.is_err() {
                    return; // engine gone (app shutdown)
                }
                let st = state_rx.borrow_and_update().clone();
                mirror.set_cursor(st.entry);
                consider_fetch(&st, &mut mirror, &deps, &mut fetched_for).await;
            }
            Some(cmd) = cmds.recv() => {
                mirror.apply(&cmd);
                // The shell forwards each command BEFORE the engine acts on
                // it, but select! may still run this arm after the watch
                // event the command caused — re-checking the current state
                // here closes that race (the command itself is an event).
                let st = state_rx.borrow().clone();
                consider_fetch(&st, &mut mirror, &deps, &mut fetched_for).await;
            }
            else => return,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;
    use sources::{SourceKind, TrackKey};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    fn track(id: &str) -> DiscoveryTrack {
        DiscoveryTrack {
            key: TrackKey::new(SourceKind::YouTube, id).unwrap(),
            title: format!("T {id}"),
            artist: "A".into(),
            album: None,
            duration_sec: Some(180),
            thumbnail_url: None,
            artists: vec![],
            album_id: None,
            is_video: false,
            explicit: false,
            playable: true,
        }
    }

    fn state(entry: u64, status: Status) -> PlaybackState {
        PlaybackState {
            rev: 1,
            status,
            entry: Some(EntryId(entry)),
            queue_rev: 1,
            anchor: app_core::playback_types::Anchor::paused(0),
            volume: 1.0,
        }
    }

    fn enqueue_cmd(ids: &[u64], placement: Placement) -> Command {
        Command::Enqueue {
            tracks: ids.iter().map(|i| EntryId(*i)).collect(),
            remote: ids
                .iter()
                .map(|i| (EntryId(*i), ("youtube".into(), format!("v{i}"))))
                .collect(),
            placement,
        }
    }

    /// Drives the loop with a fake related fetch and a recording enqueue.
    struct Harness {
        state_tx: watch::Sender<PlaybackState>,
        cmd_tx: mpsc::UnboundedSender<Command>,
        /// Video ids handed to the fake enqueue, in call order.
        enqueued: Arc<Mutex<Vec<String>>>,
        fetches: Arc<AtomicUsize>,
        /// Rows the fake related returns on the next fetch.
        related_rows: Arc<Mutex<Vec<DiscoveryTrack>>>,
        /// The task's enabled flag (flipped by the not-when-off test).
        flag: Arc<AtomicBool>,
    }

    fn harness(autoplay_on: bool) -> Harness {
        let (state_tx, state_rx) = watch::channel(app_core::playback_types::PlaybackState {
            rev: 0,
            status: Status::Idle,
            entry: None,
            queue_rev: 0,
            anchor: app_core::playback_types::Anchor::paused(0),
            volume: 1.0,
        });
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let enqueued: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let fetches = Arc::new(AtomicUsize::new(0));
        let related_rows = Arc::new(Mutex::new(Vec::new()));
        let deps = AutoplayDeps {
            enabled: Arc::new(AtomicBool::new(autoplay_on)),
            related: {
                let fetches = fetches.clone();
                let rows = related_rows.clone();
                Arc::new(move |_kind, _id| {
                    fetches.fetch_add(1, Ordering::SeqCst);
                    let rows = rows.lock().unwrap().clone();
                    Box::pin(async move { Ok(rows) }) as BoxFut<_>
                })
            },
            enqueue: {
                let enqueued = enqueued.clone();
                Arc::new(move |picked: Vec<DiscoveryTrack>| {
                    let ids: Vec<String> =
                        picked.iter().map(|t| t.key.id.clone()).collect();
                    let minted: Vec<(EntryId, String)> = ids
                        .iter()
                        .enumerate()
                        .map(|(i, id)| (EntryId(9_000 + i as u64), id.clone()))
                        .collect();
                    enqueued.lock().unwrap().extend(ids);
                    Box::pin(async move { minted }) as BoxFut<_>
                })
            },
        };
        let flag = deps.enabled.clone();
        tokio::spawn(run_autoplay(state_rx, cmd_rx, deps));
        Harness { state_tx, cmd_tx, enqueued, fetches, related_rows, flag }
    }

    /// Publishes a state and lets the loop settle.
    async fn publish(h: &Harness, st: PlaybackState) {
        h.state_tx.send(st).unwrap();
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    }

    async fn wait_for(cond: impl Fn() -> bool) {
        for _ in 0..500 {
            if cond() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        panic!("condition not reached");
    }

    #[tokio::test]
    async fn triggers_once_at_tail_with_dedup() {
        let h = harness(true);
        h.cmd_tx.send(enqueue_cmd(&[1, 2], Placement::Append)).unwrap();
        *h.related_rows.lock().unwrap() =
            vec![track("v2"), track("v7"), track("v8")]; // v2 already queued
        publish(&h, state(2, Status::Playing)).await;
        wait_for(|| h.fetches.load(Ordering::SeqCst) == 1).await;
        // Exactly one enqueue, without the already-queued row.
        wait_for(|| !h.enqueued.lock().unwrap().is_empty()).await;
        assert_eq!(*h.enqueued.lock().unwrap(), vec!["v7".to_string(), "v8".to_string()]);
        // A second event for the same entry (seek, volume, re-publish) does
        // not fetch or enqueue again.
        publish(&h, state(2, Status::Playing)).await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(h.fetches.load(Ordering::SeqCst), 1, "once per entry");
        assert_eq!(h.enqueued.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn not_when_off_until_enabled() {
        let h = harness(false);
        h.cmd_tx.send(enqueue_cmd(&[1], Placement::Append)).unwrap();
        *h.related_rows.lock().unwrap() = vec![track("v9")];
        publish(&h, state(1, Status::Playing)).await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(h.fetches.load(Ordering::SeqCst), 0, "off → no fetch");
        // Enabling takes effect on the next event, not retroactively.
        h.flag.store(true, Ordering::SeqCst);
        h.state_tx.send(state(1, Status::Playing)).unwrap();
        wait_for(|| h.fetches.load(Ordering::SeqCst) == 1).await;
    }

    #[tokio::test]
    async fn paused_tail_does_not_fetch() {
        let h = harness(true);
        h.cmd_tx.send(enqueue_cmd(&[1], Placement::Append)).unwrap();
        *h.related_rows.lock().unwrap() = vec![track("v9")];
        publish(&h, state(1, Status::Paused)).await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(h.fetches.load(Ordering::SeqCst), 0, "paused → no fetch");
        // Resume at the tail is a new Playing event → one fetch.
        publish(&h, state(1, Status::Playing)).await;
        wait_for(|| h.fetches.load(Ordering::SeqCst) == 1).await;
    }

    #[tokio::test]
    async fn advance_into_appended_tail_fetches_next() {
        let h = harness(true);
        h.cmd_tx.send(enqueue_cmd(&[1], Placement::Append)).unwrap();
        *h.related_rows.lock().unwrap() = vec![track("v9")];
        publish(&h, state(1, Status::Playing)).await;
        wait_for(|| h.fetches.load(Ordering::SeqCst) == 1).await;
        wait_for(|| !h.enqueued.lock().unwrap().is_empty()).await;
        // The engine auto-advances into the appended entry (mirror adopted
        // it via extend_autoplay) → a new fetch for the new tail.
        publish(&h, state(9_000, Status::Playing)).await;
        wait_for(|| h.fetches.load(Ordering::SeqCst) == 2).await;
    }

    // ---- mirror semantics ----

    #[test]
    fn mirror_append_insert_next_and_clear() {
        let mut m = QueueMirror::default();
        m.apply(&enqueue_cmd(&[1, 2], Placement::Append));
        assert_eq!(m.order, vec![EntryId(1), EntryId(2)]);
        // Cursor follows the watch: playing 1 (queue [1, 2]) is NOT the tail.
        m.set_cursor(Some(EntryId(1)));
        assert!(!m.is_tail(EntryId(1)));
        m.set_cursor(Some(EntryId(2)));
        assert!(m.is_tail(EntryId(2)));
        assert!(!m.is_tail(EntryId(1)));

        // InsertNext goes after the cursor (engine: split at i+1); the
        // cursor is still 1 from the is_tail asserts above.
        m.set_cursor(Some(EntryId(1)));
        m.apply(&enqueue_cmd(&[3], Placement::Next));
        assert_eq!(m.order, vec![EntryId(1), EntryId(3), EntryId(2)]);

        // No cursor → "next" means the front (original order preserved).
        let mut m2 = QueueMirror::default();
        m2.apply(&enqueue_cmd(&[5, 6], Placement::Next));
        assert_eq!(m2.order, vec![EntryId(5), EntryId(6)]);

        // Remote-only ids append after the locals (engine arm).
        let mut m3 = QueueMirror::default();
        m3.apply(&Command::Enqueue {
            tracks: vec![EntryId(1)],
            remote: vec![(EntryId(7), ("youtube".into(), "v7".into()))],
            placement: Placement::Append,
        });
        assert_eq!(m3.order, vec![EntryId(1), EntryId(7)]);
        assert_eq!(m3.known_ids(), ["v7".to_string()].into_iter().collect());

        m3.apply(&Command::ClearQueue);
        assert!(m3.order.is_empty() && m3.keys.is_empty() && m3.cursor.is_none());
    }

    #[test]
    fn pick_related_dedups_and_caps() {
        let mut known: HashSet<String> = ["v1".to_string()].into_iter().collect();
        let rows = vec![track("v1"), track("v2"), track("v3"), track("v4")];
        let picked = pick_related(rows, &mut known, 2);
        assert_eq!(picked.iter().map(|t| t.key.id.as_str()).collect::<Vec<_>>(), ["v2", "v3"], "dedup + cap");
        assert!(pick_related(vec![], &mut known, 10).is_empty());
    }
}
