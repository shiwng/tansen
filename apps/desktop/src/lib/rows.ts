import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import type { Track } from '@/data/music/types';
import { trackFromRef } from '@/state/player';
import type { QueueSource } from '@/playback/queue/timeline';

/** A library row as the Rust commands return it (serde camel/snake mix). */
export interface TrackRow {
  id: number;
  title: string;
  artist: string;
  album: string;
  album_artist: string;
  track_no: number | null;
  disc_no: number | null;
  year: number | null;
  genre: string;
  duration_sec: number;
  codec: string;
  bitrate_kbps: number | null;
  sample_rate: number | null;
  bit_depth: number | null;
  channels: number | null;
  art_path: string | null;
  missing: boolean;
}

export interface AlbumRow {
  key: string;
  album: string;
  album_artist: string;
  year: number | null;
  art_path: string | null;
  track_count: number;
  duration_sec: number;
}

export interface ArtistRow {
  name: string;
  track_count: number;
  album_count: number;
  art_path: string | null;
}

export function rowToTrack(row: TrackRow): Track {
  return trackFromRef({
    id: row.id,
    title: row.title,
    artist: row.artist,
    album: row.album,
    duration_sec: row.duration_sec,
    art_path: row.art_path,
  });
}

export function rowsToTracks(rows: TrackRow[]): Track[] {
  return rows.map(rowToTrack);
}

export const q = {
  tracks: () => ({ queryKey: ['local', 'tracks'], queryFn: () => invoke<TrackRow[]>('tracks_get') }),
  albums: () => ({ queryKey: ['local', 'albums'], queryFn: () => invoke<AlbumRow[]>('albums_get') }),
  artists: () => ({ queryKey: ['local', 'artists'], queryFn: () => invoke<ArtistRow[]>('artists_get') }),
  albumTracks: (key: string) => ({
    queryKey: ['local', 'album', key],
    queryFn: () => invoke<TrackRow[]>('album_tracks_get', { key }),
  }),
  artistTracks: (name: string) => ({
    queryKey: ['local', 'artist', name],
    queryFn: () => invoke<TrackRow[]>('artist_tracks_get', { name }),
  }),
  history: (limit: number) => ({
    queryKey: ['local', 'history', limit],
    queryFn: () => invoke<{ track: TrackRef; played_at: number }[]>('history_get', { limit }),
  }),
};

interface TrackRef {
  id: number;
  title: string;
  artist: string;
  album: string;
  duration_sec: number;
  art_path: string | null;
  path: string;
}

/** The source context a list was opened from, for the queue header. */
export function sourceFor(type: QueueSource['type'], title: string, id?: string | null): QueueSource {
  return { type, title, id: id ?? undefined };
}

export function artUrl(path: string | null | undefined): string | null {
  return path ? convertFileSrc(path, 'art') : null;
}
