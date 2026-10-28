import { useQuery } from '@tanstack/react-query';
import { ShelfView, trackItem } from '@/components/music/ShelfView';
import { SkeletonBlock } from '@/components/Skeleton';
import { q, rowsToTracks } from '@/lib/rows';
import type { MusicItem, Shelf } from '@/data/music/types';

/** Home: recently added albums, then recently added tracks, as square shelves. */
export function HomePage() {
  const tracks = useQuery(q.tracks());
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
  return (
    <div>
      <h1 className="mx-(--body-gutter) pt-11 type-header-emphasized">Home</h1>
      {rows.length === 0 ? (
        <EmptyLibrary />
      ) : (
        <div className="mt-4">
          <ShelfView shelf={albumShelf(rows)} />
          <ShelfView shelf={trackShelf(rows)} />
        </div>
      )}
    </div>
  );
}

/** Albums as square lockups; the route key is the album group key. */
function albumShelf(tracks: ReturnType<typeof rowsToTracks>): Shelf {
  const seen = new Map<string, MusicItem>();
  for (const t of tracks) {
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
    <p className="mt-6 max-w-md pb-10 type-body text-secondary">
      No music yet. Open <strong>Songs</strong> in the sidebar and import a folder — the player indexes it, artwork
      included.
    </p>
  );
}
