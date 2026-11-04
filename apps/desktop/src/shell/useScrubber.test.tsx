// @vitest-environment happy-dom
//
// Regression: the scrubber was still written against the retired HTMLAudio
// element (`getAudio()` returns null on the engine backend), so pointer-down
// bailed out and no `Seek` command was ever sent — dragging the bar did
// nothing. It now reads the duration from the published state and seeks the
// engine directly.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { usePlayer } from '@/state/player';
import { useScrubber } from './useScrubber';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => null),
  convertFileSrc: (path: string) => `art://localhost/${encodeURIComponent(path)}`,
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));

const mocked = vi.mocked(invoke);

function Harness() {
  const { bar, handlers } = useScrubber();
  return <div data-testid="bar" ref={bar} {...handlers} />;
}

let root: Root | null = null;
let host: HTMLElement | null = null;

beforeEach(() => {
  mocked.mockClear();
  usePlayer.setState({
    status: 'playing',
    nowPlaying: { videoId: 'v1', title: 'Song', artist: 'Artist', artworkUrl: null, durationSec: 200 },
  });
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  act(() => root!.render(<Harness />));
});

afterEach(() => {
  if (root) {
    act(() => root!.unmount());
    root = null;
  }
  host?.remove();
  host = null;
});

function drag(fromX: number, toX: number) {
  const bar = host!.querySelector('[data-testid="bar"]') as HTMLElement;
  bar.getBoundingClientRect = () => ({ left: 0, width: 100, top: 0, height: 10, right: 100, bottom: 10, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect;
  // happy-dom has no pointer capture; the hook only needs it to keep the drag.
  bar.setPointerCapture = () => {};
  act(() => {
    bar.dispatchEvent(new PointerEvent('pointerdown', { clientX: fromX, pointerId: 1, bubbles: true }));
  });
  act(() => {
    bar.dispatchEvent(new PointerEvent('pointermove', { clientX: toX, pointerId: 1, bubbles: true }));
  });
  act(() => {
    bar.dispatchEvent(new PointerEvent('pointerup', { clientX: toX, pointerId: 1, bubbles: true }));
  });
}

describe('scrubber seek', () => {
  it('sends a Seek command at the dragged position', () => {
    drag(10, 50);

    expect(mocked).toHaveBeenCalledWith('engine_command', { cmd: { Seek: { ms: 100_000 } } });
  });

  it('does nothing when the track has no known duration', () => {
    act(() => {
      usePlayer.setState({ nowPlaying: { videoId: 'v1', title: 'Song', artist: 'Artist', artworkUrl: null, durationSec: 0 } });
    });
    drag(10, 50);

    expect(mocked).not.toHaveBeenCalled();
  });
});
