// @vitest-environment happy-dom
//
// Regression: the boot restore parsed localStorage with no guard, so a
// corrupt or older payload threw on startup. A bad payload is now dropped and
// the app starts clean.
import { describe, expect, it } from 'vitest';
import { parsePersisted } from './player';

const track = { videoId: 'v1', kind: 'remote' as const, title: 'Song', artist: 'Artist', album: null, durationSec: 200, thumbnail: null };

describe('restored queue', () => {
  it('discards corrupt JSON', () => {
    expect(parsePersisted('{not json')).toBeNull();
    expect(parsePersisted('null')).toBeNull();
    expect(parsePersisted(null)).toBeNull();
  });

  it('discards a payload without the current version', () => {
    expect(parsePersisted(JSON.stringify({ tracks: [track], index: 0, posMs: 0, volume: 1, muted: false }))).toBeNull();
  });

  it('keeps a current payload and drops rows without an id', () => {
    const saved = parsePersisted(
      JSON.stringify({
        v: 1,
        tracks: [track, { title: 'no id' }],
        index: 0,
        posMs: 1200,
        volume: 0.5,
        muted: false,
      }),
    );
    expect(saved?.tracks).toHaveLength(1);
    expect(saved?.posMs).toBe(1200);
  });
});
