import { useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { PlayIcon } from '@/components/icons/Transport';
import { PrefetchLink } from '@/components/PrefetchLink';
import { Artwork } from '@/components/music/Artwork';
import { ShelfView } from '@/components/music/ShelfView';
import { hrefFor } from '@/components/music/links';
import { playTrack } from '@/components/music/play';
import { SkeletonBlock } from '@/components/Skeleton';
import { RotateCw } from 'lucide-react';
import { useState } from 'react';
import type { ItemKind, MusicItem, Shelf } from '@/data/music/types';

/**
 * Home: real catalogue tiles and shelves, ranked by the listening profile in
 * the Rust recommendation service. No account is required — new listeners
 * get general discovery, and own listening replaces it as events accumulate.
 */

interface HomeItem {
  kind: string;
  id: string;
  title: string;
  subtitle: string;
  thumbnail_url: string | null;
  duration_sec: number | null;
  is_video: boolean;
}

interface HomeSection {
  id: string;
  title: string;
  subtitle: string;
  layout: string;
  items: HomeItem[];
}

interface HomeFeed {
  personalized: boolean;
  plays: number;
  top_artists: string[];
  featured: HomeItem[];
  sections: HomeSection[];
  catalogue_error: string | null;
}

const KINDS: ItemKind[] = ['song', 'video', 'album', 'playlist', 'artist', 'episode', 'podcast'];

function itemKind(item: HomeItem): ItemKind {
  return KINDS.includes(item.kind as ItemKind) ? (item.kind as ItemKind) : 'unknown';
}

function musicItem(item: HomeItem): MusicItem {
  return {
    kind: itemKind(item),
    id: item.id,
    title: item.title,
    subtitle: item.subtitle,
    artists: item.kind === 'song' || item.kind === 'video' ? [{ name: item.subtitle || 'Unknown Artist', id: null }] : [],
    album: null,
    thumbnail: item.thumbnail_url,
    wide: item.is_video,
    durationSec: item.duration_sec,
    explicit: false,
  };
}

function shelfFor(section: HomeSection): Shelf {
  const layout = section.layout === 'video' ? 'video' : section.layout === 'circle' ? 'circle' : 'square';
  return {
    title: section.title,
    strapline: section.subtitle || null,
    layout,
    items: section.items.map(musicItem),
    seeAll: null,
  };
}

export function HomePage() {
  const [refresh, setRefresh] = useState(0);
  const feed = useQuery({
    queryKey: ['home', 'feed', refresh],
    queryFn: () => invoke<HomeFeed>('home_feed', { refresh: refresh > 0 }),
    retry: false,
    staleTime: 5 * 60_000,
    // Cheap: the backend answers from its cache unless the profile changed.
    refetchInterval: 5 * 60_000,
    refetchIntervalInBackground: false,
  });

  const data = feed.data;
  const sections = data?.sections ?? [];
  const featured = data?.featured?.length
    ? data.featured
    : sections.flatMap((section) => section.items.filter((item) => item.thumbnail_url)).slice(0, 10);

  return (
    <div className="pb-16">
      <header className="mx-(--body-gutter) flex flex-wrap items-end justify-between gap-x-6 gap-y-2 pt-8">
        <div className="min-w-0">
          <h1 className="type-header-emphasized">Home</h1>
          <p className="mt-1 type-callout text-secondary">
            {data?.personalized
              ? `Picked for you from ${data.plays} ${data.plays === 1 ? 'play' : 'plays'}${
                  data.top_artists.length ? ` · ${data.top_artists.join(', ')}` : ''
                }`
              : 'Popular music, new releases and artists to explore'}
          </p>
        </div>
        <button
          className="mb-1 flex items-center gap-1.5 rounded-pill bg-quinary px-3 py-1.5 type-callout text-secondary transition-colors hover:text-primary disabled:opacity-50"
          onClick={() => setRefresh((n) => n + 1)}
          disabled={feed.isFetching}
          aria-label="Refresh recommendations"
        >
          <RotateCw size={13} strokeWidth={2.2} className={feed.isFetching ? 'animate-spin' : undefined} />
          {feed.isFetching ? 'Refreshing…' : 'Refresh'}
        </button>
      </header>

      {feed.isPending && <HomeSkeleton />}

      {feed.isError && (
        <div className="mx-(--body-gutter) mt-8 max-w-lg">
          <p className="type-body text-secondary">
            Home is unavailable: {String(feed.error).replace(/^Error:\s*/, '')}
          </p>
          <button
            className="mt-3 rounded-pill bg-quinary px-4 py-2 type-callout-emphasized text-primary"
            onClick={() => setRefresh((n) => n + 1)}
          >
            Try again
          </button>
        </div>
      )}

      {data && featured.length > 0 && (
        <section aria-label="Featured" className="mx-(--body-gutter) mt-7">
          <div className="grid grid-cols-2 gap-x-5 gap-y-5 sm:grid-cols-3 lg:grid-cols-5">
            {featured.map((item) => (
              <FeatureTile key={`${item.kind}-${item.id}`} item={item} />
            ))}
          </div>
        </section>
      )}

      {data && sections.map((section) => <ShelfView key={section.id} shelf={shelfFor(section)} />)}

      {data && sections.length === 0 && featured.length === 0 && (
        <p className="mx-(--body-gutter) mt-8 max-w-lg type-body text-secondary">
          No catalogue shelves are available right now. Check Settings → Sources, then refresh.
        </p>
      )}

      {data?.catalogue_error && (
        <p className="mx-(--body-gutter) mt-8 flex items-center gap-2 type-caption text-secondary">
          Some sections could not load.
          <button
            className="rounded-pill bg-quinary px-3 py-1 type-caption text-primary hover:bg-quaternary"
            onClick={() => setRefresh((n) => n + 1)}
            disabled={feed.isFetching}
          >
            Retry
          </button>
        </p>
      )}
    </div>
  );
}

/** A large browse tile, matching the reference's five-column feature grid. */
function FeatureTile({ item }: { item: HomeItem }) {
  const music = musicItem(item);
  const href = hrefFor(music);
  const playable = music.kind === 'song' || music.kind === 'video';
  const body = (
    <>
      <Artwork src={item.thumbnail_url} displayWidth={280} aspect={5 / 3} radius="var(--radius-artwork-large)" className="min-h-full" />
      <span className="pointer-events-none absolute inset-0 z-1 rounded-[inherit] bg-gradient-to-t from-black/85 via-black/25 to-transparent" />
      <span className="absolute inset-x-0 bottom-0 z-2 line-clamp-2 px-3 pb-2.5 type-title-3-emphasized text-white [text-shadow:0_1px_6px_#0009]">
        {item.title}
      </span>
      {playable && (
        <span className="pointer-events-none absolute top-2.5 right-2.5 z-2 grid size-8 place-items-center rounded-full bg-black/45 text-white opacity-0 backdrop-blur-sm transition-opacity duration-150 group-hover/tile:opacity-100">
          <PlayIcon size={16} />
        </span>
      )}
    </>
  );
  const className =
    'group/tile relative aspect-[5/3] overflow-hidden rounded-[var(--radius-artwork-large)] bg-joe artwork-stroke';
  if (href) {
    return (
      <PrefetchLink to={href} className={className} aria-label={item.title}>
        {body}
      </PrefetchLink>
    );
  }
  return (
    <button
      className={`${className} text-left`}
      aria-label={playable ? `Play ${item.title}` : item.title}
      onClick={() => (playable ? playTrack(music) : undefined)}
    >
      {body}
    </button>
  );
}

function HomeSkeleton() {
  return (
    <div className="mx-(--body-gutter) mt-7">
      <div className="grid grid-cols-2 gap-5 sm:grid-cols-3 lg:grid-cols-5">
        {Array.from({ length: 5 }).map((_, i) => (
          <SkeletonBlock key={i} width="100%" height={168} radius={10} />
        ))}
      </div>
      <div className="mt-10 flex flex-col gap-6">
        {Array.from({ length: 2 }).map((_, i) => (
          <div key={i}>
            <SkeletonBlock width={190} height={22} radius={6} className="mb-3.5" />
            <SkeletonBlock width="100%" height={190} radius={10} />
          </div>
        ))}
      </div>
    </div>
  );
}
