// @vitest-environment happy-dom
//
// Regression: Lcd called useArtworkUrl after `if (!nowPlaying) return`, so the
// restored queue (null → playing) changed the hook count and React threw error
// #310 at launch. All hooks now run before the early return.
import { describe, expect, it, vi } from 'vitest';
import { createRoot } from 'react-dom/client';
import { act } from 'react';
import { MemoryRouter } from 'react-router';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => null),
  convertFileSrc: (path: string) => `art://localhost/${encodeURIComponent(path)}`,
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));

import { usePlayer } from '@/state/player';
import type { Track } from '@/data/music/types';
import { Lcd } from './Lcd';

const track: Track = {
  videoId: 'v1',
  title: 'Song',
  artists: [{ name: 'Artist', id: null }],
  album: null,
  durationSec: 200,
  explicit: false,
  thumbnail: null,
  index: null,
  plays: null,
};

describe('Lcd', () => {
  it('renders an idle pill, then a restored queue playing', () => {
    usePlayer.setState({ status: 'idle', nowPlaying: null, queue: [], index: 0, error: null, deck: 0, mixing: false });
    const host = document.createElement('div');
    document.body.appendChild(host);
    const root = createRoot(host);

    act(() => root.render(<MemoryRouter><Lcd /></MemoryRouter>));
    act(() =>
      usePlayer.setState({
        status: 'playing',
        nowPlaying: { videoId: 'v1', title: 'Song', artist: 'Artist', artworkUrl: null, durationSec: 200 },
        queue: [{ entryId: 'v1#0', track, tier: 'context', source: null, engineId: 1 }],
      }),
    );

    expect(host.textContent).toContain('Song');
    act(() => root.unmount());
    host.remove();
  });
});
