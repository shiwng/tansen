/**
 * The player store, mirroring the upstream web player's `state/player.ts`
 * contract (same flat fields and actions the shell components read) but
 * backed by the Tauri/Rust task engine instead of YouTube decks.
 *
 * The UI rides the `playback_state` event (full PlaybackState per
 * engine publish) and forwards every action as a `Command` over
 * `engine_command`. Position is interpolated locally:
 * `pos = anchor.pos_ms + (now − receivedAt) × rate` — the engine publishes
 * on changes only, no polling. The queue body never crosses IPC: this store
 * keeps its own mirror, rebuilt from persisted state on startup.
 *
 * The legacy `player://event` path and its IPC commands are gone.
 */
import { create } from 'zustand';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { mapStreamInfo, type StreamInfoDto, type StreamInfoView } from '@/lib/streamInfo';
import { mergeTrackRows, type QueueRow } from '@/lib/queueRows';
import type { Track } from '@/data/music/types';
import type { QueueEntry, QueueSource } from '@/playback/queue/timeline';

/** A mirror queue entry: the timeline shape plus the engine EntryId. */
export interface PlayerQueueEntry extends QueueEntry {
  /** Local: library track id; remote: `remote_tracks` row id. */
  engineId: number;
}

export const CROSSFADE_CHOICES = [0, 3, 6, 9, 12];

export interface NowPlaying {
  videoId: string;
  title: string;
  artist: string;
  artworkUrl: string | null;
  durationSec: number;
  /** `radio` marks a live broadcast: no duration, no seeking, LIVE badge. */
  sourceKind?: string;
}

/** Persisted user preferences. Crossfade mirrors the Rust audio settings; autoplay mirrors its own Rust key. */
interface Settings {
  shuffle: boolean;
  repeat: 'off' | 'all' | 'one';
  autoplay: boolean;
  volume: number;
  muted: boolean;
  crossfadeSec: number;
  automix: boolean;
  automixBars: number;
  showNerdStats: boolean;
}

const DEFAULT_SETTINGS: Settings = {
  shuffle: false,
  repeat: 'off',
  autoplay: false,
  volume: 1,
  muted: false,
  crossfadeSec: 0,
  automix: false,
  automixBars: 16,
  showNerdStats: false,
};

/** Player settings + the queue mirror: what a restart restores. */
const PERSIST_KEY = 'tansen:playback';
const SETTINGS_KEY = 'tansen:player';

const PERSIST_VERSION = 1;

type Persisted = {
  /** Bumped when the payload shape changes; older state is discarded. */
  v: typeof PERSIST_VERSION;
  tracks: Array<{
    videoId: string;
    kind: 'local' | 'remote';
    /** Remote rows: the source kind (`youtube` / `addon`). */
    sourceKind?: string;
    /** The engine EntryId at persist time (remote ids are re-upserted on restore). */
    engineId?: number;
    title: string;
    artist: string;
    album: string | null;
    durationSec: number | null;
    thumbnail: string | null;
    /** Remote rows: the original catalogue artwork URL. */
    artworkUrl?: string | null;
  }>;
  index: number;
  posMs: number;
  volume: number;
  muted: boolean;
};

function loadSettings(): Settings {
  try {
    return { ...DEFAULT_SETTINGS, ...(JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? '{}') as Partial<Settings>) };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

function saveSettings(state: Settings) {
  localStorage.setItem(SETTINGS_KEY, JSON.stringify(state));
}

/** The saved queue mirror, or null: a corrupt or older payload never throws. */
export function parsePersisted(raw: string | null): Persisted | null {
  try {
    const saved = JSON.parse(raw ?? 'null') as Partial<Persisted> | null;
    if (!saved || saved.v !== PERSIST_VERSION || !Array.isArray(saved.tracks)) return null;
    const tracks = saved.tracks.filter(
      (t) => t && typeof t.videoId === 'string' && typeof t.title === 'string',
    );
    return tracks.length ? { ...(saved as Persisted), tracks } : null;
  } catch {
    return null;
  }
}

let persistTimer: ReturnType<typeof setTimeout> | null = null;
function persistPlayback(get: () => PlayerState) {
  if (persistTimer) clearTimeout(persistTimer);
  persistTimer = setTimeout(() => {
    const s = get();
    const tracks = s.queue.map((e) => ({
      videoId: e.track.videoId,
      kind: e.track.videoId.startsWith('local:') ? ('local' as const) : ('remote' as const),
      sourceKind: e.track.sourceKind,
      engineId: e.engineId,
      title: e.track.title,
      artist: e.track.artists[0]?.name ?? '',
      album: e.track.album?.name ?? null,
      durationSec: e.track.durationSec,
      thumbnail: e.track.thumbnail,
      artworkUrl: e.track.artworkUrl ?? null,
    }));
    const body: Persisted = {
      v: PERSIST_VERSION,
      tracks,
      index: s.index,
      posMs: Math.round(playhead.posMs),
      volume: s.volume,
      muted: s.muted,
    };
    try {
      localStorage.setItem(PERSIST_KEY, JSON.stringify(body));
    } catch {
      // quota/private mode: restore simply won't have anything to offer
    }
  }, 300);
}

type Status = 'idle' | 'loading' | 'playing' | 'paused' | 'buffering' | 'error';

interface PlayerState {
  queue: PlayerQueueEntry[];
  index: number;
  nowPlaying: NowPlaying | null;
  status: Status;
  error: string | null;
  streamInfo: StreamInfoView | null;
  /** Bumped whenever the position source is replaced (engine restart) — scrubber re-subscribes. */
  deck: number;
  /** Bumped on every seek, so a paused seek (no frame loop, no engine publish needed) is observable. */
  seekSeq: number;
  /** The automix blend indicator; the engine owns the actual fade. */
  mixing: boolean;
  // Persisted settings, flat as upstream keeps them.
  shuffle: boolean;
  repeat: 'off' | 'all' | 'one';
  autoplay: boolean;
  volume: number;
  muted: boolean;
  crossfadeSec: number;
  automix: boolean;
  automixBars: number;
  showNerdStats: boolean;

  playTracks: (tracks: Track[], startIndex: number, source: QueueSource | null) => Promise<void>;
  playTrack: (track: Track, source: QueueSource | null) => Promise<void>;
  playNext: (tracks: Track[], source?: QueueSource | null) => Promise<void>;
  addToQueue: (tracks: Track[], source?: QueueSource | null) => Promise<void>;
  jumpTo: (i: number) => void;
  removeAt: (i: number) => void;
  move: (from: number, to: number) => void;
  clearUpNext: () => void;
  next: () => void;
  previous: () => void;
  togglePlay: () => void;
  seek: (sec: number) => void;
  toggleShuffle: () => void;
  cycleRepeat: () => void;
  setAutoplay: (b: boolean) => void;
  setVolume: (n: number) => void;
  toggleMute: () => void;
  setCrossfade: (n: number) => void;
  setAutomix: (b: boolean) => void;
  setAutomixBars: (n: number) => void;
  setShowNerdStats: (b: boolean) => void;
  refresh: () => Promise<void>;
}

/** A remote (source-keyed) track: anything without the `local:` prefix. */
export function isRemoteTrack(track: Track): boolean {
  return !track.videoId.startsWith('local:');
}

/** The RemoteKey ([source, id]) a remote track maps to; null for local. */
function remoteKeyOf(track: Track): [string, string] | null {
  return isRemoteTrack(track) ? [track.sourceKind ?? 'youtube', track.videoId] : null;
}

function toEntry(track: Track, i: number, source: QueueSource | null, engineId: number): PlayerQueueEntry {
  return { entryId: `${track.videoId}#${i}`, track, tier: 'context', source, engineId };
}

/** A prepared entry: the track plus its resolved engine EntryId. */
interface PreparedEntry {
  track: Track;
  engineId: number;
}

/** One autoplay row announced by the backend. */
interface AutoplayRow {
  engineId: number;
  videoId: string;
  title: string;
  artist: string;
  album: string | null;
  durationSec: number | null;
  thumbnailUrl: string | null;
}

/**
 * Resolves the engine EntryId for each track — the library id for local
 * tracks, or the `remote_tracks` row id returned by the upsert for remote
 * ones (metadata written through the single DB writer BEFORE any command).
 */
async function prepareEngineIds(tracks: Track[]): Promise<PreparedEntry[]> {
  return Promise.all(
    tracks.map(async (track): Promise<PreparedEntry> => {
      if (!isRemoteTrack(track)) {
        const id = Number(track.videoId.slice('local:'.length));
        return { track, engineId: Number.isFinite(id) && id > 0 ? id : 0 };
      }
      // Tauri names command arguments in camelCase; snake_case keys are
      // rejected with "missing required key" before the command body runs.
      const engineId = await invoke<number>('remote_track_upsert', {
        keySource: track.sourceKind ?? 'youtube',
        keyId: track.videoId,
        title: track.title,
        artist: track.artists[0]?.name ?? '',
        album: track.album?.name ?? null,
        durationSec: track.durationSec ?? 0,
        artworkUrl: track.artworkUrl?.startsWith('https://') ? track.artworkUrl : null,
      });
      return { track, engineId };
    }),
  );
}

/** The Enqueue `remote` list for prepared entries ([rowId, [source, id]]). */
function remotePairsOf(prepared: PreparedEntry[]): Array<[number, [string, string]]> {
  const pairs: Array<[number, [string, string]]> = [];
  for (const p of prepared) {
    const key = remoteKeyOf(p.track);
    if (key && p.engineId > 0) pairs.push([p.engineId, key]);
  }
  return pairs;
}

/** A Track for a queue row: local library files and catalogue rows alike. */
export function trackFromRef(t: {
  id: number; title: string; artist: string; album: string; duration_sec: number; art_path: string | null; video_id?: string | null;
  key_source?: string | null; artwork_url?: string | null;
}): Track {
  return {
    videoId: t.video_id ?? `local:${t.id}`,
    title: t.title,
    artists: [{ name: t.artist || 'Unknown Artist', id: null }],
    album: t.album ? { id: null, name: t.album } : null,
    durationSec: t.duration_sec > 0 ? t.duration_sec : null,
    explicit: false,
    thumbnail: t.art_path ? convertFileSrc(t.art_path, 'art') : (t.artwork_url ?? null),
    index: null,
    plays: null,
    // Imported Spotify rows with no YouTube Music match: shown, never played.
    unavailable: t.key_source === 'spotify' ? 'missing' : undefined,
    sourceKind: t.key_source ?? undefined,
    artworkUrl: t.artwork_url ?? null,
  };
}

/** One Command over the `engine_command` bridge (serde externally tagged). */
type CommandDto =
  | { Play: { entry: number; remote?: [string, string] | null } }
  | 'Pause'
  | 'Resume'
  | 'Next'
  | 'Prev'
  | { Seek: { ms: number } }
  | { SetVolume: number }
  | {
      Enqueue: {
        tracks: number[];
        /** Remote row ids paired with their TrackKeys. */
        remote?: Array<[number, [string, string]]>;
        placement: 'Next' | 'Append';
      };
    }
  | { Remove: { entry: number } }
  | { Move: { entry: number; to: number } }
  | 'ClearQueue'
  | { SetRepeat: 'Off' | 'All' | 'One' };

/** Maps the store's lowercase mode to the engine's RepeatMode enum. */
function repeatMode(mode: 'off' | 'all' | 'one'): 'Off' | 'All' | 'One' {
  return mode === 'all' ? 'All' : mode === 'one' ? 'One' : 'Off';
}

/** Sends one command; resolves false when the engine refused it (error shown). */
async function send(cmd: CommandDto): Promise<boolean> {
  try {
    await invoke('engine_command', { cmd });
    return true;
  } catch {
    usePlayer.setState({ status: 'error', error: 'The playback engine is not running' });
    return false;
  }
}

/** Freshest position: the last published anchor, advanced by the local clock. */
const playhead = { posMs: 0, receivedAt: 0, rate: 0 };
const endedListeners = new Set<() => void>();
export function onTrackEnded(listener: () => void): () => void {
  endedListeners.add(listener);
  return () => endedListeners.delete(listener);
}

/** The active audio clock equivalent: published anchor + local interpolation. */
export function getPlayhead(): { sec: number; paused: boolean } | null {
  if (playhead.receivedAt === 0) return null;
  const drift = playhead.rate === 0 ? 0 : (performance.now() - playhead.receivedAt) * playhead.rate;
  return { sec: (playhead.posMs + drift) / 1000, paused: playhead.rate === 0 };
}

/** No beat analysis on desktop: the seek bar's beat pulse stays off. */
export function getMixBeat(): { phase: number | null } | null {
  return null;
}

function nowPlayingFrom(track: Track | undefined): NowPlaying | null {
  if (!track) return null;
  return {
    videoId: track.videoId,
    title: track.title,
    artist: track.artists[0]?.name ?? 'Unknown Artist',
    artworkUrl: track.thumbnail,
    durationSec: track.durationSec ?? 0,
    sourceKind: track.sourceKind,
  };
}

export const usePlayer = create<PlayerState>((set, get) => ({
  queue: [],
  streamInfo: null,
  index: -1,
  nowPlaying: null,
  status: 'idle',
  error: null,
  deck: 0,
  seekSeq: 0,
  mixing: false,
  ...loadSettings(),

  playTracks: async (tracks, startIndex, _source) => {
    if (!tracks.length) return;
    let prepared: PreparedEntry[];
    try {
      prepared = await prepareEngineIds(tracks);
    } catch {
      usePlayer.setState({ status: 'error', error: 'Could not register remote track metadata' });
      return;
    }
    if (prepared.some((p) => p.engineId <= 0)) return;
    // Shuffle reorders the selection around the track the user pressed: that
    // track starts, everything else follows in random order.
    const chosen = Math.min(Math.max(startIndex, 0), prepared.length - 1);
    if (get().shuffle && prepared.length > 1) {
      const head = prepared[chosen]!;
      const tail = prepared.filter((_, i) => i !== chosen);
      for (let i = tail.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1));
        [tail[i], tail[j]] = [tail[j]!, tail[i]!];
      }
      prepared = [head, ...tail];
    }
    const ids = prepared.map((p) => p.engineId);
    const remote = remotePairsOf(prepared);
    const start = get().shuffle ? 0 : Math.min(Math.max(startIndex, 0), ids.length - 1);
    const startKey = remote.find(([id]) => id === ids[start])?.[1] ?? null;
    // Sequential: the engine applies commands in send order, so the entry
    // must be enqueued before Play names it.
    if (!(await send('ClearQueue'))) return;
    if (!(await send({ Enqueue: { tracks: ids, remote, placement: 'Append' } }))) return;
    if (!(await send({ Play: { entry: ids[start]!, remote: startKey } }))) return;
    set({
      queue: prepared.map((p, i) => toEntry(p.track, i, null, p.engineId)),
      index: start,
      nowPlaying: nowPlayingFrom(prepared[start]!.track),
      status: 'loading',
      error: null,
    });
    persistPlayback(get);
  },

  playTrack: async (track, source) => {
    await get().playTracks([track], 0, source);
  },

  playNext: async (tracks, _source) => {
    if (!tracks.length) return;
    let prepared: PreparedEntry[];
    try {
      prepared = await prepareEngineIds(tracks);
    } catch {
      return;
    }
    if (prepared.some((p) => p.engineId <= 0)) return;
    const ids = prepared.map((p) => p.engineId);
    const remote = remotePairsOf(prepared);
    send({ Enqueue: { tracks: ids, remote, placement: 'Next' } });
    set((s) => {
      const at = s.index + 1;
      const queue = [
        ...s.queue.slice(0, at),
        ...prepared.map((p, i) => toEntry(p.track, at + i, null, p.engineId)),
        ...s.queue.slice(at),
      ];
      return { queue };
    });
    persistPlayback(get);
  },

  addToQueue: async (tracks, _source) => {
    if (!tracks.length) return;
    let prepared: PreparedEntry[];
    try {
      prepared = await prepareEngineIds(tracks);
    } catch {
      return;
    }
    if (prepared.some((p) => p.engineId <= 0)) return;
    const ids = prepared.map((p) => p.engineId);
    const remote = remotePairsOf(prepared);
    send({ Enqueue: { tracks: ids, remote, placement: 'Append' } });
    set((s) => ({
      queue: [...s.queue, ...prepared.map((p, i) => toEntry(p.track, s.queue.length + i, null, p.engineId))],
    }));
    persistPlayback(get);
  },

  jumpTo: (i) => {
    const entry = get().queue[i];
    if (!entry) return;
    send({ Play: { entry: entry.engineId, remote: remoteKeyOf(entry.track) } });
    set({ index: i, nowPlaying: nowPlayingFrom(entry.track), status: 'loading', error: null });
  },

  removeAt: (i) => {
    const snapshot = get();
    const entry = snapshot.queue[i];
    if (!entry || i === snapshot.index) return;
    send({ Remove: { entry: entry.engineId } });
    set((s) => ({
      queue: s.queue.filter((_, at) => at !== i),
      index: i < s.index ? s.index - 1 : s.index,
    }));
    persistPlayback(get);
  },
  move: (from, to) => {
    const snapshot = get();
    const entry = snapshot.queue[from];
    const target = Math.max(0, Math.min(to, snapshot.queue.length - 1));
    if (!entry || from === snapshot.index || from === target) return;
    send({ Move: { entry: entry.engineId, to: target } });
    set((s) => {
      const queue = [...s.queue];
      const [moved] = queue.splice(from, 1);
      if (!moved) return s;
      queue.splice(target, 0, moved);
      let index = s.index;
      if (from < index && target >= index) index -= 1;
      else if (from > index && target <= index) index += 1;
      return { queue, index };
    });
    persistPlayback(get);
  },

  clearUpNext: () => {
    send('ClearQueue');
    set({ queue: [], index: -1, nowPlaying: null, status: 'idle' });
    persistPlayback(get);
  },

  next: () => send('Next'),
  previous: () => send('Prev'),

  togglePlay: () => {
    const s = get();
    if (s.status === 'playing') send('Pause');
    else if (s.status === 'paused') send('Resume');
    else if (s.queue[s.index]) {
      const entry = s.queue[s.index]!;
      send({ Play: { entry: entry.engineId, remote: remoteKeyOf(entry.track) } });
    }
  },

  seek: (sec) => {
    playhead.posMs = Math.round(sec * 1000);
    playhead.receivedAt = performance.now();
    playhead.rate = get().status === 'playing' ? 1 : 0;
    // Publish the seek: while paused nothing else will (the engine may only
    // ack later), and position readers ride store publishes.
    set({ seekSeq: get().seekSeq + 1 });
    send({ Seek: { ms: Math.round(sec * 1000) } });
  },

  toggleShuffle: () => updateSettings(set, get, { shuffle: !get().shuffle }),
  // Repeat is engine-owned: store the preference for the UI and tell the
  // engine, so auto-advance is correct even while Settings is closed.
  cycleRepeat: () => {
    const order = ['off', 'all', 'one'] as const;
    const repeat = order[(order.indexOf(get().repeat) + 1) % order.length]!;
    updateSettings(set, get, { repeat });
    void send({ SetRepeat: repeatMode(repeat) });
  },
  // The one autoplay setting: mirrors the Rust key (autoplay_set updates the
  // stored value AND the task's flag), hydrated in initPlayerStore — the
  // store is its only renderer-side owner.
  setAutoplay: (autoplay) => {
    updateSettings(set, get, { autoplay });
    void invoke('autoplay_set', { on: autoplay }).catch(() => updateSettings(set, get, { autoplay: !autoplay }));
  },
  setVolume: (n) => {
    const volume = Math.min(1, Math.max(0, n));
    updateSettings(set, get, { volume, muted: false });
    send({ SetVolume: volume });
  },
  toggleMute: () => {
    const muted = !get().muted;
    updateSettings(set, get, { muted });
    send({ SetVolume: muted ? 0 : get().volume });
  },
  // Crossfade/auto-mix duration is owned by the Rust audio settings (the same
  // value the pipeline and Settings read). Mirror it locally for components
  // that ask the player store, and persist through the audio command.
  setCrossfade: (crossfadeSec) => {
    updateSettings(set, get, { crossfadeSec });
    void invoke('audio_settings_set', { patch: { crossfadeMs: Math.round(crossfadeSec * 1000) } }).catch(() => {});
  },
  setAutomix: (automix) => updateSettings(set, get, { automix }),
  setAutomixBars: (automixBars) => updateSettings(set, get, { automixBars }),
  setShowNerdStats: (showNerdStats) => updateSettings(set, get, { showNerdStats }),

  // No server-side snapshot exists (the queue body stays in the engine);
  // the mirror in this store is the source of truth for the UI.
  refresh: async () => {},
}));

function updateSettings(set: (p: Partial<PlayerState>) => void, get: () => PlayerState, patch: Partial<Settings>) {
  set(patch as Partial<PlayerState>);
  const s = get();
  saveSettings({
    shuffle: s.shuffle, repeat: s.repeat, autoplay: s.autoplay, volume: s.volume, muted: s.muted,
    crossfadeSec: s.crossfadeSec, automix: s.automix, automixBars: s.automixBars, showNerdStats: s.showNerdStats,
  });
}

/** Serde's external tagging of the engine Status. */
type EngineStatus = 'Idle' | 'Resolving' | 'Buffering' | 'Playing' | 'Paused' | { Error: string };

function statusOf(raw: EngineStatus): { status: Status; message?: string } {
  if (raw === 'Idle') return { status: 'idle' };
  if (raw === 'Resolving') return { status: 'loading' };
  if (raw === 'Buffering') return { status: 'buffering' };
  if (raw === 'Playing') return { status: 'playing' };
  if (raw === 'Paused') return { status: 'paused' };
  const kind = typeof raw === 'object' && raw && 'Error' in raw ? raw.Error : 'unknown';
  return { status: 'error', message: `Playback failed (${kind})` };
}

const RADIO_RETRIES = 3;
let radioAttempts = 0;
let radioTimer: ReturnType<typeof setTimeout> | null = null;

let started = false;
/** Wire engine events to the store. Called once from main.tsx. */
export async function initPlayerStore(): Promise<void> {
  if (started) return;
  started = true;

  // Hydrate the autoplay setting from its owner (the Rust key) so the
  // store's mirror is right before Settings or hasNext read it.
  void invoke<boolean>('autoplay_get')
    .then((on) => {
      if (usePlayer.getState().autoplay !== on) updateSettings(usePlayer.setState, usePlayer.getState, { autoplay: on });
    })
    .catch(() => {});

  // Repeat is engine-owned; restore the stored cycle before any queue work.
  void send({ SetRepeat: repeatMode(usePlayer.getState().repeat) });

  // Crossfade/auto-mix duration lives in SQLite (audio settings). Mirror it
  // into the player store so legacy components see the same value.
  void invoke<{ crossfadeMs?: number }>('audio_settings_get')
    .then((settings) => {
      if (typeof settings?.crossfadeMs === 'number') {
        const crossfadeSec = settings.crossfadeMs / 1000;
        if (usePlayer.getState().crossfadeSec !== crossfadeSec) {
          updateSettings(usePlayer.setState, usePlayer.getState, { crossfadeSec });
        }
      }
    })
    .catch(() => {});

  // On a new `queue_rev`, refresh the mirror's metadata from the
  // DB for BOTH entry kinds — local library ids and remote row ids (order
  // and unknown ids stay as the mirror holds them).
  let lastQueueRev = -1;
  let lastStreamInfoEntry: number | null = null;
  let streamInfoFetch = 0;
  let refreshToken = 0;
  const refreshQueueRows = async () => {
    const token = ++refreshToken;
    const snapshot = usePlayer.getState();
    const ids: number[] = [];
    const remote_ids: number[] = [];
    for (const en of snapshot.queue) {
      if (en.engineId <= 0) continue;
      if (en.track.videoId.startsWith('local:')) ids.push(en.engineId);
      else remote_ids.push(en.engineId);
    }
    if (!ids.length && !remote_ids.length) return;
    try {
      const rows = await invoke<QueueRow[]>('queue_tracks_get', { ids, remoteIds: remote_ids });
      if (token !== refreshToken) return; // a newer rev superseded this fetch
      // Remote artwork: route through the allowlisted fetch command once
      // (best-effort), so queue rows show cached local art:// paths.
      const remoteThumbs = new Map<number, string | null>();
      await Promise.all(
        rows
          .filter((r) => r.kind === 'remote' && r.artwork_url)
          .map(async (r) => {
            try {
              const path = await invoke<string>('fetch_artwork', { url: r.artwork_url });
              remoteThumbs.set(r.id, convertFileSrc(path, 'art'));
            } catch {
              remoteThumbs.set(r.id, null);
            }
          }),
      );
      usePlayer.setState((s) => {
        const tracks = mergeTrackRows(
          s.queue.map((en) => ({ track: en.track, engineId: en.engineId })),
          rows,
          (row) =>
            row.kind === 'remote'
              ? remoteThumbs.get(row.id) ?? null
              : row.art_path
                ? convertFileSrc(row.art_path, 'art')
                : null,
        );
        const queue = s.queue.map((en, i) => ({ ...en, track: tracks[i] ?? en.track }));
        const current = s.index >= 0 ? queue[s.index]?.track : undefined;
        return {
          queue,
          nowPlaying: current ? nowPlayingFrom(current) : s.nowPlaying,
        };
      });
      persistPlayback(usePlayer.getState);
    } catch {
      // The refresh is an upgrade; the mirror stays authoritative on failure.
    }
  };

  await listen<{
    rev: number;
    status: EngineStatus;
    entry: number | null;
    queue_rev: number;
    anchor: { pos_ms: number; rate: number };
    volume: number;
  }>('playback_state', (e) => {
    const st = e.payload;
    const mapped = statusOf(st.status);
    playhead.posMs = st.anchor.pos_ms;
    playhead.receivedAt = performance.now();
    playhead.rate = st.anchor.rate;
    if (st.queue_rev !== lastQueueRev) {
      lastQueueRev = st.queue_rev;
      void refreshQueueRows();
    }

    // Stream-info fetch on entry change — event-driven, no polling. The
    // first publish for an entry is Resolving (nothing resolved yet); when
    // the answer is Playing and no info is known, one re-fetch covers the
    // resolve that just landed (and mid-play re-resolve upgrades).
    const entryChanged = st.entry !== lastStreamInfoEntry;
    if (entryChanged) {
      lastStreamInfoEntry = st.entry;
      streamInfoFetch += 1;
      usePlayer.setState({ streamInfo: null });
    }
    // One in-flight query per publish; stale responses are dropped by token.
    if (st.entry !== null && (entryChanged || (st.status === 'Playing' && !usePlayer.getState().streamInfo))) {
      const token = ++streamInfoFetch;
      void invoke<StreamInfoDto | null>('stream_info_get', { entry: st.entry })
        .then((dto) => {
          if (dto && token === streamInfoFetch) {
            usePlayer.setState({ streamInfo: mapStreamInfo(dto) });
          }
        })
        .catch(() => {
          /* read-only query; absence is fine */
        });
    }

    usePlayer.setState((s) => {
      const entry = st.entry;
      const idx = entry !== null ? s.queue.findIndex((en) => en.engineId === entry) : -1;
      const current = idx >= 0 ? s.queue[idx]!.track : undefined;
      const patch: Partial<PlayerState> = {
        status: mapped.status,
        error: mapped.message ?? null,
      };
      if (current) patch.nowPlaying = nowPlayingFrom(current);
      if (idx >= 0) patch.index = idx;
      // Queue exhausted: the engine fell to Idle after the last track.
      if (mapped.status === 'idle' && s.status === 'playing') {
        endedListeners.forEach((l) => l());
      }
      if (mapped.status === 'playing') radioAttempts = 0;
      if (mapped.status === 'error' && current?.sourceKind === 'radio') {
        // A broadcast dropped mid-stream: retry the same station a bounded
        // number of times, honestly labelled, instead of silently skipping.
        if (radioAttempts < RADIO_RETRIES && !radioTimer) {
          radioAttempts += 1;
          const attempt = radioAttempts;
          patch.error = `Stream lost — reconnecting (${attempt}/${RADIO_RETRIES})…`;
          radioTimer = setTimeout(() => {
            radioTimer = null;
            const live = usePlayer.getState().queue[usePlayer.getState().index];
            if (live?.track.sourceKind === 'radio') {
              void usePlayer.getState().playTrack(live.track, null);
            }
          }, 1500 * attempt);
        }
      }
      return patch;
    });
  });

  // Autoplay rows appended engine-side. The queue_rev refresh above
  // only resolves ids the mirror already knows, so the backend announces
  // each appended row; adopt them into the mirror here (remote artwork
  // routed through the allowlisted fetch, as everywhere else).
  void listen<AutoplayRow[]>('autoplay_enqueued', async (e) => {
    const entries = await Promise.all(
      e.payload.map(async (r): Promise<PlayerQueueEntry> => {
        let thumbnail: string | null = null;
        if (r.thumbnailUrl) {
          try {
            thumbnail = convertFileSrc(await invoke<string>('fetch_artwork', { url: r.thumbnailUrl }), 'art');
          } catch {
            thumbnail = null;
          }
        }
        const track: Track = {
          videoId: r.videoId,
          title: r.title,
          artists: [{ name: r.artist || 'Unknown Artist', id: null }],
          album: r.album ? { id: null, name: r.album } : null,
          durationSec: r.durationSec && r.durationSec > 0 ? r.durationSec : null,
          explicit: false,
          thumbnail,
          index: null,
          plays: null,
          sourceKind: 'youtube',
          artworkUrl: r.thumbnailUrl,
        };
        return { entryId: `${r.videoId}#a${r.engineId}`, track, tier: 'context', source: null, engineId: r.engineId };
      }),
    );
    usePlayer.setState((s) => ({ queue: [...s.queue, ...entries] }));
  });

  // Restart persistence: re-seed the engine queue from the mirror, paused at
  // the saved position (Play+Pause+Seek — the Command set has no
  // load-paused; the blip is milliseconds). Remote rows are re-upserted to
  // recover their row ids and re-seeded with fresh TrackKeys only — no
  // stored stream URLs ever restore; the engine resolves fresh.
  try {
    const saved = parsePersisted(localStorage.getItem(PERSIST_KEY));
    if (saved && saved.tracks.length) {
      const rebuilt: PreparedEntry[] = [];
      for (const t of saved.tracks) {
        if (!t.videoId.startsWith('local:')) {
          try {
            const engineId = await invoke<number>('remote_track_upsert', {
              keySource: t.sourceKind ?? 'youtube',
              keyId: t.videoId,
              title: t.title,
              artist: t.artist,
              album: t.album ?? null,
              durationSec: t.durationSec ?? 0,
              artworkUrl: t.artworkUrl?.startsWith('https://') ? t.artworkUrl : null,
            });
            rebuilt.push({
              track: {
                videoId: t.videoId,
                title: t.title,
                artists: [{ name: t.artist || 'Unknown Artist', id: null }],
                album: t.album ? { id: null, name: t.album } : null,
                durationSec: t.durationSec,
                explicit: false,
                thumbnail: t.thumbnail,
                index: null,
                plays: null,
                sourceKind: t.sourceKind,
                artworkUrl: t.artworkUrl ?? null,
              },
              engineId,
            });
          } catch {
            // A remote row that cannot be re-registered is dropped.
          }
        } else {
          const id = Number(t.videoId.replace('local:', ''));
          if (!Number.isFinite(id) || id <= 0) continue;
          rebuilt.push({
            track: {
              videoId: t.videoId,
              title: t.title,
              artists: [{ name: t.artist || 'Unknown Artist', id: null }],
              album: t.album ? { id: null, name: t.album } : null,
              durationSec: t.durationSec,
              explicit: false,
              thumbnail: t.thumbnail,
              index: null,
              plays: null,
            },
            engineId: id,
          });
        }
      }
      if (!rebuilt.length) return;
      const ids = rebuilt.map((r) => r.engineId);
      const remote = remotePairsOf(rebuilt);
      const index = Math.min(Math.max(saved.index, 0), ids.length - 1);
      const tracks = rebuilt.map((r) => r.track);
      usePlayer.setState({
        queue: tracks.map((t, i) => toEntry(t, i, null, ids[i]!)),
        index,
        nowPlaying: nowPlayingFrom(tracks[index]),
        volume: saved.volume,
        muted: saved.muted,
      });
      const startKey = remote.find(([id]) => id === ids[index])?.[1] ?? null;
      await send({ Enqueue: { tracks: ids, remote, placement: 'Append' } });
      await send({ Play: { entry: ids[index]!, remote: startKey } });
      await send('Pause');
      if (saved.posMs > 500) await send({ Seek: { ms: saved.posMs } });
      await send({ SetVolume: saved.muted ? 0 : saved.volume });
      playhead.posMs = saved.posMs;
      playhead.receivedAt = performance.now();
      playhead.rate = 0;
    }
  } catch {
    // No restorable state: start clean.
  }
}
