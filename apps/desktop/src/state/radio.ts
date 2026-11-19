/**
 * Radio preferences that outlive a session: favourite stations, recently
 * played stations and user-added stream URLs. This is local app data (the
 * same storage the player settings use) — nothing is sent anywhere, and the
 * Radio Browser directory itself only ever sees discovery requests.
 */
import { create } from 'zustand';
import type { RadioStation } from '@/lib/radio';

const KEY = 'tansen:radio';
const VERSION = 1;
const MAX_RECENT = 20;
const MAX_CUSTOM = 30;

/** A user-added stream, given a stable local id. */
export interface CustomStation {
  id: string;
  name: string;
  url: string;
}

interface Stored {
  v: number;
  favourites: RadioStation[];
  recent: RadioStation[];
  custom: CustomStation[];
}

interface RadioState extends Stored {
  toggleFavourite: (station: RadioStation) => void;
  isFavourite: (id: string) => boolean;
  addRecent: (station: RadioStation) => void;
  addCustom: (name: string, url: string) => void;
  removeCustom: (id: string) => void;
}

function load(): Stored {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? 'null') as Partial<Stored> | null;
    if (!raw || raw.v !== VERSION) return { v: VERSION, favourites: [], recent: [], custom: [] };
    return {
      v: VERSION,
      favourites: Array.isArray(raw.favourites) ? raw.favourites.filter((s) => s && s.id && s.url) : [],
      recent: Array.isArray(raw.recent) ? raw.recent.filter((s) => s && s.id && s.url) : [],
      custom: Array.isArray(raw.custom) ? raw.custom.filter((c) => c && c.id && c.url) : [],
    };
  } catch {
    return { v: VERSION, favourites: [], recent: [], custom: [] };
  }
}

function save(state: Stored) {
  try {
    localStorage.setItem(KEY, JSON.stringify({ v: VERSION, favourites: state.favourites, recent: state.recent, custom: state.custom }));
  } catch {
    // quota/private mode: favourites survive this session only
  }
}

export const useRadio = create<RadioState>((set, get) => ({
  ...load(),
  toggleFavourite: (station) => {
    const exists = get().favourites.some((s) => s.id === station.id);
    const favourites = exists ? get().favourites.filter((s) => s.id !== station.id) : [station, ...get().favourites];
    set({ favourites });
    save({ ...get(), favourites });
  },
  isFavourite: (id) => get().favourites.some((s) => s.id === id),
  addRecent: (station) => {
    const recent = [station, ...get().recent.filter((s) => s.id !== station.id)].slice(0, MAX_RECENT);
    set({ recent });
    save({ ...get(), recent });
  },
  addCustom: (name, url) => {
    const existing = get().custom.find((c) => c.url === url);
    const custom = [
      { id: existing?.id ?? `custom-${Date.now().toString(36)}`, name: name.trim() || url, url },
      ...get().custom.filter((c) => c.url !== url),
    ].slice(0, MAX_CUSTOM);
    set({ custom });
    save({ ...get(), custom });
  },
  removeCustom: (id) => {
    const custom = get().custom.filter((c) => c.id !== id);
    set({ custom });
    save({ ...get(), custom });
  },
}));

/** The radio Track the player engine plays: a live stream keyed by its URL. */
export function stationTrack(station: Pick<RadioStation, 'id' | 'name' | 'url' | 'country' | 'tags'>, artwork: string | null) {
  const detail = [station.country, ...station.tags.split(',').map((t) => t.trim()).filter(Boolean).slice(0, 2)]
    .filter(Boolean)
    .join(' • ');
  return {
    videoId: station.url,
    title: station.name,
    artists: [{ name: detail || 'Live Radio', id: null }],
    album: null,
    durationSec: null,
    explicit: false,
    thumbnail: artwork,
    index: null,
    plays: null,
    sourceKind: 'radio' as const,
  };
}
