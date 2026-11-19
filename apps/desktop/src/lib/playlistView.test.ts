// The All Playlists view pipeline: filter → search → sort over real rows.
import { describe, expect, it } from 'vitest';
import { applyPlaylistView } from './playlistView';
import type { PlaylistRow } from '@/types';

const rows: PlaylistRow[] = [
  { id: 1, name: 'Road Trip', favourite: false, created_at: 100, track_count: 12 },
  { id: 2, name: 'alex\'s mix', favourite: true, created_at: 300, track_count: 3 },
  { id: 3, name: 'Chill', favourite: false, created_at: 200, track_count: 0 },
  { id: 4, name: 'ALex\'s Mix II', favourite: true, created_at: 50, track_count: 7 },
];

const view = (over: Partial<Parameters<typeof applyPlaylistView>[1]> = {}) =>
  applyPlaylistView(rows, { filter: 'all', sort: '', search: '', ...over });

describe('applyPlaylistView', () => {
  it('filters to favourites only', () => {
    expect(view({ filter: 'favourites' }).map((p) => p.id)).toEqual([2, 4]);
  });

  it('searches names case-insensitively', () => {
    expect(view({ search: 'alex' }).map((p) => p.id)).toEqual([2, 4]);
    expect(view({ search: 'road' }).map((p) => p.id)).toEqual([1]);
    expect(view({ search: 'zzz' })).toEqual([]);
  });

  it('sorts by name locale-aware and by date added newest-first', () => {
    expect(view({ sort: 'name' }).map((p) => p.name)).toEqual(['alex\'s mix', 'ALex\'s Mix II', 'Chill', 'Road Trip']);
    expect(view({ sort: 'added' }).map((p) => p.id)).toEqual([2, 3, 1, 4]);
  });

  it('composes filter, search and sort', () => {
    const out = view({ filter: 'favourites', search: 'mix', sort: 'name' });
    expect(out.map((p) => p.id)).toEqual([2, 4]);
  });

  it('leaves library order untouched by default and never mutates the input', () => {
    expect(view().map((p) => p.id)).toEqual([1, 2, 3, 4]);
    expect(rows.map((p) => p.id)).toEqual([1, 2, 3, 4]);
  });
});
