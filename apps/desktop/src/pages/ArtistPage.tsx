import { useQuery } from '@tanstack/react-query';
import { useParams } from 'react-router';
import { DetailHeader } from '@/components/music/DetailHeader';
import { TrackList } from '@/components/music/TrackList';
import { shuffleList } from '@/components/music/play';
import { usePlayer } from '@/state/player';
import { q, rowsToTracks, sourceFor } from '@/lib/rows';
import { RemoteBrowsePage } from './RemoteBrowsePage';

/** An artist: everything credited to the name (album_artist or track artist). */
export function ArtistPage() {
  const { id } = useParams<{ id: string }>();
  const name = decodeURIComponent(id ?? '');
  if (name.startsWith('UC')) return <RemoteBrowsePage kind="artist" id={name} />;
  return <LocalArtistPage artistName={name} />;
}

function LocalArtistPage({ artistName: name }: { artistName: string }) {
  const tracks = useQuery(q.artistTracks(name));
  const playTracks = usePlayer((s) => s.playTracks);

  if (tracks.isError) {
    return <p className="mx-(--body-gutter) pt-11 type-body text-secondary">{String(tracks.error)}</p>;
  }
  if (tracks.isPending) return null;

  const rows = rowsToTracks(tracks.data);
  const source = sourceFor('artist', name, name);
  const minutes = Math.round(rows.reduce((sum, t) => sum + (t.durationSec ?? 0), 0) / 60);

  return (
    <div>
      <DetailHeader
        info={{
          title: name,
          subtitle: 'Artist',
          meta: `${tracks.data.length} songs, ${minutes} minute${minutes === 1 ? '' : 's'}`,
          description: null,
          thumbnail: null,
          artists: [{ name, id: null }],
          explicit: false,
          library: null,
        }}
        onPlay={() => playTracks(rows, 0, source)}
        onShuffle={() => shuffleList(rows, { source })}
        canPlay={rows.length > 0}
      />
      {rows.length > 0 && (
        <div className="mx-(--body-gutter) mb-10">
          <TrackList tracks={rows} variant="playlist" source={source} />
        </div>
      )}
    </div>
  );
}
