/**
 * M11 + S6: the queue body lives in the engine; the store keeps an ordered
 * mirror. When the engine publishes a new `queue_rev`, the mirror's
 * metadata is refreshed from the database via `queue_tracks_get`, which
 * returns display rows for BOTH kinds — local library tracks and remote
 * (source-keyed) tracks. This pure merge keeps the mirror's order and
 * leaves entries the DB does not know untouched.
 */

/** Raw `queue_tracks_get` row (QueueDisplayRow on the wire). */
export interface QueueRow {
  kind: 'local' | 'remote';
  /** Local: library track id; remote: `remote_tracks` row id — the EntryId. */
  id: number;
  title: string;
  artist: string;
  album: string | null;
  duration_sec: number;
  /** Local rows: art cache path (rendered via `art://`). */
  art_path: string | null;
  /** Remote rows: catalogue artwork URL (never fetched directly). */
  artwork_url: string | null;
  /** Remote rows: the source's own track id. */
  video_id: string | null;
  /** Remote rows: source kind (`youtube` / `addon`). */
  key_source: string | null;
}

export interface TrackLike {
  videoId: string;
  title: string;
  artists: Array<{ name: string; id: string | number | null }>;
  album: { id: string | number | null; name: string } | null;
  durationSec: number | null;
  thumbnail: string | null;
  /** Remote rows: the original catalogue artwork URL (S6). */
  artworkUrl?: string | null;
}

/** A mirror entry fed to the merge: the track plus its engine EntryId. */
export interface MirrorEntry<T extends TrackLike = TrackLike> {
  track: T;
  engineId: number;
}

/**
 * Applies DB rows onto the mirror: local rows match by `local:<engineId>`
 * (the library id), remote rows by `remote:<engineId>` (the remote_tracks
 * row id). `thumbnailFor` turns a row's artwork into the URL the UI uses
 * (injected so this module stays free of Tauri imports and testable).
 */
export function mergeTrackRows<T extends TrackLike>(
  items: MirrorEntry<T>[],
  rows: QueueRow[],
  thumbnailFor: (row: QueueRow) => string | null,
): T[] {
  const byId = new Map<string, QueueRow>(rows.map((r) => [`${r.kind}:${r.id}`, r]));
  return items.map(({ track, engineId }) => {
    const kind = track.videoId.startsWith('local:') ? 'local' : 'remote';
    const row = byId.get(`${kind}:${engineId}`);
    if (!row) return track;
    return {
      ...track,
      title: row.title,
      artists: [{ name: row.artist || 'Unknown Artist', id: null }],
      album: row.album ? { id: null, name: row.album } : null,
      durationSec: row.duration_sec > 0 ? row.duration_sec : null,
      thumbnail: thumbnailFor(row),
      artworkUrl: row.artwork_url ?? track.artworkUrl ?? null,
    };
  });
}
