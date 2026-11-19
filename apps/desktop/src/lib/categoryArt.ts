import type { DiscoveryCard } from './discovery';

/**
 * Category artwork: the first real artist portrait returned for the category's
 * catalogue search. Selection is deterministic (no random pick, no shared
 * generic image), and the chosen URL is cached for seven days so the grid looks
 * the same across refreshes and relaunches.
 */

// v4: tiles moved from playlist mosaics to one curated artist portrait each.
const STORE_KEY = 'tansen.browse.category-art.v4';
const TTL_MS = 7 * 24 * 60 * 60 * 1000;

interface CachedArt {
  url: string;
  at: number;
}

type Store = Record<string, CachedArt>;

function readStore(): Store {
  try {
    const raw = localStorage.getItem(STORE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    return parsed && typeof parsed === 'object' ? (parsed as Store) : {};
  } catch {
    return {};
  }
}

function writeStore(store: Store): void {
  try {
    localStorage.setItem(STORE_KEY, JSON.stringify(store));
  } catch {
    // Storage full or disabled: the live React Query cache still holds the art.
  }
}

/** The cached URL for a category, or null when absent or older than the TTL. */
export function readCachedCategoryArt(id: string, now = Date.now()): string | null {
  const entry = readStore()[id];
  if (!entry || typeof entry.url !== 'string' || typeof entry.at !== 'number') return null;
  return now - entry.at > TTL_MS ? null : entry.url;
}

export function writeCachedCategoryArt(id: string, url: string, now = Date.now()): void {
  const store = readStore();
  store[id] = { url, at: now };
  writeStore(store);
}

/**
 * Pick the curated artist's portrait from an artist search. Exact name match
 * first (so "Adele" never resolves to a tribute act), then a title that
 * contains it, then the first result in catalogue rank order.
 */
export function pickCategoryArt(cards: readonly DiscoveryCard[], artist: string): string | null {
  const withArt = cards.filter((card) => card.kind === 'artist' && Boolean(card.thumbnail_url));
  const needle = artist.trim().toLowerCase();
  const exact = withArt.find((card) => card.title.trim().toLowerCase() === needle);
  const named = withArt.find((card) => card.title.trim().toLowerCase().includes(needle));
  return (exact ?? named ?? withArt[0])?.thumbnail_url ?? null;
}
