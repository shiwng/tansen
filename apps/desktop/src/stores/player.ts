import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { backend } from "../backend";
import type { EngineEvent, TrackRef } from "../types";

interface PlayerState {
  ready: boolean;
  current: TrackRef | null;
  position: number;
  duration: number;
  playing: boolean;
  queue: TrackRef[];
  queueIndex: number | null;
  repeat: "off" | "all" | "one";
  volume: number;
  error: string | null;
  queueOpen: boolean;

  init: () => Promise<void>;
  toggle: () => void;
  next: () => void;
  previous: () => void;
  seek: (sec: number) => void;
  setVolume: (v: number) => void;
  cycleRepeat: () => void;
  playTracks: (ids: number[], start: number) => void;
  addToQueue: (ids: number[]) => void;
  removeFromQueue: (index: number) => void;
  jumpTo: (index: number) => void;
  clearQueue: () => void;
  setQueueOpen: (open: boolean) => void;
  setError: (e: string | null) => void;
}

function applyEvent(s: PlayerState, ev: EngineEvent): Partial<PlayerState> {
  switch (ev.type) {
    case "track_changed":
      return {
        current: ev.track,
        position: 0,
        duration: ev.track?.duration_sec ?? s.duration,
        error: null,
      };
    case "position":
      return { position: ev.sec, duration: ev.duration || s.duration, playing: ev.playing };
    case "queue_changed":
      return {
        queue: ev.tracks,
        queueIndex: ev.index,
        repeat: ev.repeat,
        playing: ev.playing,
      };
    case "volume":
      return { volume: ev.level };
    case "finished":
      return { playing: false };
    case "track_error":
      return { error: `Could not play “${ev.track.title}”: ${ev.message}` };
  }
}

export const usePlayer = create<PlayerState>((set, get) => ({
  ready: false,
  current: null,
  position: 0,
  duration: 0,
  playing: false,
  queue: [],
  queueIndex: null,
  repeat: "off",
  volume: 1,
  error: null,
  queueOpen: false,

  init: async () => {
    await listen<EngineEvent>("player://event", (e) => {
      set((s) => applyEvent(s, e.payload));
    });
    const snap = await backend.queueSnapshot();
    set({
      queue: snap.tracks,
      queueIndex: snap.index,
      repeat: snap.repeat,
      playing: snap.playing,
      ready: true,
    });
  },

  toggle: () => backend.playerToggle(),
  next: () => backend.playerNext(),
  previous: () => backend.playerPrevious(),
  seek: (sec) => {
    set({ position: sec });
    backend.playerSeek(sec);
  },
  setVolume: (v) => {
    set({ volume: v });
    backend.playerVolume(v);
  },
  cycleRepeat: () => {
    const order = ["off", "all", "one"] as const;
    const next = order[(order.indexOf(get().repeat) + 1) % order.length];
    backend.playerRepeat(next);
  },
  playTracks: (ids, start) => backend.playerLoad(ids, start, true),
  addToQueue: (ids) => backend.queueInsert(ids),
  removeFromQueue: (index) => backend.queueRemove(index),
  jumpTo: (index) => backend.queueJump(index),
  clearQueue: () => backend.queueClear(),
  setQueueOpen: (open) => set({ queueOpen: open }),
  setError: (e) => set({ error: e }),
}));
