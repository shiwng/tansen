// @vitest-environment happy-dom
//
// Cover-art path regression. Catalogue thumbnails are https URLs on hosts
// outside the webview's `img-src` allowlist, so they must be fetched through
// `fetch_artwork` and rendered from the local `art://` cache. Two defects are
// pinned here:
//  1. every remote cover went through the Rust allowlist, which was missing
//     `yt3.googleusercontent.com` (the host live search actually returns);
//  2. rows that already hold an `art://` URL (queue mirror, library rows,
//     now-playing bar) were converted a second time, which 404s.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Artwork, isArtworkUrl } from './Artwork';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => '/Users/owner/Library/tansen/art/remote/abc.jpg'),
  convertFileSrc: (path: string) => `art://localhost/${encodeURIComponent(path)}`,
}));

const mocked = vi.mocked(invoke);
let root: Root | null = null;
let host: HTMLElement | null = null;

async function render(src: string | null) {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root!.render(<Artwork src={src} displayWidth={40} />);
  });
  return host.querySelector('img');
}

afterEach(() => {
  if (root) {
    act(() => root!.unmount());
    root = null;
  }
  host?.remove();
  host = null;
  mocked.mockClear();
});

describe('cover art URL', () => {
  it('routes a catalogue thumbnail through fetch_artwork and renders the art:// cache URL', async () => {
    const img = await render('https://yt3.googleusercontent.com/abc=w60-h60');

    expect(mocked).toHaveBeenCalledWith('fetch_artwork', {
      url: 'https://yt3.googleusercontent.com/abc=w60-h60',
    });
    expect(img?.getAttribute('src')).toBe(
      'art://localhost/%2FUsers%2Fowner%2FLibrary%2Ftansen%2Fart%2Fremote%2Fabc.jpg',
    );
  });

  it('leaves an already-resolved art:// URL alone', async () => {
    const img = await render('art://localhost/%2Ftmp%2Fa.jpg');

    expect(mocked).not.toHaveBeenCalled();
    expect(img?.getAttribute('src')).toBe('art://localhost/%2Ftmp%2Fa.jpg');
    expect(isArtworkUrl('http://art.localhost/tmp/a.jpg')).toBe(true);
    expect(isArtworkUrl('/tmp/a.jpg')).toBe(false);
  });

  it('renders no image when the fetch is refused', async () => {
    mocked.mockRejectedValueOnce(new Error('host not allowed: yt3.googleusercontent.com'));
    // A URL this module has not cached yet (the cache is process-wide).
    const img = await render('https://yt3.googleusercontent.com/def=w60-h60');

    expect(img).toBeNull();
  });
});
