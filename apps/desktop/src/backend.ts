import { invoke } from "@tauri-apps/api/core";
import type { Folder, QueueSnapshot, ScanSummary, Track, TrackRef } from "./types";

// One place where the frontend names the Rust command surface.

export const backend = {
  settingsGet: () => invoke<[string, string][]>("settings_get"),
  settingsSet: (key: string, value: string) => invoke<void>("settings_set", { key, value }),

  folderAdd: (path: string) => invoke<number>("folder_add", { path }),
  folderRemove: (id: number) => invoke<void>("folder_remove", { id }),
  foldersGet: () => invoke<Folder[]>("folders_get"),
  tracksGet: () => invoke<Track[]>("tracks_get"),
  scanStart: () => invoke<ScanSummary>("scan_start"),

  playerLoad: (ids: number[], start: number, play = true) =>
    invoke<void>("player_load", { ids, start, play }),
  playerToggle: () => invoke<void>("player_toggle"),
  playerNext: () => invoke<void>("player_next"),
  playerPrevious: () => invoke<void>("player_previous"),
  playerSeek: (sec: number) => invoke<void>("player_seek", { sec }),
  playerVolume: (level: number) => invoke<void>("player_volume", { level }),
  playerRepeat: (mode: "off" | "all" | "one") => invoke<void>("player_repeat", { mode }),

  queueSnapshot: () => invoke<QueueSnapshot>("queue_snapshot"),
  queueInsert: (ids: number[], at?: number) => invoke<void>("queue_insert", { ids, at }),
  queueRemove: (index: number) => invoke<void>("queue_remove", { index }),
  queueJump: (index: number) => invoke<void>("queue_jump", { index }),
  queueClear: () => invoke<void>("queue_clear"),

  playlistCreate: (name: string) => invoke<number>("playlist_create", { name }),
  playlistsGet: () => invoke<[number, string][]>("playlists_get"),
  playlistTracksGet: (id: number) => invoke<TrackRef[]>("playlist_tracks_get", { id }),
  playlistTracksSet: (id: number, ids: number[]) => invoke<void>("playlist_tracks_set", { id, ids }),
};
