import { useQuery } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { DetailHeader } from '@/components/music/DetailHeader';
import { TrackList } from '@/components/music/TrackList';
import { playList, shuffleList } from '@/components/music/play';
import { type Track } from '@/data/music/types';
import { sourceFor } from '@/lib/rows';

type BrowseKind = 'album' | 'artist' | 'playlist';

interface DiscoveryTrack {
  key: { source: string; id: string };
  title: string;
  artist: string;
  album: string | null;
  duration_sec: number | null;
  thumbnail_url: string | null;
  artists: Array<{ name: string; browse_id: string | null }>;
  album_id: string | null;
  is_video: boolean;
  explicit: boolean;
  playable: boolean;
}

interface DiscoveryPage {
  title: string | null;
  subtitle: string | null;
  thumbnail_url: string | null;
  tracks: DiscoveryTrack[];
  suggested: DiscoveryTrack[];
  continuation: string | null;
}

function toTrack(row: DiscoveryTrack, index: number): Track {
  return {
    videoId: row.key.id,
    title: row.title,
    artists: row.artists.length ? row.artists.map((a) => ({ name: a.name, id: a.browse_id })) : [{ name: row.artist || 'Unknown Artist', id: null }],
    album: row.album ? { id: row.album_id, name: row.album } : null,
    durationSec: row.duration_sec,
    explicit: row.explicit,
    thumbnail: row.thumbnail_url,
    index,
    plays: null,
    sourceKind: 'youtube',
    artworkUrl: row.thumbnail_url,
    ...(row.playable ? {} : { unavailable: 'missing' as const }),
  };
}

export function RemoteBrowsePage({ kind, id }: { kind: BrowseKind; id: string }) {
  const query = useQuery({
    queryKey: ['catalogue-browse', kind, id],
    queryFn: () => invoke<DiscoveryPage>(`discovery_${kind}`, { id }),
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

  const rows = tracks.map(toTrack);
  const source = sourceFor(kind, query.data.title ?? kind, id);
  const title = query.data.title ?? kind[0]!.toUpperCase() + kind.slice(1);
  const subtitle = query.data.subtitle ?? (kind === 'artist' ? 'Artist' : kind[0]!.toUpperCase() + kind.slice(1));
  const loadMore = async () => {
    if (!continuation) return;
    const page = await invoke<DiscoveryPage>('discovery_continuation', { token: continuation });
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
      />
      {rows.length > 0 ? <div className="mx-(--body-gutter) mb-10"><TrackList tracks={rows} variant="playlist" fallbackArtwork={query.data.thumbnail_url} source={source} /></div> : <p className="mx-(--body-gutter) pb-10 type-body text-secondary">No playable tracks in this page.</p>}
      {continuation && <div className="mx-(--body-gutter) pb-10"><button className="rounded-pill bg-quinary px-4 py-2 type-callout text-primary" onClick={() => void loadMore()}>Load more</button></div>}
    </div>
  );
}
