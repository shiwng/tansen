import { useQuery } from '@tanstack/react-query';
import { useMemo, useState } from 'react';
import { useSearchParams } from 'react-router';
import { invoke } from '@tauri-apps/api/core';
import { Segmented } from '@/components/Segmented';
import { SearchField } from '@/components/music/SearchField';
import { ShelfView } from '@/components/music/ShelfView';
import { TrackList } from '@/components/music/TrackList';
import { q, rowsToTracks, sourceFor } from '@/lib/rows';
import type { Track } from '@/data/music/types';

/** Search: the local library by default; YouTube Music behind the catalogue flag. */
export function SearchPage() {
  const [params, setParams] = useSearchParams();
  const [tab, setTab] = useState<'library' | 'youtube'>('library');
  const query = params.get('q') ?? '';

  return (
    <div>
      <div className="mx-(--body-gutter) pt-11 pb-5">
        <div className="flex items-center gap-3">
          <SearchField
            initial={query}
            onSearch={(v) => {
              if (v) setParams({ q: v });
              else setParams({});
            }}
          />
          <Segmented
            value={tab}
            onChange={setTab}
            options={[
              { value: 'library', label: 'Library' },
              { value: 'youtube', label: 'YouTube Music' },
            ]}
          />
        </div>
      </div>

      {query.trim() === '' ? (
        <p className="mx-(--body-gutter) type-body text-secondary">Type to search.</p>
      ) : tab === 'library' ? (
        <LibraryResults query={query} />
      ) : (
        <YouTubeResults query={query} />
      )}
    </div>
  );
}

function LibraryResults({ query }: { query: string }) {
  const tracks = useQuery(q.tracks());
  const albums = useQuery(q.albums());
  const artists = useQuery(q.artists());
  const source = sourceFor('search', query);

  const results = useMemo(() => {
    const needle = query.trim().toLowerCase();
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

  if (tracks.isError) {
    return <p className="mx-(--body-gutter) type-body text-secondary">{String(tracks.error)}</p>;
  }

  return (
    <div className="pb-10">
      {results.albumHits.length > 0 && (
        <div className="mb-8">
          <ShelfView
            shelf={{
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
            }}
          />
        </div>
      )}
      {results.artistHits.length > 0 && (
        <div className="mb-8">
          <ShelfView
            shelf={{
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
            }}
          />
        </div>
      )}
      <section className="mx-(--body-gutter) mb-8">
        <h2 className="mb-3 type-title-2-emphasized">Songs</h2>
        {results.songHits.length > 0 ? (
          <TrackList tracks={results.songHits} variant="playlist" source={source} />
        ) : (
          <p className="type-body text-secondary">Nothing matches “{query}”.</p>
        )}
      </section>
    </div>
  );
}

/** One YouTube row from the sidecar's search. */
interface YtRow {
  video_id: string;
  title: string;
  artist: string;
  album: string | null;
  duration_sec: number | null;
  thumbnail_url: string | null;
}

function YouTubeResults({ query }: { query: string }) {
  const yt = useQuery({
    queryKey: ['yt', 'search', query],
    queryFn: () => invoke<YtRow[]>('yt_search', { query }),
    enabled: query.trim().length > 0,
    staleTime: 10 * 60_000,
    retry: false,
  });

  const rows: Track[] = (yt.data ?? []).map((r) => ({
    videoId: r.video_id,
    title: r.title,
    artists: [{ name: r.artist || 'Unknown Artist', id: null }],
    album: r.album ? { id: null, name: r.album } : null,
    durationSec: r.duration_sec && r.duration_sec > 0 ? r.duration_sec : null,
    explicit: false,
    thumbnail: r.thumbnail_url,
    index: null,
    plays: null,
  }));
  const source = sourceFor('search', query, 'yt');

  return (
    <div className="mx-(--body-gutter) pb-10">
      {yt.isPending && <p className="type-body text-secondary">Searching YouTube Music…</p>}
      {yt.isError && (
        <p className="type-body text-secondary">
          YouTube Music is unavailable: {String(yt.error)}. Enable catalogue sources in Settings → Sources (needs the
          sidecar jar) and try again.
        </p>
      )}
      {yt.data && rows.length === 0 && <p className="type-body text-secondary">Nothing matches “{query}”.</p>}
      {rows.length > 0 && <TrackList tracks={rows} variant="playlist" source={source} />}
    </div>
  );
}
