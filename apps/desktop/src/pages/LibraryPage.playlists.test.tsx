// @vitest-environment happy-dom
//
// The All Playlists section: real rows render as tiles with their song count,
// the empty library shows the centred empty state (no fabricated playlists),
// and a filter/search miss explains itself.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { LibraryPage } from './LibraryPage';
import type { PlaylistRow } from '@/types';

let playlistRows: PlaylistRow[] = [
  { id: 1, name: 'Road Trip', favourite: false, created_at: 100, track_count: 12 },
  { id: 2, name: 'Faves', favourite: true, created_at: 200, track_count: 1 },
];
let pageParams = '';

const navigateMock = vi.hoisted(() => vi.fn());
vi.mock('react-router', () => ({
  useParams: () => ({ section: 'playlists' }),
  useNavigate: () => navigateMock,
  useSearchParams: () => [new URLSearchParams(pageParams), vi.fn()],
}));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === 'playlists_get') return playlistRows;
    return [];
  }),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(async () => null) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@/components/music/Artwork', () => ({
  Artwork: ({ src, alt }: { src: string | null; alt?: string }) => <img data-testid="artwork" src={src ?? ''} alt={alt ?? ''} />,
}));
let root: Root | null = null;
let host: HTMLElement | null = null;

function mount() {
  if (!('fonts' in document)) {
    (document as never as { fonts: { load: () => Promise<unknown[]> } }).fonts = { load: async () => [] };
  }
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

describe('LibraryPage All Playlists section', () => {
  afterEach(() => {
    if (root) {
      act(() => root!.unmount());
      root = null;
      host?.remove();
      host = null;
    }
    playlistRows = [
      { id: 1, name: 'Road Trip', favourite: false, created_at: 100, track_count: 12 },
      { id: 2, name: 'Faves', favourite: true, created_at: 200, track_count: 1 },
    ];
    pageParams = '';
    navigateMock.mockClear();
  });

  it('renders every playlist as a tile with its song count', async () => {
    const host = mount();
    await settle(host, 'Road Trip');
    expect(host.textContent).toContain('Road Trip');
    expect(host.textContent).toContain('Faves');
    expect(host.textContent).toContain('12 songs');
    expect(host.textContent).toContain('1 song');
    expect(host.querySelectorAll('[data-testid="artwork"]').length).toBe(2);
  });

  it('shows the centred empty state with the discovery action when no playlists exist', async () => {
    playlistRows = [];
    const host = mount();
    await settle(host, 'Add Music to Your Library');
    expect(host.textContent).toContain('Add Music to Your Library');
    const browse = Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.includes('Browse Tansen'));
    expect(browse).toBeTruthy();
    act(() => browse!.click());
    expect(navigateMock).toHaveBeenCalledWith('/search');
    // Creating a playlist stays available through the header.
    const create = Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.includes('New playlist'));
    expect(create).toBeTruthy();
    expect(host.querySelectorAll('[data-testid="artwork"]').length).toBe(0);
  });

  it('explains a favourites-filter miss instead of rendering a blank page', async () => {
    playlistRows = [{ id: 1, name: 'Road Trip', favourite: false, created_at: 100, track_count: 12 }];
    pageParams = 'filter=favourites';
    const host = mount();
    await settle(host, 'No favourites yet');
    expect(host.textContent).toContain('No favourites yet — open a playlist and choose Favourite.');
  });

  it('explains a search miss with the query', async () => {
    pageParams = 'q=zzz';
    const host = mount();
    await settle(host, 'No playlists matching');
    expect(host.textContent).toContain('No playlists matching “zzz”.');
  });
});
