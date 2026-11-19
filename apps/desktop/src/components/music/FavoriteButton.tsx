import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { Heart } from 'lucide-react';
import type { Track } from '@/data/music/types';

interface KeyRef {
  key_source: string;
  key_id: string;
}

function trackKey(track: Track | null | undefined): { key_source: string; key_id: string } | null {
  if (!track) return null;
  if (track.videoId.startsWith('local:')) {
    return { key_source: 'local', key_id: track.videoId.slice('local:'.length) };
  }
  return { key_source: track.sourceKind ?? 'youtube', key_id: track.videoId };
}

function isLiked(rows: KeyRef[] | undefined, key: { key_source: string; key_id: string } | null): boolean {
  return !!key && !!rows?.some((row) => row.key_source === key.key_source && row.key_id === key.key_id);
}

function useLikedTracks() {
  return useQuery({
    queryKey: ['likes', 'tracks'],
    queryFn: () => invoke<KeyRef[]>('liked_tracks_get'),
    staleTime: 60_000,
  });
}

/**
 * The player's heart. Likes are explicit feedback for Home recommendations;
 * they persist in the database and never invent counts or social data.
 */
export function FavoriteButton({
  track,
  videoId,
  className,
  size = 18,
}: {
  track?: Track | null;
  /** Back-compat for callers that only know the id; local ids keep their prefix. */
  videoId?: string | null;
  className?: string;
  size?: number;
}) {
  const queryClient = useQueryClient();
  const key = trackKey(
    track ??
      (videoId
        ? { videoId, title: '', artists: [], album: null, durationSec: null, explicit: false, thumbnail: null, index: null, plays: null }
        : null),
  );
  const likes = useLikedTracks();
  const liked = isLiked(likes.data, key);
  const toggle = useMutation({
    mutationFn: async () => {
      if (!key) return;
      await invoke('liked_tracks_set', {
        keySource: key.key_source,
        keyId: key.key_id,
        liked: !liked,
      });
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['likes', 'tracks'] });
      void queryClient.invalidateQueries({ queryKey: ['home'] });
    },
  });
  if (!key) return null;
  return (
    <button
      className={className}
      aria-label={liked ? 'Unlike' : 'Like'}
      aria-pressed={liked}
      disabled={toggle.isPending}
      onClick={() => toggle.mutate()}
    >
      <Heart size={size} strokeWidth={2} fill={liked ? 'currentColor' : 'none'} />
    </button>
  );
}

export function AlbumSaveButton({
  albumKey,
  sourceKind,
}: {
  albumKey: string;
  sourceKind: 'local' | 'youtube';
}) {
  const queryClient = useQueryClient();
  const albums = useQuery({
    queryKey: ['likes', 'albums'],
    queryFn: () => invoke<KeyRef[]>('liked_albums_get'),
    staleTime: 60_000,
  });
  const key = { key_source: sourceKind, key_id: albumKey };
  const liked = isLiked(albums.data, key);
  const toggle = useMutation({
    mutationFn: () =>
      invoke('liked_albums_set', {
        keySource: key.key_source,
        keyId: key.key_id,
        liked: !liked,
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['likes', 'albums'] });
      void queryClient.invalidateQueries({ queryKey: ['home'] });
    },
  });
  return (
    <button
      className="flex items-center gap-1.5 rounded-pill bg-quinary px-4 py-2 type-callout-emphasized text-primary hover:bg-quaternary disabled:opacity-50"
      aria-pressed={liked}
      disabled={toggle.isPending}
      onClick={() => toggle.mutate()}
    >
      <Heart size={14} strokeWidth={2} fill={liked ? 'currentColor' : 'none'} />
      {liked ? 'Saved' : 'Save album'}
    </button>
  );
}

export function ArtistFollowButton({
  artistKey,
  name,
  sourceKind,
}: {
  artistKey: string;
  name: string;
  sourceKind: 'local' | 'youtube';
}) {
  const queryClient = useQueryClient();
  const followed = useQuery({
    queryKey: ['likes', 'artists'],
    queryFn: () => invoke<Array<[KeyRef, string]>>('followed_artists_get'),
    staleTime: 60_000,
  });
  const key = { key_source: sourceKind, key_id: artistKey };
  const isFollowing = !!followed.data?.some(
    ([row]) => row.key_source === key.key_source && row.key_id === key.key_id,
  );
  const toggle = useMutation({
    mutationFn: () =>
      invoke('followed_artist_set', {
        keySource: key.key_source,
        keyId: key.key_id,
        name,
        followed: !isFollowing,
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['likes', 'artists'] });
      void queryClient.invalidateQueries({ queryKey: ['home'] });
    },
  });
  return (
    <button
      className="flex items-center gap-1.5 rounded-pill bg-quinary px-4 py-2 type-callout-emphasized text-primary hover:bg-quaternary disabled:opacity-50"
      aria-pressed={isFollowing}
      disabled={toggle.isPending}
      onClick={() => toggle.mutate()}
    >
      <Heart size={14} strokeWidth={2} fill={isFollowing ? 'currentColor' : 'none'} />
      {isFollowing ? 'Following' : 'Follow'}
    </button>
  );
}
