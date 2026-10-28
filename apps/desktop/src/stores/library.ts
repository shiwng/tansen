import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { backend } from "../backend";
import type { Folder, ScanProgress, Track } from "../types";

interface LibraryState {
  tracks: Track[];
  folders: Folder[];
  loading: boolean;
  scanning: boolean;
  scanProgress: ScanProgress | null;
  lastScan: { added: number; errors: number } | null;
  error: string | null;

  init: () => Promise<void>;
  refresh: () => Promise<void>;
  addFolder: (path: string) => Promise<void>;
  removeFolder: (id: number) => Promise<void>;
  rescan: () => Promise<void>;
}

export const useLibrary = create<LibraryState>((set, get) => ({
  tracks: [],
  folders: [],
  loading: true,
  scanning: false,
  scanProgress: null,
  lastScan: null,
  error: null,

  init: async () => {
    await listen<ScanProgress>("scan://progress", (e) =>
      set({ scanProgress: e.payload, scanning: true }),
    );
    await get().refresh();
    set({ loading: false });
  },

  refresh: async () => {
    try {
      const [tracks, folders] = await Promise.all([backend.tracksGet(), backend.foldersGet()]);
      set({ tracks, folders, error: null });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  addFolder: async (path) => {
    set({ scanning: true, scanProgress: null, error: null });
    try {
      await backend.folderAdd(path);
      const summary = await backend.scanStart();
      await get().refresh();
      set({ lastScan: { added: summary.added, errors: summary.errors } });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ scanning: false, scanProgress: null });
    }
  },

  removeFolder: async (id) => {
    try {
      await backend.folderRemove(id);
      await get().refresh();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  rescan: async () => {
    set({ scanning: true, scanProgress: null, error: null });
    try {
      const summary = await backend.scanStart();
      await get().refresh();
      set({ lastScan: { added: summary.added, errors: summary.errors } });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ scanning: false, scanProgress: null });
    }
  },
}));
