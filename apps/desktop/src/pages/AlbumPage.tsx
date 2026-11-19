import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { useParams } from 'react-router';
import { DetailHeader } from '@/components/music/DetailHeader';
import { TrackList } from '@/components/music/TrackList';
import { AlbumSaveButton } from '@/components/music/FavoriteButton';
import { shuffleList } from '@/components/music/play';
import { usePlayer } from '@/state/player';
import { artUrl, q, rowsToTracks, sourceFor } from '@/lib/rows';
import { addDownloads, describeOutcome, requestsFromRefs } from '@/lib/downloads';
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
  const [downloadStatus, setDownloadStatus] = useState<string | null>(null);

  async function downloadAlbum() {
    if (!tracks.data?.length) return;
    try {
      const outcome = await addDownloads(requestsFromRefs(tracks.data, 'album'));
      setDownloadStatus(describeOutcome(outcome));
    } catch (error) {
      setDownloadStatus(String(error).replace(/^Error:\s*/, ''));
    }
  }

  // Hooks first: the early returns below would otherwise change the hook
  // count when the queries land (React #310).
  const info = album.data?.find((a) => a.key === key);
  // Artwork-driven page tint, as the original's album pages.
  const tint = useArtworkColor(artUrl(info?.art_path));
  usePageTint(tint);

  if (album.isError || tracks.isError) {
    return <p className="mx-(--body-gutter) pt-11 type-body text-secondary">{String(album.error ?? tracks.error)}</p>;
  }
  if (tracks.isPending || album.isPending) return null;
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
        extra={
          <div className="flex items-center gap-2">
            <AlbumSaveButton albumKey={key} sourceKind="local" />
            <button
              className="rounded-pill bg-quinary px-4 py-2 type-callout-emphasized text-primary hover:bg-quaternary"
              onClick={() => void downloadAlbum()}
              disabled={tracks.data.length === 0}
            >
              Download album
            </button>
          </div>
        }
      />
      {downloadStatus && (
        <p className="mx-(--body-gutter) mb-4 type-callout text-secondary">{downloadStatus}</p>
      )}
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
