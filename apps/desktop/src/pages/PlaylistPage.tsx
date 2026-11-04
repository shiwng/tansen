import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { useNavigate } from 'react-router';
import { useParams } from 'react-router';
import { invoke } from '@tauri-apps/api/core';
import { DetailHeader } from '@/components/music/DetailHeader';
import { TrackList } from '@/components/music/TrackList';
import { shuffleList } from '@/components/music/play';
import { trackFromRef, usePlayer } from '@/state/player';
import type { TrackRef } from '@/types';
import { artUrl, sourceFor } from '@/lib/rows';
import { useArtworkColor } from '@/lib/artworkColor';
import { usePageTint } from '@/lib/usePageTint';
import { RemoteBrowsePage } from './RemoteBrowsePage';

/** A user playlist: an ordered list of library tracks. */
export function PlaylistPage() {
  const { id } = useParams<{ id: string }>();
  const rawId = decodeURIComponent(id ?? '');
  if (rawId.startsWith('VL') || rawId.startsWith('PL') || rawId.startsWith('OLAK5uy')) {
    return <RemoteBrowsePage kind="playlist" id={rawId} />;
  }
  return <LocalPlaylistPage playlistId={Number(rawId)} />;
}

function LocalPlaylistPage({ playlistId }: { playlistId: number }) {
  const navigate = useNavigate();
  const client = useQueryClient();
  const playTracks = usePlayer((s) => s.playTracks);
  const [renaming, setRenaming] = useState(false);
  const [name, setName] = useState('');

  const meta = useQuery({ queryKey: ['local', 'playlists'], queryFn: () => invoke<[number, string][]>('playlists_get') });
  const tracks = useQuery({
    queryKey: ['local', 'playlist-tracks', playlistId],
    queryFn: () => invoke<TrackRef[]>('playlist_tracks_get', { id: playlistId }),
  });

  const rename = useMutation({
    mutationFn: (newName: string) => invoke('playlist_rename', { id: playlistId, name: newName }),
    onSuccess: () => void client.invalidateQueries({ queryKey: ['local', 'playlists'] }),
  });
  const remove = useMutation({
    mutationFn: () => invoke('playlist_delete', { id: playlistId }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ['local', 'playlists'] });
      window.history.back();
    },
  });

  if (meta.isPending || tracks.isPending) return null;
  if (meta.isError || tracks.isError) {
    return <p className="mx-(--body-gutter) pt-11 type-body text-secondary">{String(meta.error ?? tracks.error)}</p>;
  }

  const title = meta.data.find(([pid]) => pid === playlistId)?.[1] ?? 'Playlist';
  const rows = tracks.data.map(trackFromRef);
  // Tint from the first track's artwork, like an upstream playlist page.
  const tint = useArtworkColor(artUrl(tracks.data.find((t) => t.art_path)?.art_path));
  usePageTint(tint);
  const source = sourceFor('playlist', title, String(playlistId));
  const minutes = Math.round(tracks.data.reduce((sum, t) => sum + t.duration_sec, 0) / 60);

  return (
    <div>
      <DetailHeader
        info={{
          title,
          subtitle: 'Playlist',
          meta: `${tracks.data.length} songs, ${minutes} minute${minutes === 1 ? '' : 's'}`,
          description: null,
          thumbnail: rows[0]?.thumbnail ?? null,
          artists: [],
          explicit: false,
          library: null,
        }}
        onPlay={() => playTracks(rows, 0, source)}
        onShuffle={() => shuffleList(rows, { source, fallbackArtwork: rows[0]?.thumbnail ?? null })}
        canPlay={rows.length > 0}
        extra={
          <>
            <button
              className="rounded-pill bg-quinary px-4 py-2 type-callout-emphasized text-primary hover:bg-quaternary"
              onClick={() => navigate(`/playlist/${playlistId}/reorder`)}
            >
              Reorder
            </button>
            <button
              className="rounded-pill bg-quinary px-4 py-2 type-callout-emphasized text-primary hover:bg-quaternary"
              onClick={() => {
                setName(title);
                setRenaming(true);
              }}
            >
              Rename
            </button>
            <button
              className="rounded-pill bg-quinary px-4 py-2 type-callout-emphasized text-primary hover:bg-quaternary"
              onClick={() => {
                if (window.confirm(`Delete “${title}”? The songs stay in your library.`)) remove.mutate();
              }}
            >
              Delete
            </button>
          </>
        }
      />
      {renaming && (
        <form
          className="mx-(--body-gutter) mb-6 flex max-w-md items-center gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim()) rename.mutate(name.trim());
            setRenaming(false);
          }}
        >
          <input
            autoFocus
            value={name}
            onChange={(e) => setName(e.currentTarget.value)}
            className="min-w-0 flex-1 rounded-[10px] border-0 bg-quinary px-3 py-2 type-body text-primary outline-none"
            aria-label="Playlist name"
          />
          <button type="submit" className="rounded-pill bg-key px-4 py-2 type-callout-emphasized text-white">
            Save
          </button>
          <button type="button" className="type-callout-emphasized text-secondary" onClick={() => setRenaming(false)}>
            Cancel
          </button>
        </form>
      )}
      {rows.length > 0 ? (
        <div className="mx-(--body-gutter) mb-10">
          <TrackList tracks={rows} variant="playlist" source={source} />
        </div>
      ) : (
        <p className="mx-(--body-gutter) pb-10 type-body text-secondary">
          Empty playlist. Add songs from any track's ⋯ menu → Add to Playlist.
        </p>
      )}
    </div>
  );
}
