// @vitest-environment happy-dom
//
// Regression: the full-screen player rendered purely from `nowPlaying`,
// so restoring a persisted queue at boot mounted the fixed inset-0 overlay
// at launch with a close button that only cleared a flag nobody read — the
// whole app appeared frozen behind it. The gate now also requires the
// `fullScreen` UI flag.
import { describe, expect, it } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react';
import { MemoryRouter } from 'react-router';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { FullScreenPlayer } from './FullScreenPlayer';
import { usePlayer } from '@/state/player';
import { useUi } from '@/state/ui';
import { useAppearance } from '@/state/appearance';
import { useLyricsSettings } from '@/lyrics/settings';
import { LYRICS_SOURCES } from '@/lyrics/sources';

const nowPlaying = {
  videoId: 'v1',
  title: 'Song',
  artist: 'Artist',
  artworkUrl: null as string | null,
  durationSec: 180,
};

let root: Root | null = null;
let host: HTMLElement | null = null;

function reset(fullScreen: boolean, withTrack: boolean) {
  // happy-dom has no document.fonts; LyricsPanel's loader caps at 1.5 s.
  if (!('fonts' in document)) {
    (document as never as { fonts: { load: () => Promise<unknown[]> } }).fonts = { load: async () => [] };
  }
  usePlayer.setState({
    nowPlaying: withTrack ? (nowPlaying as never) : null,
    queue: [],
    index: -1,
    status: 'idle',
  });
  useUi.setState({ fullScreen });
  // Disable every lyrics provider so the panel never fetches in happy-dom.
  useLyricsSettings.setState({ disabled: LYRICS_SOURCES.map((source) => source.id) });
}

function mount() {
  if (root || !host) {
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
  }
  act(() => {
    root!.render(
      <QueryClientProvider client={new QueryClient()}>
        <MemoryRouter>
          <FullScreenPlayer />
        </MemoryRouter>
      </QueryClientProvider>,
    );
  });
  return host;
}

describe('FullScreenPlayer mount gate', () => {
  it.afterEach(() => {
    if (root) {
      act(() => root!.unmount());
      root = null;
      host?.remove();
      host = null;
    }
  });

  it('renders nothing when the UI flag is off, even with a restored track (the boot-freeze regression)', () => {
    reset(false, true);
    const host = mount();
    expect(host.childElementCount).toBe(0);
  });

  it('renders the overlay with a close button when the flag is on and a track is set', () => {
    reset(true, true);
    const host = mount();
    expect(host.querySelector('[role="dialog"][aria-label="Now Playing"]')).toBeTruthy();
    expect(host.querySelector('button[aria-label="Close"]')).toBeTruthy();
  });

  it('renders nothing with no track at all', () => {
    reset(true, false);
    const host = mount();
    expect(host.childElementCount).toBe(0);
  });

  it('opens even when full-screen cover art is disabled', () => {
    useAppearance.setState({ fullScreenArt: false });
    useUi.setState({ fullScreen: false });
    useUi.getState().setFullScreen(true);
    expect(useUi.getState().fullScreen).toBe(true);
    useAppearance.setState({ fullScreenArt: true });
  });

  it('closes on Escape', () => {
    reset(true, true);
    mount();
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(useUi.getState().fullScreen).toBe(false);
  });

  it('holds the overlay for the exit animation before unmounting', async () => {
    reset(true, true);
    const host = mount();
    act(() => useUi.getState().setFullScreen(false));
    expect(host.querySelector('[role="dialog"]')).toBeTruthy();
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 250));
    });
    expect(host.querySelector('[role="dialog"]')).toBeNull();
  });
});
