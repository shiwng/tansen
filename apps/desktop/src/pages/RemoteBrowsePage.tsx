import { useQuery } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { DetailHeader } from '@/components/music/DetailHeader';
import { TrackList } from '@/components/music/TrackList';
import { AlbumSaveButton, ArtistFollowButton } from '@/components/music/FavoriteButton';
import { playList, shuffleList } from '@/components/music/play';
import { sourceFor } from '@/lib/rows';
import {
  discoveryContinuation,
  discoveryToTrack,
  type DiscoveryBrowsePage,
  type DiscoveryTrack,
} from '@/lib/discovery';

type BrowseKind = 'album' | 'artist' | 'playlist';

export function RemoteBrowsePage({ kind, id }: { kind: BrowseKind; id: string }) {
  const query = useQuery({
    queryKey: ['catalogue-browse', kind, id],
    queryFn: () => invoke<DiscoveryBrowsePage>(`discovery_${kind}`, { id }),
    retry: false,
  });
  const [tracks, setTracks] = useState<DiscoveryTrack[]>([]);
  const [continuation, setContinuation] = useState<string | null>(null);
  useEffect(() => {
    setTracks(query.data?.tracks ?? []);
    setContinuation(query.data?.continuation ?? null);
  }, [query.data]);

  if (query.isPending) return <p className="mx-(--body-gutter) pt-11 type-body text-secondary">Loading catalogue page…</p>;
  if (query.isError || !query.data) return <p className="mx-(--body-gutter) pt-11 type-body text-secondary">Catalogue unavailable: {String(query.error)}</p>;

  const rows = tracks.map((row, index) => discoveryToTrack(row, index));
  const source = sourceFor(kind, query.data.title ?? kind, id);
  const title = query.data.title ?? kind[0]!.toUpperCase() + kind.slice(1);
  const subtitle = query.data.subtitle ?? (kind === 'artist' ? 'Artist' : kind[0]!.toUpperCase() + kind.slice(1));
  const loadMore = async () => {
    if (!continuation) return;
    const page = await discoveryContinuation(continuation);
    setTracks((old) => [...old, ...page.tracks]);
    setContinuation(page.continuation);
  };

  return (
    <div>
      <DetailHeader
        info={{
          title,
          subtitle,
          meta: `${rows.length} songs`,
          description: null,
          thumbnail: query.data.thumbnail_url,
          artists: [{ name: kind === 'artist' ? title : subtitle.replace(/^.*•\s*/, ''), id: null }],
          explicit: false,
          library: null,
        }}
        onPlay={() => playList(rows, 0, { source, fallbackArtwork: query.data.thumbnail_url })}
        onShuffle={() => shuffleList(rows, { source, fallbackArtwork: query.data.thumbnail_url })}
        canPlay={rows.length > 0}
        extra={
          kind === 'album' ? (
            <AlbumSaveButton albumKey={id} sourceKind="youtube" />
          ) : kind === 'artist' ? (
            <ArtistFollowButton artistKey={id} name={title} sourceKind="youtube" />
          ) : undefined
        }
      />
      {rows.length > 0 ? <div className="mx-(--body-gutter) mb-10"><TrackList tracks={rows} variant="playlist" fallbackArtwork={query.data.thumbnail_url} source={source} /></div> : <p className="mx-(--body-gutter) pb-10 type-body text-secondary">No playable tracks in this page.</p>}
      {continuation && <div className="mx-(--body-gutter) pb-10"><button className="rounded-pill bg-quinary px-4 py-2 type-callout text-primary" onClick={() => void loadMore()}>Load more</button></div>}
    </div>
  );
}
