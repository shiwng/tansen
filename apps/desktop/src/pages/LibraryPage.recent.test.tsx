// @vitest-environment happy-dom
//
// Recently Added is library additions, not playback history: it asks the
// backend for `recently_added_get` (never `history_get`), shows the real list
// when tracks exist, and the centred library state when there are none.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { LibraryPage } from './LibraryPage';
import type { TrackRow } from '@/lib/rows';

const navigateMock = vi.hoisted(() => vi.fn());
vi.mock('react-router', () => ({
  useParams: () => ({ section: 'recent' }),
  useNavigate: () => navigateMock,
  useSearchParams: () => [new URLSearchParams(), vi.fn()],
}));
let trackRows: TrackRow[] = [];
let calls: string[] = [];
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string) => {
    calls.push(cmd);
    return cmd === 'recently_added_get' ? trackRows : [];
  }),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(async () => null) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@/components/music/TrackList', () => ({
  TrackList: ({ tracks }: { tracks: { title: string }[] }) => (
    <div data-testid="track-list">{tracks.map((t) => t.title).join(', ')}</div>
  ),
}));

let root: Root | null = null;
let host: HTMLElement | null = null;

function mount() {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  act(() => {
    root!.render(
      <QueryClientProvider client={new QueryClient()}>
        <LibraryPage />
      </QueryClientProvider>,
    );
  });
  return host;
}

async function settle(host: HTMLElement, text: string) {
  for (let i = 0; i < 20 && !host.textContent?.includes(text); i++) {
    await act(async () => {
      await new Promise((r) => setTimeout(r, 10));
    });
  }
}

const row = (id: number, title: string): TrackRow => ({
  id,
  title,
  artist: 'Artist',
  album: 'Album',
  album_artist: 'Artist',
  track_no: id,
  disc_no: 1,
  year: 2026,
  genre: 'Pop',
  duration_sec: 180,
  codec: 'flac',
  bitrate_kbps: null,
  sample_rate: null,
  bit_depth: null,
  channels: null,
  art_path: null,
  missing: false,
});

describe('LibraryPage Recently Added section', () => {
  afterEach(() => {
    if (root) {
      act(() => root!.unmount());
      root = null;
      host?.remove();
      host = null;
    }
    navigateMock.mockClear();
    trackRows = [];
    calls = [];
  });

  it('reads additions, not playback history', async () => {
    mount();
    await settle(host!, 'Add Music to Your Library');
    expect(calls).toContain('recently_added_get');
    expect(calls).not.toContain('history_get');
  });

  it('shows the centred empty state when nothing has been added', async () => {
    const host = mount();
    await settle(host, 'Add Music to Your Library');
    expect(host.textContent).toContain('Explore music and add your favourites to your Tansen library.');
    expect(host.textContent).toContain('Browse Tansen');
    expect(host.querySelector('h1')).toBeNull();
  });

  it('shows the real list, titled Recently Added, when tracks exist', async () => {
    trackRows = [row(2, 'Newest'), row(1, 'Older')];
    const host = mount();
    await settle(host, 'Newest');
    expect(host.querySelector('h1')?.textContent).toBe('Recently Added');
    expect(host.textContent).not.toContain('Add Music to Your Library');
    expect(host.querySelector('[data-testid="track-list"]')?.textContent).toBe('Newest, Older');
  });
});
