import { useQuery, useQueryClient } from '@tanstack/react-query';
import { ChevronLeft } from 'lucide-react';
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { Link, useLocation, useNavigate, useParams } from 'react-router';
import { useArtworkUrl } from '@/components/music/Artwork';
import { ShelfView, TrackShelf } from '@/components/music/ShelfView';
import type { MusicItem, Shelf, Track } from '@/data/music/types';
import { findBrowseCategory, type BrowseCategory } from '@/data/browse/categories';
import { artworkAtPixels } from '@/lib/artwork';
import {
  categoryScrollKey,
  readPageScroll,
  writePageScroll,
} from '@/lib/browseScroll';
import {
  cardToItem,
  discoverySearch,
  discoveryToTrack,
  type DiscoveryFilter,
  type DiscoverySearchPage,
} from '@/lib/discovery';
import { sourceFor } from '@/lib/rows';
import { useCategoryArt } from '@/lib/useCategoryArt';
import { useUi } from '@/state/ui';

/** A category page: real playlist/album/artist/song shelves for one genre. */
export function BrowseCategoryPage() {
  const { category: raw } = useParams<{ category: string }>();
  const category = findBrowseCategory(raw);
  return category ? <CategoryDetail category={category} /> : <UnknownCategory />;
}

function useCategoryShelf(
  categoryId: string,
  key: string,
  query: string,
  filter: DiscoveryFilter,
  limit: number,
) {
  return useQuery({
    queryKey: ['browse-category', categoryId, key, query, limit],
    queryFn: () => discoverySearch(query, filter, limit),
    staleTime: 10 * 60_000,
    retry: 1,
  });
}

function shelfFromCards(
  title: string,
  page: DiscoverySearchPage | undefined,
  layout: 'square' | 'circle',
): Shelf | null {
  if (!page) return null;
  const seen = new Set<string>();
  const items: MusicItem[] = [];
  for (const card of page.cards) {
    const item = cardToItem(card);
    if (!item) continue;
    const key = `${item.kind}:${item.id}`;
    if (seen.has(key)) continue;
    seen.add(key);
    items.push(item);
  }
  return items.length ? { title, strapline: null, layout, items, seeAll: null } : null;
}

function CategoryDetail({ category }: { category: BrowseCategory }) {
  const location = useLocation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const state = location.state as { from?: string } | null;
  const fromBrowse = state?.from === 'browse';
  const fullScreenPlayer = useUi((ui) => ui.fullScreen);

  const playlists = useCategoryShelf(category.id, 'playlists', category.query, 'playlists', 12);
  const songs = useCategoryShelf(category.id, 'songs', category.query, 'songs', 24);
  const albums = useCategoryShelf(category.id, 'albums', category.query, 'albums', 12);
  const artists = useCategoryShelf(category.id, 'artists', category.query, 'artists', 12);
  const releases = useCategoryShelf(category.id, 'new-releases', `${category.label} new releases`, 'albums', 12);

  const categoryArt = useCategoryArt(category);
  const heroArt =
    categoryArt ??
    playlists.data?.cards.find((card) => card.thumbnail_url)?.thumbnail_url ??
    null;
  const heroUrl = useArtworkUrl(heroArt ? artworkAtPixels(heroArt, 1200) : null);
  const [heroLoaded, setHeroLoaded] = useState(false);
  const heroImage = useRef<HTMLImageElement | null>(null);
  useEffect(() => setHeroLoaded(false), [heroUrl]);
  // A cached image can finish before React attaches onLoad, which would leave
  // the sharp hero at opacity 0 behind its blurred copy. Check completeness
  // once the element is mounted as well as listening for the event.
  useEffect(() => {
    const image = heroImage.current;
    if (image?.complete && image.naturalWidth > 0) setHeroLoaded(true);
  }, [heroUrl]);

  const playlistShelf = useMemo(
    () => shelfFromCards('Featured Playlists', playlists.data, 'square'),
    [playlists.data],
  );
  const albumShelf = useMemo(() => shelfFromCards('Albums', albums.data, 'square'), [albums.data]);
  const artistShelf = useMemo(
    () => shelfFromCards('Artists', artists.data, 'circle'),
    [artists.data],
  );
  const releaseShelf = useMemo(
    () => shelfFromCards('New Releases', releases.data, 'square'),
    [releases.data],
  );
  const tracks = useMemo<Track[]>(() => {
    const seen = new Set<string>();
    const rows: Track[] = [];
    for (const row of songs.data?.tracks ?? []) {
      if (!row.playable || seen.has(row.key.id)) continue;
      seen.add(row.key.id);
      rows.push(discoveryToTrack(row, rows.length));
    }
    return rows;
  }, [songs.data]);

  const source = sourceFor('search', category.label, `browse:${category.id}`);
  const hasContent = Boolean(playlistShelf || albumShelf || artistShelf || releaseShelf || tracks.length);
  const queries = [playlists, songs, albums, artists, releases];
  const anyPending = queries.some((query) => query.isPending);
  const failed = queries.filter((query) => query.isError).length;

  const goBack = useCallback(() => navigate(-1), [navigate]);

  // Escape mirrors the circular control, but only inside the Browse flow.
  useEffect(() => {
    if (!fromBrowse) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.defaultPrevented || fullScreenPlayer) return;
      const target = event.target;
      if (target instanceof Element && target.closest('input, textarea, select, [contenteditable="true"]')) return;
      if (document.querySelector('[role="menu"], [role="dialog"]')) return;
      navigate(-1);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [fromBrowse, fullScreenPlayer, navigate]);

  // Restore this category's shelf position after a playlist visit; start a
  // different category at the top instead of inheriting the previous offset.
  const scrollKey = categoryScrollKey(category.id);
  useLayoutEffect(() => {
    const page = document.getElementById('scrollable-page');
    if (!page) return;
    const saved = readPageScroll(scrollKey);
    let second = 0;
    const first = requestAnimationFrame(() => {
      second = requestAnimationFrame(() => {
        page.scrollTop = saved ?? 0;
      });
    });
    return () => {
      cancelAnimationFrame(first);
      cancelAnimationFrame(second);
      writePageScroll(scrollKey);
    };
  }, [scrollKey]);

  const retry = () => {
    void queryClient.invalidateQueries({ queryKey: ['browse-category', category.id] });
    void queryClient.invalidateQueries({ queryKey: ['browse-category-art', category.id] });
  };

  const [from] = category.tint;

  return (
    <div className="pb-16">
      <section
        aria-labelledby="browse-category-title"
        className="relative mx-(--body-gutter) mt-4 overflow-hidden rounded-[var(--radius-artwork-large)] bg-joe artwork-stroke"
      >
        <div className="relative h-[clamp(220px,24vw,340px)]">
          {/* The fallback is the category's own tint; no generic stock image. */}
          <div
            aria-hidden="true"
            className="absolute inset-0"
            style={{
              background: `radial-gradient(90% 130% at 12% 0%, ${from} 0%, transparent 62%), linear-gradient(130deg, ${category.tint[0]}, ${category.tint[1]})`,
            }}
          />
          {heroUrl && (
            <>
              <img
                src={heroUrl}
                alt=""
                aria-hidden="true"
                draggable={false}
                referrerPolicy="no-referrer"
                className="absolute inset-0 size-full scale-110 object-cover opacity-45 blur-2xl"
              />
              <img
                ref={heroImage}
                src={heroUrl}
                alt=""
                draggable={false}
                referrerPolicy="no-referrer"
                data-loaded={heroLoaded || undefined}
                onLoad={() => setHeroLoaded(true)}
                className="absolute inset-0 size-full object-cover opacity-0 transition-opacity duration-500 ease-out data-loaded:opacity-100 motion-reduce:transition-none"
              />
            </>
          )}
          <div
            aria-hidden="true"
            className="absolute inset-0"
            style={{
              background: `linear-gradient(120deg, ${from}59 0%, transparent 58%), linear-gradient(0deg, #000000e6 0%, #00000066 46%, #0000001f 100%)`,
            }}
          />
          {fromBrowse && (
            <button
              onClick={goBack}
              aria-label="Back to Browse Categories"
              title="Back to Browse Categories"
              className="glass-pill glass-on-art absolute top-4 left-4 z-2 grid size-10 place-items-center rounded-full transition-[scale] duration-150 hover:scale-[1.04] active:scale-95 focus-visible:ring-[3px] focus-visible:ring-white/70 focus-visible:outline-none motion-reduce:scale-100 motion-reduce:transition-none"
            >
              <ChevronLeft size={20} strokeWidth={2.4} />
            </button>
          )}
          <div className="absolute inset-x-0 bottom-0 flex flex-col gap-1.5 p-5 sm:p-6">
            <p className="type-caption font-[600] tracking-[0.14em] text-white/60 uppercase">Browse Category</p>
            <h1
              id="browse-category-title"
              className="type-header-emphasized text-white [text-shadow:0_2px_16px_#0009]"
            >
              {category.label}
            </h1>
            <p className="max-w-[560px] type-body text-white/80 [text-shadow:0_1px_8px_#0009]">
              {category.description}
            </p>
          </div>
        </div>
      </section>

      {playlistShelf && <ShelfView shelf={playlistShelf} />}
      {tracks.length > 0 && (
        <TrackShelf title="Popular Tracks" tracks={tracks} source={source} />
      )}
      {albumShelf && <ShelfView shelf={albumShelf} />}
      {artistShelf && <ShelfView shelf={artistShelf} />}
      {releaseShelf && <ShelfView shelf={releaseShelf} />}

      {failed > 0 && (
        <div className="mx-(--body-gutter) mt-7 flex flex-wrap items-center gap-3 type-callout text-secondary">
          <span>
            {failed === 1 ? 'One category shelf could not load.' : `${failed} category shelves could not load.`}
          </span>
          <button
            onClick={retry}
            className="rounded-pill bg-quinary px-3 py-1 type-callout-emphasized text-primary hover:bg-quaternary"
          >
            Retry
          </button>
        </div>
      )}

      {!anyPending && !hasContent && (
        <div className="mx-(--body-gutter) mt-8 max-w-lg">
          <p className="type-body text-secondary">
            The catalogue has no results for {category.label} right now.
          </p>
          <button
            onClick={retry}
            className="mt-3 rounded-pill bg-quinary px-4 py-2 type-callout-emphasized text-primary hover:bg-quaternary"
          >
            Try again
          </button>
        </div>
      )}
    </div>
  );
}

function UnknownCategory() {
  return (
    <div className="mx-(--body-gutter) pt-11 pb-16">
      <h1 className="type-header-emphasized">Category not found</h1>
      <p className="mt-2 max-w-lg type-body text-secondary">
        This browse category isn't part of Tansen's catalogue grid.
      </p>
      <Link
        to="/search"
        className="mt-4 inline-flex rounded-pill bg-quinary px-4 py-2 type-callout-emphasized text-primary hover:bg-quaternary"
      >
        Browse Categories
      </Link>
    </div>
  );
}
