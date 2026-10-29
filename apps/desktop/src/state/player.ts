/**
 * The player store, mirroring the upstream web player's `state/player.ts`
 * contract (same flat fields and actions the shell components read) but
 * backed by the Tauri/Rust audio engine instead of YouTube decks.
 *
 * Queue and transport live in Rust; this store holds the projected state the
 * UI renders and forwards actions as commands. Local tracks enter as
 * `TrackRef`s from the library and are mirrored into the upstream `Track`
 * shape so vendored components work unmodified.
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

const SETTINGS_KEY = 'bitchord:player';

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

/** Catalogue rows keep their YouTube video ids and ride the remote path. */
function isRemote(tracks: Track[]): boolean {
  return tracks.length > 0 && !tracks[0]!.videoId.startsWith('local:');
}

function playRemoteCommand(tracks: Track[], start: number, mode: 'load' | 'insert-next' | 'append'): void {
  invoke('player_play_remote', {
    args: {
      tracks: tracks.map((t) => ({
        videoId: t.videoId,
        title: t.title,
        artist: t.artists[0]?.name ?? '',
        album: t.album?.name ?? null,
        duration_sec: t.durationSec ?? 0,
        thumbnailUrl: t.thumbnail,
      })),
      start,
      mode,
    },
  }).catch((e) => usePlayer.setState({ status: 'error', error: String(e) }));
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

function toEntry(track: Track, i: number, source: QueueSource | null): QueueEntry {
  return { entryId: `${track.videoId}#${i}`, track, tier: 'context', source };
}

type RefLike = { id: number; title: string; artist: string; album: string; duration_sec: number; art_path: string | null; path: string };

/** Freshest position, fed by engine events at 10 Hz and interpolated in getPlayhead. */
const playhead = { sec: 0, sampledAt: 0, paused: true };
const endedListeners = new Set<() => void>();
export function onTrackEnded(listener: () => void): () => void {
  endedListeners.add(listener);
  return () => endedListeners.delete(listener);
}

/** The active audio clock equivalent: engine position, interpolated between ticks. */
export function getPlayhead(): { sec: number; paused: boolean } | null {
  if (playhead.sampledAt === 0) return null;
  const drift = playhead.paused ? 0 : (performance.now() - playhead.sampledAt) / 1000;
  return { sec: playhead.sec + drift, paused: playhead.paused };
}

/** No automix on desktop yet — the seek bar's beat pulse stays off. */
export function getMixBeat(): { phase: number | null } | null {
  return null;
}

/** No HTMLAudioElement on this backend; consumers must use getPlayhead(). */
export function getAudio(): HTMLAudioElement | null {
  return null;
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
    if (isRemote(tracks)) return playRemoteCommand(tracks, startIndex, 'load');
    const ids = idsOf(tracks);
    if (!ids.length) return;
    set({ status: 'loading' });
    invoke('player_load', { ids, start: startIndex, play: true }).catch((e) =>
      set({ status: 'error', error: String(e) }),
    );
  },

  playTrack: (track, source) => {
    if (!track.videoId.startsWith('local:')) return playRemoteCommand([track], 0, 'load');
    get().playTracks([track], 0, source);
  },

  playNext: (tracks, _source) => {
    if (isRemote(tracks)) void playRemoteCommand(tracks, 0, 'insert-next');
    else invoke('queue_insert', { ids: idsOf(tracks), at: get().index + 1 }).catch(() => {});
  },

  addToQueue: (tracks, _source) => {
    if (isRemote(tracks)) void playRemoteCommand(tracks, 0, 'append');
    else invoke('queue_insert', { ids: idsOf(tracks), at: null }).catch(() => {});
  },

  jumpTo: (i) => invoke('queue_jump', { index: i }).catch(() => {}),
  removeAt: (i) => invoke('queue_remove', { index: i }).catch(() => {}),
  move: (from, to) => invoke('queue_move', { from, to }).catch(() => {}),
  clearUpNext: () => invoke('queue_clear').catch(() => {}),
  next: () => invoke('player_next').catch(() => {}),
  previous: () => invoke('player_previous').catch(() => {}),
  togglePlay: () => invoke('player_toggle').catch(() => {}),
  seek: (sec) => {
    playhead.sec = sec;
    playhead.sampledAt = performance.now();
    invoke('player_seek', { sec }).catch(() => {});
  },

  toggleShuffle: () => updateSettings(set, get, { shuffle: !get().shuffle }),
  cycleRepeat: () => {
    const order = ['off', 'all', 'one'] as const;
    const repeat = order[(order.indexOf(get().repeat) + 1) % order.length];
    updateSettings(set, get, { repeat });
    invoke('player_repeat', { mode: repeat }).catch(() => {});
  },
  setAutoplay: (autoplay) => updateSettings(set, get, { autoplay }),
  setVolume: (n) => {
    const volume = Math.min(1, Math.max(0, n));
    updateSettings(set, get, { volume, muted: false });
    invoke('player_volume', { level: volume }).catch(() => {});
  },
  toggleMute: () => {
    const muted = !get().muted;
    updateSettings(set, get, { muted });
    invoke('player_volume', { level: muted ? 0 : get().volume }).catch(() => {});
  },
  setCrossfade: (crossfadeSec) => updateSettings(set, get, { crossfadeSec }),
  setAutomix: (automix) => updateSettings(set, get, { automix }),
  setAutomixBars: (automixBars) => updateSettings(set, get, { automixBars }),
  setShowNerdStats: (showNerdStats) => updateSettings(set, get, { showNerdStats }),

  refresh: async () => {
    const snap = await invoke<{ tracks: RefLike[]; index: number | null; repeat: 'off' | 'all' | 'one'; playing: boolean }>('queue_snapshot');
    const queue = snap.tracks.map((t, i) => toEntry(trackFromRef(t), i, null));
    const current = snap.index !== null ? snap.tracks[snap.index] : undefined;
    set({
      queue,
      index: snap.index ?? -1,
      nowPlaying: current
        ? {
            videoId: `local:${current.id}`,
            title: current.title,
            artist: current.artist || 'Unknown Artist',
            artworkUrl: current.art_path ? convertFileSrc(current.art_path, 'art') : null,
            durationSec: current.duration_sec,
          }
        : null,
      repeat: snap.repeat,
    });
  },
}));

function updateSettings(set: (p: Partial<PlayerState>) => void, get: () => PlayerState, patch: Partial<Settings>) {
  set(patch as Partial<PlayerState>);
  const s = get();
  saveSettings({
    shuffle: s.shuffle, repeat: s.repeat, autoplay: s.autoplay, volume: s.volume, muted: s.muted,
    crossfadeSec: s.crossfadeSec, automix: s.automix, automixBars: s.automixBars, showNerdStats: s.showNerdStats,
  });
}


let started = false;
/** Wire engine events to the store. Called once from main.tsx. */
export async function initPlayerStore(): Promise<void> {
  if (started) return;
  started = true;
  await listen<{ type: string; track?: RefLike | null; sec?: number; duration?: number; playing?: boolean; message?: string; snapshot?: { tracks: RefLike[]; index: number | null; repeat: 'off' | 'all' | 'one'; playing: boolean } }>(
    'player://event',
    (e) => {
      const ev = e.payload;
      switch (ev.type) {
        case 'track_changed': {
          const t = ev.track ?? null;
          usePlayer.setState((s) => ({
            nowPlaying: t
              ? {
                  videoId: `local:${t.id}`,
                  title: t.title,
                  artist: t.artist || 'Unknown Artist',
                  artworkUrl: t.art_path ? convertFileSrc(t.art_path, 'art') : null,
                  durationSec: t.duration_sec,
                }
              : null,
            status: t ? 'playing' : 'idle',
            error: null,
            index: t ? s.queue.findIndex((en) => en.track.videoId === `local:${t.id}`) : s.index,
          }));
          break;
        }
        case 'position':
          playhead.sec = ev.sec ?? 0;
          playhead.paused = !(ev.playing ?? false);
          playhead.sampledAt = performance.now();
          if (ev.playing !== undefined) {
            usePlayer.setState((s) => {
              if (s.status === 'error') return {};
              if (ev.playing && (s.status === 'paused' || s.status === 'loading')) return { status: 'playing', deck: s.deck };
              if (!ev.playing && s.status === 'playing') return { status: 'paused' };
              return {};
            });
          }
          break;
        case 'queue_changed': {
          // Rust serializes the snapshot flattened into the event payload.
          const snap = ev as unknown as {
            tracks: RefLike[];
            index: number | null;
            repeat: 'off' | 'all' | 'one';
          };
          if (!Array.isArray(snap.tracks)) break;
          usePlayer.setState({ queue: snap.tracks.map((t, i) => toEntry(trackFromRef(t), i, null)), index: snap.index ?? -1 });
          break;
        }
        case 'volume':
          break;
        case 'finished':
          usePlayer.setState({ status: 'idle' });
          endedListeners.forEach((l) => l());
          break;
        case 'track_error':
          usePlayer.setState({ status: 'error', error: ev.message ?? 'Could not play this track' });
          break;
      }
    },
  );
  await usePlayer.getState().refresh();
  const { volume, muted, repeat } = usePlayer.getState();
  invoke('player_volume', { level: muted ? 0 : volume }).catch(() => {});
  invoke('player_repeat', { mode: repeat }).catch(() => {});
}
