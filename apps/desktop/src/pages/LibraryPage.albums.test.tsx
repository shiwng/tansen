// @vitest-environment happy-dom
//
// The Albums section: the empty state is the shared discovery panel (no fake
// albums), the header keeps the REAL import action, and the populated grid
// renders every album row as a tile linking to its detail view.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { LibraryPage } from './LibraryPage';

// Two real-shaped album rows; swapped to [] for the empty-state test.
let albumRows: { key: string; album: string; album_artist: string | null; art_path: string | null }[] = [
  { key: 'Artist|First Album', album: 'First Album', album_artist: 'Some Artist', art_path: '/covers/a.jpg' },
  { key: 'Artist|Second Album', album: 'Second Album', album_artist: null, art_path: null },
];

const navigateMock = vi.hoisted(() => vi.fn());
vi.mock('react-router', () => ({
  useParams: () => ({ section: 'albums' }),
  useNavigate: () => navigateMock,
  useSearchParams: () => [new URLSearchParams(), vi.fn()],
}));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === 'albums_get') return albumRows;
    return []; // every other query renders its empty branch
  }),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(async () => null) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
// The grid tiles render artwork through this component; the URL plumbing is
// out of scope here.
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

describe('LibraryPage Albums section', () => {
  afterEach(() => {
    if (root) {
      act(() => root!.unmount());
      root = null;
      host?.remove();
      host = null;
    }
    navigateMock.mockClear();
  });

  it('renders one tile per album with title, artist and artwork, and no empty state', async () => {
    const host = mount();
    await settle(host, 'First Album');
    expect(host.textContent).toContain('First Album');
    expect(host.textContent).toContain('Second Album');
    // Null album_artist falls back to Unknown Artist on the tile.
    expect(host.textContent).toContain('Unknown Artist');
    expect(host.querySelectorAll('[data-testid="artwork"]').length).toBe(2);
    expect(host.textContent).not.toContain('Add Music to Your Library');
    albumRows = [];
  });

  it('shows the centred empty state without header scan actions', async () => {
    albumRows = [];
    const host = mount();
    await settle(host, 'Add Music to Your Library');
    expect(host.textContent).toContain('Add Music to Your Library');
    expect(host.textContent).toContain('Explore music and add your favourites to your Tansen library.');
    // The shared empty state's discovery action.
    const browse = Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.includes('Browse Tansen'));
    expect(browse).toBeTruthy();
    act(() => browse!.click());
    expect(navigateMock).toHaveBeenCalledWith('/search');
    // Folder imports live in Settings → Library; the empty page has no header action.
    expect(Array.from(host.querySelectorAll('button')).some((b) => b.textContent?.includes('Import folder'))).toBe(false);
    // No fabricated album tiles in the empty state.
    expect(host.querySelectorAll('[data-testid="artwork"]').length).toBe(0);
  });
});
