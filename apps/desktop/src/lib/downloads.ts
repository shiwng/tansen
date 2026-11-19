/** Download queue helpers shared by track menus and album/playlist pages. */
import { invoke } from '@tauri-apps/api/core';
export interface DownloadRequest {
  kind: string;
  source: string;
  trackId: string;
  title: string;
  artist: string;
  album: string | null;
  durationSec: number;
  artworkUrl: string | null;
  quality: string;
}

export interface AddOutcome {
  added: number;
  duplicates: number;
  rejected: string[];
}

export function addDownloads(tracks: DownloadRequest[]): Promise<AddOutcome> {
  return invoke<AddOutcome>('downloads_add', { tracks });
}

/** Album/playlist rows: local rows carry a library id or file path; remote
 * rows carry a catalogue key. Unsupported providers are reported per row by
 * the backend. */
export interface DownloadableRow {
  id: number;
  title: string;
  artist: string;
  album: string;
  duration_sec: number;
  path?: string;
  video_id?: string | null;
  key_source?: string | null;
  artwork_url?: string | null;
}

export function requestsFromRefs(refs: DownloadableRow[], kind: string): DownloadRequest[] {
  return refs
    .map((t) => {
      const local = t.id > 0;
      const source = local ? 'local' : (t.key_source ?? 'youtube');
      const trackId = local ? (t.path ?? `local:${t.id}`) : (t.video_id ?? '');
      if (!trackId) return null;
      return {
        kind,
        source,
        trackId,
        title: t.title,
        artist: t.artist,
        album: t.album || null,
        durationSec: t.duration_sec,
        artworkUrl: local ? null : (t.artwork_url ?? null),
        quality: 'high',
      } satisfies DownloadRequest;
    })
    .filter((item): item is DownloadRequest => item !== null);
}

/** One honest, human-readable summary of what the backend accepted. */
export function describeOutcome(outcome: AddOutcome): string {
  const parts: string[] = [];
  if (outcome.added > 0) parts.push(`${outcome.added} queued`);
  if (outcome.duplicates > 0) parts.push(`${outcome.duplicates} already queued or downloaded`);
  if (outcome.rejected.length > 0) parts.push(`${outcome.rejected.length} not downloadable`);
  return parts.length ? `${parts.join(' · ')}.` : 'Nothing to download.';
}
