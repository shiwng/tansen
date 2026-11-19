import { describe, expect, it } from 'vitest';
import { buildImportRows, dedupeTracks, mapPool, parseSpotifyPlaylistId, pickMatch } from './spotifyImport';

const row = (videoId: string, durationSec: number | null) => ({
  video_id: videoId,
  title: 'Song',
  artist: 'Artist',
  duration_sec: durationSec,
  thumbnail_url: `https://i.ytimg.com/${videoId}.jpg`,
});

describe('parseSpotifyPlaylistId', () => {
  it('accepts the shared URL, URI and bare-id forms', () => {
    expect(parseSpotifyPlaylistId('https://open.spotify.com/playlist/4hSEJgIGVwZP350BO6MvSk')).toBe(
      '4hSEJgIGVwZP350BO6MvSk',
    );
    expect(parseSpotifyPlaylistId('https://open.spotify.com/intl-de/playlist/4hSEJgIGVwZP350BO6MvSk?si=x')).toBe(
      '4hSEJgIGVwZP350BO6MvSk',
    );
    expect(parseSpotifyPlaylistId('spotify:playlist:4hSEJgIGVwZP350BO6MvSk')).toBe('4hSEJgIGVwZP350BO6MvSk');
    expect(parseSpotifyPlaylistId(' 4hSEJgIGVwZP350BO6MvSk ')).toBe('4hSEJgIGVwZP350BO6MvSk');
  });

  it('rejects non-playlist input', () => {
    expect(parseSpotifyPlaylistId('https://open.spotify.com/album/4hSEJgIGVwZP350BO6MvSk')).toBeNull();
    expect(parseSpotifyPlaylistId('https://open.spotify.com/playlist/shortid')).toBeNull();
    expect(parseSpotifyPlaylistId('spotify:track:4hSEJgIGVwZP350BO6MvSk')).toBeNull();
    expect(parseSpotifyPlaylistId('')).toBeNull();
  });
});

describe('dedupeTracks', () => {
  it('keeps the first occurrence of each Spotify id and counts repeats', () => {
    const a = { id: 'A', title: 'One', artist: 'X', duration_sec: 10, playable: true };
    const b = { id: 'B', title: 'Two', artist: 'Y', duration_sec: 11, playable: true };
    const a2 = { ...a, title: 'One (repeat)' };
    const { rows, duplicates } = dedupeTracks([a, b, a2, a]);
    expect(rows.map((r) => r.id)).toEqual(['A', 'B']);
    expect(duplicates).toBe(2);
  });
});

describe('pickMatch', () => {
  it('prefers the row within ±5 s of the Spotify duration', () => {
    const rows = [row('long', 300), row('close', 200.5)];
    expect(pickMatch(rows, 202)?.video_id).toBe('close');
  });

  it('falls back to the first row when no duration lines up, and null on empty', () => {
    expect(pickMatch([row('first', 300), row('second', 100)], 200)?.video_id).toBe('first');
    expect(pickMatch([row('first', null)], 200)?.video_id).toBe('first');
    expect(pickMatch([], 200)).toBeNull();
  });
});

describe('buildImportRows', () => {
  const track = (id: string, playable = true) => ({
    id,
    title: `Song ${id}`,
    artist: 'Artist',
    duration_sec: 200,
    playable,
  });

  it('maps playable rows to matched youtube rows and keeps the rest as spotify rows', async () => {
    const searches: string[] = [];
    const { rows, matched } = await buildImportRows(
      [track('sp1'), track('sp2'), track('sp3', false)],
      (query) => {
        searches.push(query);
        return Promise.resolve([row(`yt-${query}`, 201)]);
      },
      4,
      () => {},
    );
    expect(matched).toBe(2);
    expect(rows[0]).toMatchObject({ key_source: 'youtube', key_id: 'yt-Song sp1 Artist', title: 'Song sp1' });
    expect(rows[1]).toMatchObject({ key_source: 'youtube', key_id: 'yt-Song sp2 Artist' });
    expect(rows[2]).toMatchObject({ key_source: 'spotify', key_id: 'sp3' });
    expect(searches).toHaveLength(3);
  });

  it('reports progress for every row (dedupe is the caller’s step)', async () => {
    const progress: Array<[number, number]> = [];
    await buildImportRows([track('a'), track('a')], () => Promise.resolve([]), 2, (done, total) =>
      progress.push([done, total]),
    );
    expect(progress).toEqual([
      [1, 2],
      [2, 2],
    ]);
  });
});

describe('mapPool', () => {
  it('preserves order and respects the concurrency cap', async () => {
    let inFlight = 0;
    let peak = 0;
    const out = await mapPool([1, 2, 3, 4, 5], 2, async (n) => {
      inFlight += 1;
      peak = Math.max(peak, inFlight);
      await new Promise((r) => setTimeout(r, 1));
      inFlight -= 1;
      return n * 10;
    });
    expect(out).toEqual([10, 20, 30, 40, 50]);
    expect(peak).toBeLessThanOrEqual(2);
  });

  it('handles empty input', async () => {
    expect(await mapPool([], 4, async (n) => n)).toEqual([]);
  });
});

describe('pickMatch artist sanity', () => {
  const row = (videoId: string, artist: string, dur: number) => ({
    video_id: videoId,
    title: 'Song',
    artist,
    duration_sec: dur,
    thumbnail_url: null,
  });

  it('prefers a duration-close candidate whose artist overlaps the Spotify artist', () => {
    const rows = [row('cover', 'Some Tribute Band', 200.5), row('real', 'Tame Impala', 201)];
    expect(pickMatch(rows, 201, 'Tame Impala')?.video_id).toBe('real');
  });

  it('falls back to duration-close without artist overlap when nothing overlaps', () => {
    const rows = [row('a', 'Other Artist', 200.5), row('b', 'Another One', 201)];
    expect(pickMatch(rows, 201, 'Tame Impala')?.video_id).toBe('a');
  });
});

describe('buildImportRows cancellation and partial failures', () => {
  const track = { id: 'sp1', title: 'Song', artist: 'Artist', duration_sec: 200, playable: true };

  it('aborts with ImportCancelled when cancelled and never calls search again', async () => {
    let searches = 0;
    await expect(
      buildImportRows([track, track, track], () => {
        searches += 1;
        return Promise.resolve([{ video_id: 'v', title: 'Song', artist: 'Artist', duration_sec: 200, thumbnail_url: null }]);
      }, 1, () => {}, () => true),
    ).rejects.toThrow('import cancelled');
    expect(searches).toBe(0);
  });

  it('treats a failed lookup as a partial failure: row imports unmatched', async () => {
    const { rows, matched } = await buildImportRows(
      [track],
      () => Promise.reject(new Error('sidecar down')),
      1,
      () => {},
    );
    expect(matched).toBe(0);
    expect(rows[0]).toMatchObject({ key_source: 'spotify', key_id: 'sp1' });
  });
});
