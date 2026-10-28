import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { useNavigate, useParams } from 'react-router';
import { invoke } from '@tauri-apps/api/core';
import { Artwork } from '@/components/music/Artwork';
import { trackFromRef } from '@/state/player';
import { reorderMove } from '@/lib/reorder';
import { artUrl } from '@/lib/rows';
import type { TrackRef } from '@/types';

/**
 * The playlist reorder page, as the original's ReorderPlaylistPage: one
 * scrollable list, drag a row onto another to move it, Save commits. Order is
 * kept client-side until Save — Cancel is always free.
 */
export function ReorderPlaylistPage() {
  const { id } = useParams<{ id: string }>();
  const playlistId = Number(id);
  const navigate = useNavigate();
  const client = useQueryClient();
  const [order, setOrder] = useState<TrackRef[] | null>(null);
  const [dragFrom, setDragFrom] = useState<number | null>(null);
  const [dragOver, setDragOver] = useState<number | null>(null);

  const meta = useQuery({ queryKey: ['local', 'playlists'], queryFn: () => invoke<[number, string][]>('playlists_get') });
  const tracks = useQuery({
    queryKey: ['local', 'playlist-tracks', playlistId],
    queryFn: () => invoke<TrackRef[]>('playlist_tracks_get', { id: playlistId }),
  });

  const save = useMutation({
    mutationFn: (ids: number[]) => invoke('playlist_tracks_set', { id: playlistId, ids }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ['local', 'playlist-tracks', playlistId] });
      navigate(`/playlist/${playlistId}`);
    },
  });

  if (meta.isPending || tracks.isPending) return null;
  if (meta.isError || tracks.isError) {
    return <p className="mx-(--body-gutter) pt-11 type-body text-secondary">{String(meta.error ?? tracks.error)}</p>;
  }

  const title = meta.data.find(([pid]) => pid === playlistId)?.[1] ?? 'Playlist';
  const rows = order ?? tracks.data;

  return (
    <div className="mx-(--body-gutter) pt-11 pb-16">
      <div className="mb-5 flex items-center gap-3">
        <h1 className="type-header-emphasized">Reorder — {title}</h1>
        <div className="ml-auto flex items-center gap-2">
          <button
            className="rounded-pill bg-quinary px-4 py-2 type-callout-emphasized text-primary hover:bg-quaternary"
            onClick={() => navigate(`/playlist/${playlistId}`)}
          >
            Cancel
          </button>
          <button
            className="rounded-pill bg-key px-4 py-2 type-callout-emphasized text-white hover:opacity-90 disabled:opacity-50"
            disabled={!order}
            onClick={() => save.mutate(rows.map((t) => t.id))}
          >
            Save
          </button>
        </div>
      </div>
      <p className="mb-4 type-callout text-secondary">Drag a row onto another to move it.</p>
      <ol className="max-w-2xl">
        {rows.map((t, i) => (
          <li
            key={`${t.id}-${i}`}
            draggable
            onDragStart={() => setDragFrom(i)}
            onDragOver={(e) => {
              e.preventDefault();
              setDragOver(i);
            }}
            onDragLeave={() => setDragOver((cur) => (cur === i ? null : cur))}
            onDrop={(e) => {
              e.preventDefault();
              if (dragFrom !== null) setOrder(reorderMove(rows, dragFrom, i));
              setDragFrom(null);
              setDragOver(null);
            }}
            onDragEnd={() => {
              setDragFrom(null);
              setDragOver(null);
            }}
            className={`flex cursor-grab items-center gap-3 rounded-[8px] px-2 py-1.5 active:cursor-grabbing ${
              dragOver === i && dragFrom !== null && dragFrom !== i ? 'bg-quaternary' : ''
            } ${dragFrom === i ? 'opacity-40' : ''}`}
          >
            <Artwork src={artUrl(t.art_path)} className="h-9 w-9 rounded" displayWidth={36} />
            <div className="min-w-0 flex-1">
              <div className="truncate type-body">{trackFromRef(t).title}</div>
              <div className="truncate type-callout text-secondary">{t.artist || 'Unknown Artist'}</div>
            </div>
            <span className="type-callout tabular-nums text-secondary">{i + 1}</span>
          </li>
        ))}
      </ol>
    </div>
  );
}
