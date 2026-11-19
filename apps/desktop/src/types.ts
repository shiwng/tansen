/** Backend row types shared with the Rust commands. */

/** A queue/display reference to a library track (Rust `TrackRef`). */
export interface TrackRef {
  id: number;
  title: string;
  artist: string;
  album: string;
  duration_sec: number;
  art_path: string | null;
  path: string;
  /** Catalogue video id for remote rows (id is 0 for those). */
  video_id?: string | null;
  /** Source kind for remote playlist rows: `youtube` = matched/playable, `spotify` = imported metadata only. */
  key_source?: string | null;
  /** Original catalogue artwork URL for remote playlist rows (routed through `fetch_artwork`). */
  artwork_url?: string | null;
}

/** Queue snapshot as `queue_snapshot` returns it. */
export interface QueueSnapshot {
  tracks: TrackRef[];
  index: number | null;
  repeat: 'off' | 'all' | 'one';
  playing: boolean;
  at_end: boolean;
}

export interface Folder {
  id: number;
  path: string;
}

export type ScanProgress = { scanned: number; total: number; folder_id: number };
export type ScanSummary = { added: number; updated: number; marked_missing: number; errors: number };

/** One row of the All Playlists page (playlists_get). */
export interface PlaylistRow {
  id: number;
  name: string;
  favourite: boolean;
  /** Unix seconds; the "Date Added" sort. */
  created_at: number;
  /** Local + imported-remote rows, matching what the playlist page shows. */
  track_count: number;
}
