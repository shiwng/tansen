// @vitest-environment happy-dom
//
// The Songs section when the local library is empty: the centred library
// state is the whole page. The section header, scan actions and folder list
// stay out of the way; folder management lives in Settings → Library.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { LibraryPage } from './LibraryPage';
import type { TrackRow } from '@/lib/rows';

const navigateMock = vi.hoisted(() => vi.fn());
vi.mock('react-router', () => ({
  useParams: () => ({ section: 'songs' }),
  useNavigate: () => navigateMock,
}));
let trackRows: TrackRow[] = [];
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string) => (cmd === 'tracks_get' ? trackRows : [])),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(async () => null) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
// The list itself is covered by its own tests; here it just marks the normal
// Songs page.
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

describe('LibraryPage Songs section', () => {
  afterEach(() => {
    if (root) {
      act(() => root!.unmount());
      root = null;
      host?.remove();
      host = null;
    }
    navigateMock.mockClear();
    trackRows = [];
  });

  it('shows only the centred empty state when the library is empty', async () => {
    const host = mount();
    await settle(host, 'Add Music to Your Library');
    expect(host.textContent).toContain('Explore music and add your favourites to your Tansen library.');
    expect(host.textContent).toContain('Browse Tansen');
    // The page header, scan controls and folder list are replaced entirely.
    expect(host.querySelector('h1')).toBeNull();
    expect(host.textContent).not.toContain('Rescan');
    expect(host.textContent).not.toContain('Indexed folders');
  });

  it('sends Browse Tansen to Search', async () => {
    const host = mount();
    await settle(host, 'Browse Tansen');
    const browse = Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.includes('Browse Tansen'));
    act(() => browse!.click());
    expect(navigateMock).toHaveBeenCalledWith('/search');
  });

  it('restores the normal Songs page when the library has tracks', async () => {
    trackRows = [
      {
        id: 1,
        title: 'First Song',
        artist: 'Some Artist',
        album: 'First Album',
        album_artist: 'Some Artist',
        track_no: 1,
        disc_no: 1,
        year: 2026,
        genre: 'Pop',
        duration_sec: 180,
        codec: 'flac',
        bitrate_kbps: 1000,
        sample_rate: 44100,
        bit_depth: 16,
        channels: 2,
        art_path: null,
        missing: false,
      },
    ];
    const host = mount();
    await settle(host, 'First Song');
    expect(host.querySelector('h1')?.textContent).toBe('Songs');
    expect(host.textContent).toContain('Rescan');
    expect(host.textContent).toContain('Import folder');
    expect(host.textContent).not.toContain('Add Music to Your Library');
  });
});
