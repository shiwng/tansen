// @vitest-environment happy-dom
//
// The category detail page is the only surface with the contextual back
// control. This pins: hero + description, real discovery shelves, the control
// only when the page was opened from Browse Categories, and Escape mirroring
// the control.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { BrowseCategoryPage } from './BrowseCategoryPage';
import type { DiscoverySearchPage, DiscoveryTrack } from '@/lib/discovery';

// React 19's act() gate for a hand-mounted tree in a custom test environment.
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const { navigate, stateRef, invoke } = vi.hoisted(() => ({
  navigate: vi.fn(),
  stateRef: { value: null as unknown },
  invoke: vi.fn(),
}));

vi.mock('react-router', async () => {
  const React = await import('react');
  return {
    useParams: () => ({ category: 'bollywood' }),
    useLocation: () => ({ state: stateRef.value, pathname: '/browse/bollywood' }),
    useNavigate: () => navigate,
    Link: ({ to, children, ...rest }: { to: string; children: React.ReactNode }) =>
      React.createElement('a', { href: to, ...rest }, children),
  };
});

vi.mock('@tauri-apps/api/core', () => ({
  invoke,
  convertFileSrc: (path: string) => `art://localhost/${encodeURIComponent(path)}`,
}));

// Shelves are shared components with their own behaviour; this test is about
// the page's navigation and data wiring.
vi.mock('@/components/music/ShelfView', () => ({
  ShelfView: ({ shelf }: { shelf: { title: string } }) => <section data-shelf={shelf.title}>{shelf.title}</section>,
  TrackShelf: ({ title, tracks }: { title: string; tracks: unknown[] }) => (
    <section data-shelf={title}>{`${title}:${tracks.length}`}</section>
  ),
}));

const track = (id: string, title: string): DiscoveryTrack => ({
  key: { source: 'youtube', id },
  title,
  artist: 'Artist',
  album: 'Album',
  duration_sec: 200,
  thumbnail_url: 'https://yt3.googleusercontent.com/t=w544-h544',
  artists: [{ name: 'Artist', browse_id: 'UC1' }],
  album_id: 'MPREb_1',
  is_video: false,
  explicit: false,
  playable: true,
});

const page = (cards: DiscoverySearchPage['cards'], tracks: DiscoveryTrack[] = []): DiscoverySearchPage => ({
  cards,
  tracks,
  queries: [],
  continuation: null,
});

const playlists = page([
  {
    kind: 'playlist',
    browse_id: 'VLbollywood',
    title: 'Bollywood Classics',
    subtitle: 'Playlist',
    thumbnail_url: 'https://yt3.googleusercontent.com/b=w544-h544',
  },
]);

const albums = page([
  {
    kind: 'album',
    browse_id: 'MPREb_2',
    title: 'Bollywood Album',
    subtitle: 'Album • Artist',
    thumbnail_url: 'https://yt3.googleusercontent.com/a=w544-h544',
  },
]);

const artists = page([
  {
    kind: 'artist',
    browse_id: 'UC2',
    title: 'Bollywood Artist',
    subtitle: 'Artist',
    thumbnail_url: 'https://yt3.googleusercontent.com/c=w544-h544',
  },
]);

let mode: 'normal' | 'fail' | 'empty' = 'normal';

invoke.mockImplementation(async (command: string, args: { filter?: string }) => {
  if (command === 'fetch_artwork') return '/tmp/cover.jpg';
  if (command !== 'discovery_search') return null;
  if (mode === 'fail') throw new Error('sidecar unavailable');
  if (mode === 'empty') return [page([], []), []];
  switch (args.filter) {
    case 'songs':
      return [page([], [track('v1', 'Song One'), track('v2', 'Song Two')]), []];
    case 'albums':
      return [albums, []];
    case 'artists':
      return [artists, []];
    default:
      return [playlists, []];
  }
});

let root: Root | null = null;
let host: HTMLElement | null = null;

async function mount() {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, retryDelay: 0 } },
  });
  const render = () =>
    root!.render(
      <QueryClientProvider client={client}>
        <BrowseCategoryPage />
      </QueryClientProvider>,
    );
  await act(async () => render());
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
  return render;
}

afterEach(() => {
  if (root) {
    act(() => root!.unmount());
    root = null;
  }
  host?.remove();
  host = null;
  navigate.mockClear();
  stateRef.value = null;
  mode = 'normal';
});

describe('BrowseCategoryPage', () => {
  it('renders the category hero, description and real discovery shelves', async () => {
    stateRef.value = { from: 'browse' };
    await mount();

    expect(host!.textContent).toContain('Bollywood');
    expect(host!.textContent).toContain('Hindi film songs');
    for (const title of ['Featured Playlists', 'Albums', 'Artists', 'New Releases']) {
      expect(host!.querySelector(`[data-shelf="${title}"]`)).not.toBeNull();
    }
    expect(host!.querySelector('[data-shelf="Popular Tracks"]')?.textContent).toBe('Popular Tracks:2');

    const filters = invoke.mock.calls
      .filter(([command]) => command === 'discovery_search')
      .map(([, args]) => (args as { filter: string }).filter);
    expect(filters).toEqual(expect.arrayContaining(['playlists', 'songs', 'albums', 'artists']));
  });

  it('shows the glass back control only when entered from Browse Categories', async () => {
    stateRef.value = { from: 'browse' };
    await mount();
    const back = host!.querySelector('button[aria-label="Back to Browse Categories"]');
    expect(back).not.toBeNull();

    await act(async () => (back as HTMLButtonElement).click());
    expect(navigate).toHaveBeenCalledWith(-1);

    navigate.mockClear();
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(navigate).toHaveBeenCalledWith(-1);
  });

  it('has no back control on direct navigation', async () => {
    stateRef.value = null;
    await mount();
    expect(host!.querySelector('button[aria-label="Back to Browse Categories"]')).toBeNull();
  });

  it('explains a catalogue failure and offers a retry', async () => {
    stateRef.value = { from: 'browse' };
    mode = 'fail';
    await mount();

    expect(host!.textContent).toContain('shelves could not load');
    expect(host!.querySelector('button')?.textContent).toBeDefined();
    const retry = [...host!.querySelectorAll('button')].find((button) => button.textContent === 'Retry');
    expect(retry).not.toBeUndefined();
  });

  it('explains an empty catalogue instead of showing blank shelves', async () => {
    stateRef.value = { from: 'browse' };
    mode = 'empty';
    await mount();

    expect(host!.textContent).toContain('The catalogue has no results for Bollywood right now.');
    const tryAgain = [...host!.querySelectorAll('button')].find(
      (button) => button.textContent === 'Try again',
    );
    expect(tryAgain).not.toBeUndefined();
  });
});
