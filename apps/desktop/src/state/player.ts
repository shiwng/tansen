/**
 * The player store, mirroring the upstream web player's `state/player.ts`
 * contract (same flat fields and actions the shell components read) but
 * backed by the Tauri/Rust task engine instead of YouTube decks.
 *
 * M10: the UI rides the `playback_state` event (full PlaybackState per
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
import type { Track } from '@/data/music/types';
import type { QueueEntry, QueueSource } from '@/playback/queue/timeline';

export const CROSSFADE_CHOICES = [0, 3, 6, 9, 12];

export interface NowPlaying {
  videoId: string;
  title: string;
  artist: string;
  artworkUrl: string | null;
  durationSec: number;
}

/** Persisted user preferences. `autoplay`/`crossfadeSec`/`automix` have no engine yet. */
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
const SETTINGS_KEY = 'bitchord:player'; // upstream-compatible name, kept

type Persisted = {
  tracks: Array<{
    videoId: string;
    title: string;
    artist: string;
    album: string | null;
    durationSec: number | null;
    thumbnail: string | null;
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

let persistTimer: ReturnType<typeof setTimeout> | null = null;
function persistPlayback(get: () => PlayerState) {
  if (persistTimer) clearTimeout(persistTimer);
  persistTimer = setTimeout(() => {
    const s = get();
    const tracks = s.queue.map((e) => ({
      videoId: e.track.videoId,
      title: e.track.title,
      artist: e.track.artists[0]?.name ?? '',
      album: e.track.album?.name ?? null,
      durationSec: e.track.durationSec,
      thumbnail: e.track.thumbnail,
    }));
    const body: Persisted = {
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
  queue: QueueEntry[];
  index: number;
  nowPlaying: NowPlaying | null;
  status: Status;
  error: string | null;
  /** Bumped whenever the position source is replaced (engine restart) — scrubber re-subscribes. */
  deck: number;
  /** The automix blend indicator; always false until phase 6 lands. */
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

  playTracks: (tracks: Track[], startIndex: number, source: QueueSource | null) => void;
  playTrack: (track: Track, source: QueueSource | null) => void;
  playNext: (tracks: Track[], source?: QueueSource | null) => void;
  addToQueue: (tracks: Track[], source?: QueueSource | null) => void;
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

/** The library ids behind a set of Track objects (local tracks are `local:<id>`). */
function idsOf(tracks: Track[]): number[] {
  return tracks.map((t) => Number(t.videoId.replace('local:', '')));
}

function entryIdOf(track: Track): number {
  return Number(track.videoId.replace('local:', ''));
}

function toEntry(track: Track, i: number, source: QueueSource | null): QueueEntry {
  return { entryId: `${track.videoId}#${i}`, track, tier: 'context', source };
}

/** A Track for a queue row: local library files and catalogue rows alike. */
export function trackFromRef(t: {
  id: number; title: string; artist: string; album: string; duration_sec: number; art_path: string | null; video_id?: string | null;
}): Track {
  return {
    videoId: t.video_id ?? `local:${t.id}`,
    title: t.title,
    artists: [{ name: t.artist || 'Unknown Artist', id: null }],
    album: t.album ? { id: null, name: t.album } : null,
    durationSec: t.duration_sec > 0 ? t.duration_sec : null,
    explicit: false,
    thumbnail: t.art_path ? convertFileSrc(t.art_path, 'art') : null,
    index: null,
    plays: null,
  };
}

/** One Command over the `engine_command` bridge (serde externally tagged). */
type CommandDto =
  | { Play: { entry: number } }
  | 'Pause'
  | 'Resume'
  | 'Next'
  | 'Prev'
  | { Seek: { ms: number } }
  | { SetVolume: number }
  | { Enqueue: { tracks: number[]; placement: 'Next' | 'Append' } }
  | 'ClearQueue';

function send(cmd: CommandDto): void {
  invoke('engine_command', { cmd }).catch(() => {
    usePlayer.setState({ status: 'error', error: 'The playback engine is not running' });
  });
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

/** No automix on desktop yet — the seek bar's beat pulse stays off. */
export function getMixBeat(): { phase: number | null } | null {
  return null;
}

/** No HTMLAudioElement on this backend; consumers must use getPlayhead(). */
export function getAudio(): HTMLAudioElement | null {
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
  };
}

export const usePlayer = create<PlayerState>((set, get) => ({
  queue: [],
  index: -1,
  nowPlaying: null,
  status: 'idle',
  error: null,
  deck: 0,
  mixing: false,
  ...loadSettings(),

  playTracks: (tracks, startIndex, _source) => {
    if (!tracks.length) return;
    const ids = idsOf(tracks);
    const start = Math.min(Math.max(startIndex, 0), ids.length - 1);
    send('ClearQueue');
    send({ Enqueue: { tracks: ids, placement: 'Append' } });
    send({ Play: { entry: ids[start]! } });
    set({
      queue: tracks.map((t, i) => toEntry(t, i, null)),
      index: start,
      nowPlaying: nowPlayingFrom(tracks[start]),
      status: 'loading',
      error: null,
    });
    persistPlayback(get);
  },

  playTrack: (track, source) => {
    get().playTracks([track], 0, source);
  },

  playNext: (tracks, _source) => {
    const ids = idsOf(tracks);
    if (!ids.length) return;
    send({ Enqueue: { tracks: ids, placement: 'Next' } });
    set((s) => {
      const at = s.index + 1;
      const queue = [...s.queue.slice(0, at), ...tracks.map((t, i) => toEntry(t, at + i, null)), ...s.queue.slice(at)];
      return { queue };
    });
    persistPlayback(get);
  },

  addToQueue: (tracks, _source) => {
    const ids = idsOf(tracks);
    if (!ids.length) return;
    send({ Enqueue: { tracks: ids, placement: 'Append' } });
    set((s) => ({ queue: [...s.queue, ...tracks.map((t, i) => toEntry(t, s.queue.length + i, null))] }));
    persistPlayback(get);
  },

  jumpTo: (i) => {
    const entry = get().queue[i];
    if (!entry) return;
    send({ Play: { entry: entryIdOf(entry.track) } });
    set({ index: i, nowPlaying: nowPlayingFrom(entry.track), status: 'loading', error: null });
  },

  // The M5 Command set has no queue-edit primitives; the old IPC commands
  // are gone. The controls stay visible (layout unchanged) and report
  // honestly until queue editing lands as commands.
  removeAt: (i) => {
    console.warn('queue editing is not wired to the task engine yet (remove at', i, ')');
  },
  move: (from, to) => {
    console.warn('queue editing is not wired to the task engine yet (move', from, '→', to, ')');
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
    else if (s.queue[s.index]) send({ Play: { entry: entryIdOf(s.queue[s.index]!.track) } });
  },

  seek: (sec) => {
    playhead.posMs = Math.round(sec * 1000);
    playhead.receivedAt = performance.now();
    playhead.rate = get().status === 'playing' ? 1 : 0;
    send({ Seek: { ms: Math.round(sec * 1000) } });
  },

  toggleShuffle: () => updateSettings(set, get, { shuffle: !get().shuffle }),
  // Repeat stays a stored preference: the task engine's Command set has no
  // repeat mode yet (the old queue algebra retired with the old path).
  cycleRepeat: () => {
    const order = ['off', 'all', 'one'] as const;
    updateSettings(set, get, { repeat: order[(order.indexOf(get().repeat) + 1) % order.length] });
  },
  setAutoplay: (autoplay) => updateSettings(set, get, { autoplay }),
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
  setCrossfade: (crossfadeSec) => updateSettings(set, get, { crossfadeSec }),
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

let started = false;
/** Wire engine events to the store. Called once from main.tsx. */
export async function initPlayerStore(): Promise<void> {
  if (started) return;
  started = true;

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

    usePlayer.setState((s) => {
      const entry = st.entry;
      const idx = entry !== null ? s.queue.findIndex((en) => entryIdOf(en.track) === entry) : -1;
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
      return patch;
    });
  });

  // Restart persistence: re-seed the engine queue from the mirror, paused at
  // the saved position (Play+Pause+Seek — the Command set has no
  // load-paused; the blip is milliseconds).
  try {
    const saved = JSON.parse(localStorage.getItem(PERSIST_KEY) ?? 'null') as Persisted | null;
    if (saved && saved.tracks.length) {
      const ids = saved.tracks.map((t) => Number(t.videoId.replace('local:', ''))).filter((n) => Number.isFinite(n));
      const index = Math.min(Math.max(saved.index, 0), ids.length - 1);
      const tracks: Track[] = saved.tracks.map((t) => ({
        videoId: t.videoId,
        title: t.title,
        artists: [{ name: t.artist || 'Unknown Artist', id: null }],
        album: t.album ? { id: null, name: t.album } : null,
        durationSec: t.durationSec,
        explicit: false,
        thumbnail: t.thumbnail,
        index: null,
        plays: null,
      }));
      usePlayer.setState({
        queue: tracks.map((t, i) => toEntry(t, i, null)),
        index,
        nowPlaying: nowPlayingFrom(tracks[index]),
        volume: saved.volume,
        muted: saved.muted,
      });
      if (ids.length) {
        send({ Enqueue: { tracks: ids, placement: 'Append' } });
        send({ Play: { entry: ids[index]! } });
        send('Pause');
        if (saved.posMs > 500) send({ Seek: { ms: saved.posMs } });
        send({ SetVolume: saved.muted ? 0 : saved.volume });
        playhead.posMs = saved.posMs;
        playhead.receivedAt = performance.now();
        playhead.rate = 0;
      }
    }
  } catch {
    // No restorable state: start clean.
  }
}
