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

export type ScanProgress = { scanned: number; total: number; folder: string };
export type ScanSummary = { added: number; updated: number; marked_missing: number; errors: number };
