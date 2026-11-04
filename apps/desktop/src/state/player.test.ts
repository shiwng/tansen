// Regression: the remote Play path used snake_case invoke arguments
// (`key_source`, `key_id`, `duration_sec`, `artwork_url`), but Tauri names
// command arguments in camelCase. Every real Play click on a catalogue row
// died with "missing required key keySource" before the engine saw anything,
// while the debug E2E driver (which writes the DB and drives the engine
// directly) kept passing. This test pins the wire shape the button produces.
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { usePlayer } from './player';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => `art://localhost/${encodeURIComponent(path)}`,
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));

const remoteTrack = {
  videoId: '4D7u5KF7SP8',
  title: 'Get Lucky',
  artists: [{ name: 'Daft Punk', id: null }],
  album: { id: null, name: 'Random Access Memories' },
  durationSec: 369,
  explicit: false,
  thumbnail: 'https://yt3.googleusercontent.com/abc=w60-h60',
  index: null,
  plays: null,
  sourceKind: 'youtube',
  artworkUrl: 'https://yt3.googleusercontent.com/abc=w60-h60',
};

const mocked = vi.mocked(invoke);

beforeEach(() => {
  mocked.mockReset();
  mocked.mockImplementation(async (cmd: string) => (cmd === 'remote_track_upsert' ? 42 : null));
});

describe('remote Play click path', () => {
  it('registers the row with camelCase argument names', async () => {
    await usePlayer.getState().playTrack(remoteTrack as never, null);

    expect(mocked).toHaveBeenCalledWith('remote_track_upsert', {
      keySource: 'youtube',
      keyId: '4D7u5KF7SP8',
      title: 'Get Lucky',
      artist: 'Daft Punk',
      album: 'Random Access Memories',
      durationSec: 369,
      artworkUrl: 'https://yt3.googleusercontent.com/abc=w60-h60',
    });
  });

  it('clears, enqueues and plays the registered entry in that order', async () => {
    await usePlayer.getState().playTrack(remoteTrack as never, null);

    const commands = mocked.mock.calls
      .filter(([cmd]) => cmd === 'engine_command')
      .map(([, args]) => (args as { cmd: unknown }).cmd);
    expect(commands).toEqual([
      'ClearQueue',
      { Enqueue: { tracks: [42], remote: [[42, ['youtube', '4D7u5KF7SP8']]], placement: 'Append' } },
      { Play: { entry: 42, remote: ['youtube', '4D7u5KF7SP8'] } },
    ]);
    expect(usePlayer.getState().status).toBe('loading');
  });

  it('reports a visible error instead of a stuck UI when registration is rejected', async () => {
    mocked.mockImplementation(async (cmd: string) => {
      if (cmd === 'remote_track_upsert') throw new Error('missing required key keySource');
      return null;
    });

    await usePlayer.getState().playTrack(remoteTrack as never, null);

    expect(usePlayer.getState().status).toBe('error');
    expect(usePlayer.getState().error).toBe('Could not register remote track metadata');
    expect(mocked.mock.calls.some(([cmd]) => cmd === 'engine_command')).toBe(false);
  });
});
