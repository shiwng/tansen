/**
 * Spotify playlist import, client side: URL parsing, dedupe, YTM matching.
 * The playlist itself is fetched by the Rust `spotify_playlist_fetch`
 * command (public embed surface, no auth); matching runs against the
 * existing `yt_search` command. Everything here is pure except `mapPool`.
 */

/** One row of a fetched Spotify playlist (Rust `SpotifyImportTrack`). */
export interface SpotifyImportTrack {
  id: string;
  title: string;
  artist: string;
  duration_sec: number;
  playable: boolean;
}

/** One stored-playlist row for `playlist_add_remote_tracks`. */
export interface RemotePlaylistRow {
  key_source: 'youtube' | 'spotify';
  key_id: string;
  title: string;
  artist: string;
  album: string | null;
  duration_sec: number;
  artwork_url: string | null;
}

/** A search row to match against (shape of `yt_search`'s CatalogRow). */
export interface CatalogRow {
  video_id: string;
  title: string;
  artist: string;
  duration_sec: number | null;
  thumbnail_url: string | null;
}

/**
 * Accepts the playlist URL forms Spotify shares (web URL, intl. web URL,
 * `spotify:playlist:` URI) or the bare 22-character id.
 */
export function parseSpotifyPlaylistId(input: string): string | null {
  const s = input.trim();
  let m = /^https?:\/\/(?:open\.)?spotify\.com\/(?:intl-[a-z]{2}\/)?playlist\/([A-Za-z0-9]{22})/.exec(s);
  if (m) return m[1];
  m = /^spotify:playlist:([A-Za-z0-9]{22})$/.exec(s);
  if (m) return m[1];
  return /^[A-Za-z0-9]{22}$/.test(s) ? s : null;
}

/** Drops repeats (same Spotify id keeps the first occurrence); counts them. */
export function dedupeTracks(tracks: SpotifyImportTrack[]): { rows: SpotifyImportTrack[]; duplicates: number } {
  const seen = new Set<string>();
  const rows: SpotifyImportTrack[] = [];
  for (const t of tracks) {
    if (!seen.has(t.id)) {
      seen.add(t.id);
      rows.push(t);
    }
  }
  return { rows, duplicates: tracks.length - rows.length };
}

/**
 * Picks the YouTube Music match for one Spotify row, in upstream
 * TrackMatcher's spirit: the candidate must look like the recording. A row
 * within ±5 s of the Spotify duration wins; among equals (and when no
 * duration lines up) a candidate whose artist shares a word with the
 * Spotify artist beats one that doesn't. Names decide order; on rare
 * same-name covers this can still pick wrong. The upgrade path is an
 * ISRC/market-aware matcher.
 */
export function pickMatch(rows: CatalogRow[], spotifyDurationSec: number | null, spotifyArtist?: string): CatalogRow | null {
  const first = rows[0];
  if (!first) return null;
  const close = (r: CatalogRow) =>
    spotifyDurationSec !== null && spotifyDurationSec > 0 && r.duration_sec !== null && Math.abs(r.duration_sec - spotifyDurationSec) <= 5;
  const artistOk = (r: CatalogRow) => artistOverlaps(r.artist, spotifyArtist ?? '');
  return rows.find((r) => close(r) && artistOk(r)) ?? rows.find((r) => close(r)) ?? rows.find(artistOk) ?? first;
}

/** True when any word (>2 chars) of the Spotify artist appears in the candidate's. */
function artistOverlaps(candidate: string, spotifyArtist: string): boolean {
  const words = (s: string) => s.toLowerCase().split(/[\s,&/]+/).filter((w) => w.length > 2);
  const want = words(spotifyArtist);
  if (want.length === 0) return false;
  return words(candidate).some((w) => want.includes(w));
}

/** Thrown when the user cancels an in-flight import; nothing was saved. */
export class ImportCancelled extends Error {
  constructor() {
    super('import cancelled');
  }
}

/**
 * Maps Spotify rows to stored-playlist rows, matching each against the
 * catalogue. Matched rows keep the Spotify display metadata but play through
 * the YouTube Music video id; unmatched rows stay as `spotify` rows the UI
 * shows as not-found. A failed search is a partial failure, not a fatal
 * one: the row imports unmatched and lands in `errors`. Runs `limit`
 * searches concurrently, reports progress after each one, and aborts
 * (throwing `ImportCancelled`, nothing saved) when `isCancelled` turns true.
 */
export async function buildImportRows(
  tracks: SpotifyImportTrack[],
  search: (query: string) => Promise<CatalogRow[]>,
  limit: number,
  onProgress: (done: number, total: number) => void,
  isCancelled: () => boolean = () => false,
): Promise<{ rows: RemotePlaylistRow[]; matched: number }> {
  // One query per distinct "title artist" pair: repeats share the search.
  const results = await mapPool(tracks, limit, async (t, i) => {
    if (isCancelled()) throw new ImportCancelled();
    let rows: CatalogRow[] = [];
    try {
      rows = await search(`${t.title} ${t.artist}`.trim());
    } catch {
      // Partial failure: this row just won't match today.
      rows = [];
    }
    onProgress(i + 1, tracks.length);
    if (isCancelled()) throw new ImportCancelled();
    const match = t.playable && rows.length > 0 ? pickMatch(rows, t.duration_sec > 0 ? t.duration_sec : null, t.artist) : null;
    const row: RemotePlaylistRow = match
      ? {
          key_source: 'youtube',
          key_id: match.video_id,
          title: t.title,
          artist: t.artist,
          album: null,
          duration_sec: t.duration_sec,
          artwork_url: match.thumbnail_url,
        }
      : {
          key_source: 'spotify',
          key_id: t.id,
          title: t.title,
          artist: t.artist,
          album: null,
          duration_sec: t.duration_sec,
          artwork_url: null,
        };
    return { row, matched: match !== null };
  });
  return {
    rows: results.map((r) => r.row),
    matched: results.filter((r) => r.matched).length,
  };
}

/** Runs `fn` over `items` with at most `limit` in flight; order preserved. */
export async function mapPool<T, R>(items: T[], limit: number, fn: (item: T, index: number) => Promise<R>): Promise<R[]> {
  const out = new Array<R>(items.length);
  let next = 0;
  const workers = Array.from({ length: Math.max(1, Math.min(limit, items.length)) }, async () => {
    while (next < items.length) {
      const i = next++;
      out[i] = await fn(items[i], i);
    }
  });
  await Promise.all(workers);
  return out;
}
