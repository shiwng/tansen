import { create } from 'zustand';

/**
 * Appearance preferences (Settings → Appearance), persisted to
 * localStorage so they survive restarts. The store owns the three visual
 * data attributes on the root element; the theme attribute itself stays
 * chrome.ts's (single writer), which re-reads `theme` from here on change.
 */

export type ThemeMode = 'system' | 'light' | 'dark';

interface AppearanceState {
  theme: ThemeMode;
  /** Liquid Glass: the frosted toolbar/panel/pill material. */
  glass: boolean;
  /** Reduce Dynamic Blur: the large decorative backdrop blurs. */
  reduceBlur: boolean;
  /** Reduce Animation: force the reduced-motion kill switch. */
  reduceMotion: boolean;
  /** Full Screen Cover Art: whether the full-screen player can open. */
  fullScreenArt: boolean;
  /** Mesh gradient backdrop behind the full-screen player. */
  meshBackdrop: boolean;
  setTheme: (theme: ThemeMode) => void;
  setGlass: (on: boolean) => void;
  setReduceBlur: (on: boolean) => void;
  setReduceMotion: (on: boolean) => void;
  setFullScreenArt: (on: boolean) => void;
  setMeshBackdrop: (on: boolean) => void;
}

const KEY = 'bitchord:appearance';

interface Stored {
  theme: ThemeMode;
  glass: boolean;
  reduceBlur: boolean;
  reduceMotion: boolean;
  fullScreenArt: boolean;
  meshBackdrop: boolean;
}

const DEFAULTS: Stored = {
  theme: 'system',
  glass: true,
  reduceBlur: false,
  reduceMotion: false,
  fullScreenArt: true,
  meshBackdrop: true,
};

const THEMES: readonly string[] = ['system', 'light', 'dark'];

/** localStorage is user-editable, so the read is sanitized against defaults. */
function read(): Stored {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return { ...DEFAULTS };
    const parsed: unknown = JSON.parse(raw);
    const obj = (parsed && typeof parsed === 'object' ? parsed : {}) as Record<string, unknown>;
    return {
      theme: THEMES.includes(obj.theme as string) ? (obj.theme as ThemeMode) : DEFAULTS.theme,
      glass: typeof obj.glass === 'boolean' ? obj.glass : DEFAULTS.glass,
      reduceBlur: typeof obj.reduceBlur === 'boolean' ? obj.reduceBlur : DEFAULTS.reduceBlur,
      reduceMotion: typeof obj.reduceMotion === 'boolean' ? obj.reduceMotion : DEFAULTS.reduceMotion,
      fullScreenArt: typeof obj.fullScreenArt === 'boolean' ? obj.fullScreenArt : DEFAULTS.fullScreenArt,
      meshBackdrop: typeof obj.meshBackdrop === 'boolean' ? obj.meshBackdrop : DEFAULTS.meshBackdrop,
    };
  } catch {
    return { ...DEFAULTS };
  }
}

function persist(state: Stored): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(state));
  } catch {
    // quota/private mode: the setting lives for this session only
  }
  // chrome.ts owns data-theme and re-reads on this event.
  window.dispatchEvent(new Event('tansen:appearance'));
}

/** The root attributes this store owns. `theme` is chrome.ts's. */
function applyVisual(state: Stored): void {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  root.dataset.glass = state.glass ? 'on' : 'off';
  root.dataset.reduceBlur = state.reduceBlur ? 'on' : 'off';
  root.dataset.reduceMotion = state.reduceMotion ? 'on' : 'off';
}

const initial = read();

export const useAppearance = create<AppearanceState>()((set, get) => ({
  ...initial,
  setTheme: (theme) => {
    const next = { ...get(), theme };
    set(next);
    persist(next);
  },
  setGlass: (glass) => {
    const next = { ...get(), glass };
    set(next);
    applyVisual(next);
    persist(next);
  },
  setReduceBlur: (reduceBlur) => {
    const next = { ...get(), reduceBlur };
    set(next);
    applyVisual(next);
    persist(next);
  },
  setReduceMotion: (reduceMotion) => {
    const next = { ...get(), reduceMotion };
    set(next);
    applyVisual(next);
    persist(next);
  },
  setFullScreenArt: (fullScreenArt) => {
    const next = { ...get(), fullScreenArt };
    set(next);
    persist(next);
  },
  setMeshBackdrop: (meshBackdrop) => {
    const next = { ...get(), meshBackdrop };
    set(next);
    persist(next);
  },
}));

applyVisual(initial);

/** The full stored appearance, for consumers outside React (chrome.ts). */
export function readAppearance(): Stored {
  return read();
}
