import { useMemo } from 'react';
import { useSearchParams } from 'react-router';
import { SearchField } from '@/components/music/SearchField';
import { TrackList } from '@/components/music/TrackList';
import { ShelfView } from '@/components/music/ShelfView';
import { q, rowsToTracks, sourceFor } from '@/lib/rows';
import { useQuery } from '@tanstack/react-query';
import type { Track } from '@/data/music/types';

/** Search across the local library: albums and artists on top, then every matching song. */
export function SearchPage() {
  const [params, setParams] = useSearchParams();
  const query = params.get('q') ?? '';
  const tracks = useQuery(q.tracks());
  const albums = useQuery(q.albums());
  const artists = useQuery(q.artists());
  const source = sourceFor('search', query || 'Search');

  const results = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return null;
    const songHits = rowsToTracks(
      (tracks.data ?? []).filter((t) =>
        [t.title, t.artist, t.album, t.album_artist, t.genre].some((f) => f.toLowerCase().includes(needle)),
      ),
    );
    const albumHits = (albums.data ?? []).filter(
      (a) => a.album.toLowerCase().includes(needle) || a.album_artist.toLowerCase().includes(needle),
    );
    const artistHits = (artists.data ?? []).filter((a) => a.name.toLowerCase().includes(needle));
    return { songHits, albumHits, artistHits };
  }, [query, tracks.data, albums.data, artists.data]);

  return (
    <div>
      <div className="mx-(--body-gutter) pt-11 pb-5">
        <SearchField
          initial={query}
          onSearch={(v) => {
            if (v) setParams({ q: v });
            else setParams({});
          }}
        />
        <p className="mt-2 type-callout text-secondary">
          Searching your library. Catalogue search (YouTube Music, addons) arrives with the source system.
        </p>
      </div>

      {!results || !query.trim() ? (
        <p className="mx-(--body-gutter) type-body text-secondary">Type to search {tracks.data?.length ?? 0} local tracks.</p>
      ) : (
        <div className="pb-10">
          {results.albumHits.length > 0 && (
            <div className="mb-8">
              <ShelfView shelf={{
                title: 'Albums',
                strapline: null,
                layout: 'square',
                seeAll: null,
                items: results.albumHits.map((a) => ({
                  kind: 'album' as const,
                  id: a.key,
                  title: a.album,
                  subtitle: `Album • ${a.album_artist || 'Unknown Artist'}`,
                  artists: [{ name: a.album_artist || 'Unknown Artist', id: null }],
                  album: null,
                  thumbnail: a.art_path,
                  wide: false,
                  durationSec: null,
                  explicit: false,
                })),
              }} />
            </div>
          )}
          {results.artistHits.length > 0 && (
            <div className="mb-8">
              <ShelfView shelf={{
                title: 'Artists',
                strapline: null,
                layout: 'circle',
                seeAll: null,
                items: results.artistHits.map((a) => ({
                  kind: 'artist' as const,
                  id: a.name,
                  title: a.name,
                  subtitle: `Artist • ${a.track_count} songs`,
                  artists: [{ name: a.name, id: null }],
                  album: null,
                  thumbnail: a.art_path,
                  wide: false,
                  durationSec: null,
                  explicit: false,
                })),
              }} />
            </div>
          )}
          <section className="mx-(--body-gutter) mb-8">
            <h2 className="mb-3 type-title-2-emphasized">Songs</h2>
            {results.songHits.length > 0 ? (
              <TrackList tracks={results.songHits as Track[]} variant="playlist" source={source} />
            ) : (
              <p className="type-body text-secondary">Nothing matches “{query}”.</p>
            )}
          </section>
        </div>
      )}
    </div>
  );
}
