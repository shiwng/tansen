import { useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { ShelfView, trackItem } from '@/components/music/ShelfView';
import { SkeletonBlock } from '@/components/Skeleton';
import { q, rowsToTracks, sourceFor } from '@/lib/rows';
import { shuffleList } from '@/components/music/play';
import type { MusicItem, Shelf, Track } from '@/data/music/types';

/** Home: a Start Listening hero over the recently-added shelves. */
export function HomePage() {
  const tracks = useQuery(q.tracks());
  const catalogue = useQuery({
    queryKey: ['catalogue-home'],
    queryFn: () => invoke<DiscoveryHome>('discovery_home'),
    retry: false,
    staleTime: 10 * 60_000,
  });
  const explore = useQuery({
    queryKey: ['catalogue-explore', 'explore'],
    queryFn: () => invoke<DiscoveryHome>('discovery_explore', { section: 'explore' }),
    retry: false,
    staleTime: 10 * 60_000,
  });
  const newReleases = useQuery({
    queryKey: ['catalogue-explore', 'new-releases'],
    queryFn: () => invoke<DiscoveryHome>('discovery_explore', { section: 'new-releases' }),
    retry: false,
    staleTime: 10 * 60_000,
  });
  if (tracks.isError) {
    return <p className="mx-(--body-gutter) pt-11 type-body text-secondary">{String(tracks.error)}</p>;
  }
  if (tracks.isPending) {
    return (
      <div className="mx-(--body-gutter) pt-11">
        <SkeletonBlock width={220} height={34} radius={8} className="mb-6" />
        <SkeletonBlock width="100%" height={220} radius={10} />
      </div>
    );
  }
  const rows = rowsToTracks(tracks.data);
  const catalogueShelves = [
    ...(catalogue.data?.shelves ?? []),
    ...(explore.data?.shelves ?? []),
    ...(newReleases.data?.shelves ?? []),
  ];
  return (
    <div>
      <h1 className="mx-(--body-gutter) pt-11 type-header-emphasized">Home</h1>
      {rows.length === 0 && catalogueShelves.length === 0 ? (
        <EmptyLibrary />
      ) : (
        <div className="mt-4">
          {rows.length > 0 && <HeroBanner tracks={rows} />}
          {rows.length > 0 && <ShelfView shelf={albumShelf(rows)} />}
          {rows.length > 0 && <ShelfView shelf={trackShelf(rows)} />}
          {catalogueShelves.map((shelf) => <ShelfView key={shelf.title} shelf={homeShelf(shelf)} />)}
        </div>
      )}
    </div>
  );
}

interface DiscoveryHomeItem { title: string; subtitle: string; thumbnail_url: string | null; video_id: string | null; browse_id: string | null; is_video: boolean }
interface DiscoveryHomeShelf { title: string; subtitle: string; layout: string; more_browse_id: string | null; more_params: string | null; items: DiscoveryHomeItem[] }
interface DiscoveryHome { shelves: DiscoveryHomeShelf[]; continuation: string | null }

function homeShelf(shelf: DiscoveryHomeShelf): Shelf {
  return {
    title: shelf.title,
    strapline: shelf.subtitle || null,
    layout: shelf.layout === 'video' ? 'video' : shelf.layout === 'list' ? 'tracks' : 'square',
    seeAll: shelf.more_browse_id ? { browseId: shelf.more_browse_id, params: shelf.more_params } : null,
    items: shelf.items.map((item) => ({
      kind: item.video_id ? (item.is_video ? 'video' : 'song') : item.browse_id?.startsWith('UC') ? 'artist' : item.browse_id?.startsWith('VL') ? 'playlist' : 'album',
      id: item.video_id ?? item.browse_id ?? item.title,
      title: item.title,
      subtitle: item.subtitle,
      artists: [{ name: item.subtitle || 'Unknown Artist', id: null }],
      album: null,
      thumbnail: item.thumbnail_url,
      wide: item.is_video,
      durationSec: null,
      explicit: false,
    })),
  };
}

/** The banner card: real library stats and a one-tap start into the queue. */
function HeroBanner({ tracks }: { tracks: Track[] }) {
  const albums = new Set(tracks.map((t) => t.album?.name).filter(Boolean)).size;
  const artists = new Set(tracks.map((t) => t.artists[0]?.name).filter(Boolean)).size;
  return (
    <section
      aria-label="Start Listening"
      className="mx-(--body-gutter) mb-8 flex flex-wrap items-center gap-x-6 gap-y-4 rounded-[14px] px-7 py-6"
      style={{ background: 'linear-gradient(120deg, var(--systemRed), color-mix(in srgb, var(--systemRed) 45%, black))' }}
    >
      <span className="type-header-emphasized text-white">Tansen</span>
      <span className="hidden h-12 w-px bg-white/30 sm:block" />
      <div className="min-w-0 flex-1">
        <p className="type-title-3-emphasized text-white">Your library, ready to play.</p>
        <p className="type-callout text-white/75">
          {tracks.length} songs · {albums} albums · {artists} artists
        </p>
      </div>
      <button
        className="rounded-pill bg-white px-5 py-2.5 type-body-emphasized text-black transition-colors hover:bg-white/85"
        onClick={() => shuffleList(tracks, { source: sourceFor('home', 'Home') })}
      >
        Start Listening
      </button>
    </section>
  );
}

/** Albums as square lockups (newest first — tracks_get returns insertion order). */
function albumShelf(tracks: ReturnType<typeof rowsToTracks>): Shelf {
  const seen = new Map<string, MusicItem>();
  for (const t of [...tracks].reverse()) {
    const name = t.album?.name;
    if (!name || seen.has(name)) continue;
    const artist = t.artists[0]?.name ?? 'Unknown Artist';
    seen.set(name, {
      kind: 'album',
      id: `${artist}||${name}`,
      title: name,
      subtitle: `Album • ${artist}`,
      artists: t.artists,
      album: null,
      thumbnail: t.thumbnail,
      wide: false,
      durationSec: null,
      explicit: false,
    });
  }
  return { title: 'Recently Added Albums', strapline: null, layout: 'square', items: [...seen.values()].slice(0, 24), seeAll: null };
}

function trackShelf(tracks: ReturnType<typeof rowsToTracks>): Shelf {
  const items = [...tracks].reverse().slice(0, 24).map(trackItem);
  return { title: 'Recently Added Songs', strapline: null, layout: 'tracks', items, seeAll: null };
}

export function EmptyLibrary() {
  return (
    <div className="mt-6 max-w-md pb-10">
      <p className="type-body text-secondary">
        Millions of songs are one search away — press <strong>Search</strong> and type anything. Results come from
        YouTube Music and play immediately.
      </p>
      <p className="mt-2 type-body text-secondary">
        For your own files, open <strong>Settings → Library</strong> and add a folder — the player indexes it, artwork
        included.
      </p>
    </div>
  );
}
