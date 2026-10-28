export interface Track {
  id: number;
  folder_id: number;
  path: string;
  file_name: string;
  title: string;
  artist: string;
  album: string;
  album_artist: string;
  track_no: number | null;
  disc_no: number | null;
  year: number | null;
  genre: string;
  duration_sec: number;
  codec: string;
  bitrate_kbps: number | null;
  sample_rate: number | null;
  bit_depth: number | null;
  channels: number | null;
  art_path: string | null;
  missing: boolean;
}

export interface TrackRef {
  id: number;
  title: string;
  artist: string;
  album: string;
  duration_sec: number;
  art_path: string | null;
  path: string;
}

export interface Folder {
  id: number;
  path: string;
}

export interface QueueSnapshot {
  tracks: TrackRef[];
  index: number | null;
  repeat: "off" | "all" | "one";
  playing: boolean;
  at_end: boolean;
}

export type ScanProgress = { scanned: number; total: number; folder: string };
export type ScanSummary = {
  added: number;
  updated: number;
  marked_missing: number;
  errors: number;
};

export type EngineEvent =
  | { type: "track_changed"; track: TrackRef | null }
  | { type: "position"; sec: number; duration: number; playing: boolean }
  | ({ type: "queue_changed" } & QueueSnapshot)
  | { type: "volume"; level: number }
  | { type: "finished" }
  | { type: "track_error"; track: TrackRef; message: string };

export function fmtTime(sec: number): string {
  if (!Number.isFinite(sec) || sec < 0) sec = 0;
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}
