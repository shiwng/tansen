import type { PlaylistRow } from '@/types';

export interface PlaylistViewOptions {
  filter: 'all' | 'favourites';
  /** '' = library order (as returned); 'name' = A→Z; 'added' = newest first. */
  sort: '' | 'name' | 'added';
  /** Case-insensitive substring match on the playlist name (already trimmed). */
  search: string;
}

/**
 * The All Playlists page's view pipeline over real rows: filter → search →
 * sort. "Recently Played" is not an option because the model carries no
 * per-playlist play data.
 */
export function applyPlaylistView(rows: PlaylistRow[], options: PlaylistViewOptions): PlaylistRow[] {
  let out = rows;
  if (options.filter === 'favourites') out = out.filter((p) => p.favourite);
  if (options.search) out = out.filter((p) => p.name.toLowerCase().includes(options.search));
  if (options.sort === 'name') out = [...out].sort((a, b) => a.name.localeCompare(b.name));
  else if (options.sort === 'added') out = [...out].sort((a, b) => b.created_at - a.created_at);
  return out;
}
