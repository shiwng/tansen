import { useQuery } from '@tanstack/react-query';
import { useParams } from 'react-router';
import { DetailHeader } from '@/components/music/DetailHeader';
import { TrackList } from '@/components/music/TrackList';
import { shuffleList } from '@/components/music/play';
import { usePlayer } from '@/state/player';
import { artUrl, q, rowsToTracks, sourceFor } from '@/lib/rows';
import { useArtworkColor } from '@/lib/artworkColor';
import { usePageTint } from '@/lib/usePageTint';
import { RemoteBrowsePage } from './RemoteBrowsePage';

/** An album: every track sharing album_artist + album, in disc/track order. */
export function AlbumPage() {
  const { id } = useParams<{ id: string }>();
  const key = decodeURIComponent(id ?? '');
  if (key.startsWith('MPREb')) return <RemoteBrowsePage kind="album" id={key} />;
  return <LocalAlbumPage albumKey={key} />;
}

function LocalAlbumPage({ albumKey: key }: { albumKey: string }) {
  const album = useQuery(q.albums());
  const tracks = useQuery(q.albumTracks(key));
  const playTracks = usePlayer((s) => s.playTracks);

  if (album.isError || tracks.isError) {
    return <p className="mx-(--body-gutter) pt-11 type-body text-secondary">{String(album.error ?? tracks.error)}</p>;
  }
  if (tracks.isPending || album.isPending) return null;

  const info = album.data.find((a) => a.key === key);
  // Artwork-driven page tint, as the original's album pages.
  const tint = useArtworkColor(artUrl(info?.art_path));
  usePageTint(tint);
  const rows = rowsToTracks(tracks.data);
  const source = sourceFor('album', info?.album ?? 'Album', key);
  const minutes = Math.round(rows.reduce((sum, t) => sum + (t.durationSec ?? 0), 0) / 60);

  return (
    <div>
      <DetailHeader
        info={{
          title: info?.album ?? 'Album',
          subtitle: `Album • ${info?.album_artist ?? ''}`,
          meta: `${tracks.data.length} songs, ${minutes} minute${minutes === 1 ? '' : 's'}`,
          description: null,
          thumbnail: info?.art_path ?? null,
          artists: [{ name: info?.album_artist || 'Unknown Artist', id: null }],
          explicit: false,
          library: null,
        }}
        onPlay={() => playTracks(rows, 0, source)}
        onShuffle={() => shuffleList(rows, { source, fallbackArtwork: info?.art_path ?? null, fallbackArtists: rows[0]?.artists })}
        canPlay={rows.length > 0}
      />
      {rows.length > 0 ? (
        <div className="mx-(--body-gutter) mb-10">
          <TrackList
            tracks={rows}
            variant="album"
            fallbackArtwork={info?.art_path ?? null}
            fallbackArtists={rows[0]?.artists}
            source={source}
          />
        </div>
      ) : (
        <p className="mx-(--body-gutter) pb-10 type-body text-secondary">This album's files are missing from disk.</p>
      )}
    </div>
  );
}
