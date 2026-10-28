/**
 * Where the LCD's title/artist lead. Local-library editions: album pages are
 * keyed by album name, artist pages by the credited name. (The upstream web
 * player resolves through YouTube browse ids, which local files don't have.)
 */
import type { AlbumRef, ArtistRef } from '@/data/music/types';

export async function albumPageFor(_videoId: string, album: AlbumRef | null, fallbackQuery: string): Promise<string> {
  return album?.name
    ? `/album/${encodeURIComponent(album.name)}`
    : `/search?q=${encodeURIComponent(fallbackQuery)}`;
}

export async function artistPageFor(_videoId: string, credit: ArtistRef): Promise<string> {
  return `/artist/${encodeURIComponent(credit.name)}`;
}
