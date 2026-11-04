import { describe, expect, it } from 'vitest';
import { mergeTrackRows, type QueueRow } from './queueRows';

const entry = (videoId: string, title: string, engineId: number) => ({
  track: {
    videoId,
    title,
    artists: [{ name: 'old', id: null }],
    album: { id: null as null, name: 'old-album' },
    durationSec: null,
    thumbnail: null,
    artworkUrl: null,
  },
  engineId,
});

const row = (id: number, title: string, extra: Partial<QueueRow> = {}): QueueRow => ({
  kind: 'local',
  id,
  title,
  artist: 'db-artist',
  album: 'db-album',
  duration_sec: 123,
  art_path: '/art/x.jpg',
  artwork_url: null,
  video_id: null,
  key_source: null,
  ...extra,
});

describe('mergeTrackRows', () => {
  it('updates matching local entries and preserves order', () => {
    const queue = [entry('local:7', 'old7', 7), entry('local:9', 'old9', 9)];
    const merged = mergeTrackRows(queue, [row(9, 'nine')], (p) => `art://${p.art_path}`);
    expect(merged[0]!.title).toBe('old7');
    expect(merged[1]!.title).toBe('nine');
    expect(merged[1]!.artists[0]!.name).toBe('db-artist');
    expect(merged[1]!.durationSec).toBe(123);
    expect(merged[1]!.thumbnail).toBe('art:///art/x.jpg');
  });

  it('leaves unknown ids untouched', () => {
    const queue = [entry('local:7', 'old7', 7), entry('hash:abc', 'remote', 9)];
    const merged = mergeTrackRows(queue, [row(7, 'seven')], () => null);
    expect(merged[1]!.title).toBe('remote');
    expect(merged[0]!.title).toBe('seven');
  });

  it('maps zero duration to null and missing art to null thumbnail', () => {
    const queue = [entry('local:7', 'old', 7)];
    const merged = mergeTrackRows(
      queue,
      [{ ...row(7, 'seven'), duration_sec: 0, art_path: null }],
      () => null,
    );
    expect(merged[0]!.durationSec).toBeNull();
    expect(merged[0]!.thumbnail).toBeNull();
  });

  it('matches remote rows by row id and applies remote fields', () => {
    const queue = [entry('vid-abc', 'Remote old', 9001)];
    const merged = mergeTrackRows(
      queue,
      [
        row(9001, 'Remote new', {
          kind: 'remote',
          album: 'Remote Album',
          artwork_url: 'https://i.ytimg.com/x.jpg',
          video_id: 'vid-abc',
          key_source: 'youtube',
        }),
      ],
      (r) => (r.kind === 'remote' ? `converted://${r.artwork_url}` : null),
    );
    expect(merged[0]!.title).toBe('Remote new');
    expect(merged[0]!.artists[0]!.name).toBe('db-artist');
    expect(merged[0]!.album!.name).toBe('Remote Album');
    expect(merged[0]!.thumbnail).toBe('converted://https://i.ytimg.com/x.jpg');
    expect(merged[0]!.artworkUrl).toBe('https://i.ytimg.com/x.jpg');
  });

  it('remote row without artwork keeps the entry artworkUrl', () => {
    const queue = [{ track: { ...entry('vid-abc', 'R', 9001).track, artworkUrl: 'https://i.ytimg.com/old.jpg' }, engineId: 9001 }];
    const merged = mergeTrackRows(queue, [row(9001, 'R', { kind: 'remote' })], () => null);
    expect(merged[0]!.thumbnail).toBeNull();
    expect(merged[0]!.artworkUrl).toBe('https://i.ytimg.com/old.jpg');
  });
});
