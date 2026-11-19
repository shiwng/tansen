import { invoke } from '@tauri-apps/api/core';
import type { MusicItem, Track } from '@/data/music/types';

/**
 * The sidecar's discovery rows, shared by Search, remote playlist/album/artist
 * pages and the Browse Categories shelves. One mapper keeps every surface's
 * playback identity and artwork handling identical.
 */

export type DiscoveryFilter = 'all' | 'songs' | 'videos' | 'albums' | 'artists' | 'playlists';

export interface DiscoveryTrack {
  key: { source: string; id: string };
  title: string;
  artist: string;
  album: string | null;
  duration_sec: number | null;
  thumbnail_url: string | null;
  artists: Array<{ name: string; browse_id: string | null }>;
  album_id: string | null;
  is_video: boolean;
  explicit: boolean;
  playable: boolean;
}

export interface DiscoveryCard {
  kind: string;
  browse_id: string;
  title: string;
  subtitle: string | null;
  thumbnail_url: string | null;
}

export interface DiscoverySearchPage {
  tracks: DiscoveryTrack[];
  cards: DiscoveryCard[];
  queries: string[];
  continuation: string | null;
}

/** A remote playlist, album or artist page (`discovery_album`, etc.). */
export interface DiscoveryBrowsePage {
  title: string | null;
  subtitle: string | null;
  thumbnail_url: string | null;
  tracks: DiscoveryTrack[];
  suggested: DiscoveryTrack[];
  continuation: string | null;
}

/** `discovery_search` returns the catalogue page and the local-library rows. */
export async function discoverySearch(
  query: string,
  filter: DiscoveryFilter,
  limit: number,
  continuation: string | null = null,
): Promise<DiscoverySearchPage> {
  const [page] = await invoke<[DiscoverySearchPage, unknown[]]>('discovery_search', {
    query,
    filter,
    continuation,
    limit,
  });
  return page;
}

export async function discoveryContinuation(token: string): Promise<DiscoveryBrowsePage> {
  return invoke<DiscoveryBrowsePage>('discovery_continuation', { token });
}

export function discoveryToTrack(row: DiscoveryTrack, index: number | null = null): Track {
  return {
    videoId: row.key.id,
    title: row.title,
    artists: row.artists.length
      ? row.artists.map((artist) => ({ name: artist.name, id: artist.browse_id }))
      : [{ name: row.artist || 'Unknown Artist', id: null }],
    album: row.album ? { id: row.album_id, name: row.album } : null,
    durationSec: row.duration_sec,
    explicit: row.explicit,
    thumbnail: row.thumbnail_url,
    index,
    plays: null,
    sourceKind: row.key.source,
    artworkUrl: row.thumbnail_url,
    ...(row.playable ? {} : { unavailable: 'missing' as const }),
  };
}

/** A browse card as a shelf item; null for kinds the app cannot open. */
export function cardToItem(card: DiscoveryCard): MusicItem | null {
  if (card.kind !== 'album' && card.kind !== 'artist' && card.kind !== 'playlist') return null;
  const fallback = card.kind === 'album' ? 'Album' : card.kind === 'artist' ? 'Artist' : 'Playlist';
  return {
    kind: card.kind,
    id: card.browse_id,
    title: card.title,
    subtitle: card.subtitle || fallback,
    artists: card.kind === 'artist' ? [{ name: card.title, id: card.browse_id }] : [],
    album: null,
    thumbnail: card.thumbnail_url,
    wide: false,
    durationSec: null,
    explicit: false,
  };
}
