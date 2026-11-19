import type { Track } from '@/data/music/types';

/** One search row as `search_tracks` returns it (unified search). */
export interface SearchTrackRow {
  key: { source: string; id: string };
  title: string;
  artist: string;
  album: string | null;
  duration_sec: number | null;
  artwork_url: string | null;
}

/** Per-source outcome: the status lives under `status.state` (serde tag). */
export type SourceOutcome = {
  source: string;
  status: { state: 'ok' | 'timeout' | 'error'; kind?: string; message?: string };
};

/** One contiguous per-source group, in backend rank order. */
export interface SourceGroup {
  source: string;
  rows: Track[];
  status: SourceOutcome['status'] | null;
}

/** Map a search row to the Track shape the UI plays from. */
export function rowToTrack(row: SearchTrackRow): Track {
  const isLocal = row.key.source === 'local';
  return {
    videoId: isLocal ? `local:${row.key.id}` : row.key.id,
    title: row.title,
    artists: [{ name: row.artist || 'Unknown Artist', id: null }],
    album: row.album ? { id: null, name: row.album } : null,
    durationSec: row.duration_sec && row.duration_sec > 0 ? row.duration_sec : null,
    explicit: false,
    thumbnail: row.artwork_url,
    index: null,
    plays: null,
    sourceKind: isLocal ? undefined : row.key.source,
    artworkUrl: row.artwork_url,
  };
}

/**
 * Groups rows into contiguous per-source sections, preserving the backend's
 * rank order (the rows arrive sorted by `key.source.rank()`).
 */
export function groupBySource(rows: SearchTrackRow[]): SourceGroup[] {
  const groups: SourceGroup[] = [];
  for (const row of rows) {
    const last = groups[groups.length - 1];
    if (last && last.source === row.key.source) {
      last.rows.push(rowToTrack(row));
    } else {
      groups.push({ source: row.key.source, rows: [rowToTrack(row)], status: null });
    }
  }
  return groups;
}

/** Human label for a source kind (existing naming: Settings uses the same words). */
export function sourceLabel(source: string): string {
  switch (source) {
    case 'addon':
      return 'Addon';
    case 'local':
      return 'Local Library';
    case 'subsonic':
      return 'Subsonic';
    case 'youtube':
      return 'YouTube Music';
    default:
      return source;
  }
}

/**
 * Attach each group's per-source status. A source with no rows still gets
 * its outcome (timeout/error), so partial failures are visible.
 */
export function attachStatuses(
  groups: SourceGroup[],
  statuses: SourceOutcome[],
): SourceGroup[] {
  const bySource = new Map(statuses.map((s) => [s.source, s.status]));
  const seen = new Set<string>();
  for (const g of groups) {
    g.status = bySource.get(g.source) ?? null;
    if (g.status) seen.add(g.source);
  }
  // Sources that answered but returned no rows: show the state, empty group.
  for (const s of statuses) {
    if (!seen.has(s.source) && s.status.state !== 'ok') {
      groups.push({ source: s.source, rows: [], status: s.status });
    }
  }
  return groups;
}

/** One-line partial-failure text for the non-ok sources. */
export function statusLine(
  statuses: SourceOutcome[],
): string | null {
  const bad = statuses.filter((s) => s.status.state !== 'ok');
  if (bad.length === 0) return null;
  const parts = bad.map(
    (s) =>
      `${sourceLabel(s.source)} ${s.status.state === 'timeout' ? 'timed out' : `failed${s.status.message ? `: ${s.status.message}` : ''}`}`,
  );
  return `${parts.join('; ')} — results below are from the rest.`;
}
