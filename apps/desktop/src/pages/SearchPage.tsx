import { useQuery } from '@tanstack/react-query';
import { useEffect, useMemo, useState } from 'react';
import { Link, useSearchParams } from 'react-router';
import { invoke } from '@tauri-apps/api/core';
import { Segmented } from '@/components/Segmented';
import { LogoMark } from '@/components/icons/LogoMark';
import { BrowseCategoryCard } from '@/components/music/BrowseCategoryCard';
import { ShelfView } from '@/components/music/ShelfView';
import { TrackList } from '@/components/music/TrackList';
import { hrefFor } from '@/components/music/links';
import { BROWSE_CATEGORIES } from '@/data/browse/categories';
import { restoreBrowseOrigin } from '@/lib/browseScroll';
import {
  discoveryToTrack,
  type DiscoveryFilter,
  type DiscoverySearchPage,
  type DiscoveryTrack,
} from '@/lib/discovery';
import { q, rowsToTracks, sourceFor } from '@/lib/rows';
import {
  type SearchTrackRow,
} from '@/lib/searchGroups';

/** Search: the local library by default; YouTube Music behind the catalogue flag. */
/** Search: the browse page when idle, results for the query in the URL. */
export function SearchPage() {
  const [params] = useSearchParams();
  const [filter, setFilter] = useState<DiscoveryFilter>('songs');
  const query = params.get('q') ?? '';
  const libraryScope = params.get('scope') === 'library';

  if (query.trim() === '') return <BrowseHome />;

  return (
    <div>
      {!libraryScope && (
        <div className="mx-(--body-gutter) max-w-full overflow-x-auto pt-2 pb-5">
          <Segmented
            value={filter}
            onChange={setFilter}
            options={[
              { value: 'songs', label: 'Songs' },
              { value: 'videos', label: 'Videos' },
              { value: 'albums', label: 'Albums' },
              { value: 'artists', label: 'Artists' },
              { value: 'playlists', label: 'Playlists' },
            ]}
          />
        </div>
      )}
      {libraryScope ? <LibraryResults query={query} /> : <SourcesResults query={query} filter={filter} />}
    </div>
  );
}

/**
 * Browse Categories: real catalogue artwork per tile. Each tile opens its own
 * detail page, and the grid's scroll offset is restored on the way back.
 */
function BrowseHome() {
  useEffect(() => restoreBrowseOrigin(), []);
  const sources = useQuery({
    queryKey: ['sources'],
    queryFn: () => invoke<{ id: string; name: string; kind: string; enabled: boolean }[]>('sources_get'),
    staleTime: 60_000,
    retry: false,
  });
  const catalogueOff = sources.isSuccess && !sources.data.some((source) => source.enabled && source.kind !== 'local');

  return (
    <div>
      {catalogueOff && (
        <div className="mx-(--body-gutter) mt-2 flex items-center gap-5 rounded-[12px] bg-[#0a6cff] px-6 py-3.5 text-white">
          <LogoMark size={26} className="flex-none" />
          <span className="h-8 w-px flex-none bg-white/30" />
          <div className="min-w-0 flex-1">
            <p className="truncate type-body-emphasized">Catalogue search is off.</p>
            <p className="truncate type-callout text-white/80">Turn on a source to search and play its music.</p>
          </div>
          <Link to="/settings" className="flex-none rounded-[6px] bg-white px-4 py-1.5 type-callout-emphasized text-black">
            Open Settings
          </Link>
        </div>
      )}
      <div className="mx-(--body-gutter) pt-5">
        <h1 className="mb-4 type-title-2-emphasized">Browse Categories</h1>
        <div className="grid grid-cols-2 gap-x-4 gap-y-5 sm:grid-cols-3 lg:grid-cols-5">
          {BROWSE_CATEGORIES.map((category, index) => (
            <BrowseCategoryCard key={category.id} category={category} eager={index < 10} />
          ))}
        </div>
      </div>
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

interface SearchTrackResponse {
  page: DiscoverySearchPage;
  local: SearchTrackRow[];
}

/**
 * Unified sources search: one query across every enabled source (local
 * library included via the backend seam), rendered as per-source sections
 * in rank order with unobtrusive per-source status.
 */
function SourcesResults({ query, filter }: { query: string; filter: DiscoveryFilter }) {
  const [more, setMore] = useState<DiscoveryTrack[]>([]);
  const [continuation, setContinuation] = useState<string | null>(null);
  const search = useQuery({
    queryKey: ['catalogue-search', query, filter],
    queryFn: async (): Promise<SearchTrackResponse> => {
      const [page, local] = await invoke<[DiscoverySearchPage, SearchTrackRow[]]>(
        'discovery_search',
        { query, filter, continuation: null, limit: 30 },
      );
      return { page, local };
    },
    enabled: query.trim().length > 0,
    staleTime: 10 * 60_000,
    retry: false,
  });

  useEffect(() => {
    setMore([]);
    setContinuation(search.data?.page.continuation ?? null);
  }, [query, filter, search.data]);

  const tracks = [...(search.data?.page.tracks ?? []), ...more].map(discoveryToTrack);
  const local = (search.data?.local ?? []).map((row) => rowToTrack(row));
  const cards = search.data?.page.cards ?? [];
  const source = sourceFor('search', query, 'remote');
  const loadMore = async () => {
    if (!continuation) return;
    const [page] = await invoke<[DiscoverySearchPage, SearchTrackRow[]]>('discovery_search', {
      query,
      filter,
      continuation,
      limit: 30,
    });
    setMore((rows) => [...rows, ...page.tracks]);
    setContinuation(page.continuation);
  };
  return (
    <div className="mx-(--body-gutter) pb-10">
      {search.isPending && <p className="type-body text-secondary">Searching sources…</p>}
      {search.isError && (
        <p className="type-body text-secondary">
          Search unavailable: {String(search.error)}. Enable catalogue sources in Settings → Sources and try again.
        </p>
      )}
      {search.data && tracks.length === 0 && cards.length === 0 && local.length === 0 && (
        <p className="type-body text-secondary">Nothing matches “{query}”.</p>
      )}
      {cards.length > 0 && <div className="mb-5 flex flex-wrap gap-2">
        {cards.map((card) => {
          const kind = card.kind === 'album' || card.kind === 'artist' || card.kind === 'playlist' ? card.kind : null;
          const href = kind && hrefFor({ kind, id: card.browse_id });
          return href ? <Link key={`${kind}-${card.browse_id}`} className="rounded-[10px] bg-quinary px-3 py-2 type-callout text-secondary" to={href}>{kind}: {card.title}</Link> : null;
        })}
      </div>}
      {local.length > 0 && <section className="mb-6"><h3 className="mb-2 type-callout-emphasized text-secondary">Local Library</h3><TrackList tracks={local} variant="playlist" source={sourceFor('search', query)} /></section>}
      {tracks.length > 0 && <section className="mb-6"><h3 className="mb-2 type-callout-emphasized text-secondary">YouTube Music</h3><TrackList tracks={tracks} variant="playlist" source={source} /></section>}
      {continuation && <button className="rounded-pill bg-quinary px-4 py-2 type-callout text-primary" onClick={() => void loadMore()}>Load more</button>}
    </div>
  );
}

function rowToTrack(row: SearchTrackRow) {
  return {
    videoId: row.key.source === 'local' ? `local:${row.key.id}` : row.key.id,
    title: row.title,
    artists: [{ name: row.artist || 'Unknown Artist', id: null }],
    album: row.album ? { id: null, name: row.album } : null,
    durationSec: row.duration_sec,
    explicit: false,
    thumbnail: row.artwork_url,
    index: null,
    plays: null,
    sourceKind: row.key.source,
    artworkUrl: row.artwork_url,
  };
}
