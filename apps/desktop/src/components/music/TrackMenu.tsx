import { MoreHorizontal, Plus, Radio } from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import type { PlaylistRow } from '@/types';
import { Menu, MenuItem, MenuSeparator, MenuSubmenu, type MenuPlacement } from '@/components/Menu';
import type { Track } from '@/data/music/types';
import { usePlayer } from '@/state/player';
import { TrackInfoDialog } from './TrackInfoDialog';

interface KeyRef {
  key_source: string;
  key_id: string;
}

/**
 * The ⋯ menu on every track row, after Music's compact contextual menu:
 * playlist and station actions first, then metadata and feedback, then the
 * queue and download actions. Every row invokes its real command; unsupported
 * rows (a station for a local file, a download for a stream) are not shown.
 */
export function TrackMenu({
  track,
  className,
  placement,
  onOpenChange,
}: {
  track: Track;
  className?: string;
  /** Where the menu opens from the button; the player's sit at the bottom of the window. */
  placement?: MenuPlacement;
  /** Told when the menu opens and closes; most mounts don't need to know. */
  onOpenChange?: (open: boolean) => void;
}) {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const [infoOpen, setInfoOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setAnchor(null), []);

  useEffect(() => {
    onOpenChange?.(anchor !== null);
  }, [anchor, onOpenChange]);

  return (
    <>
      <button
        ref={trigger}
        type="button"
        className={className}
        aria-label={`More options for ${track.title}`}
        aria-haspopup="menu"
        aria-expanded={anchor !== null}
        data-open={anchor !== null || undefined}
        onClick={(event) => {
          event.stopPropagation();
          setAnchor(anchor ? null : event.currentTarget);
        }}
        onDoubleClick={(event) => event.stopPropagation()}
      >
        <MoreHorizontal size={18} strokeWidth={2} />
      </button>
      {anchor && (
        <Menu
          anchor={anchor}
          placement={placement}
          onClose={close}
          width={213}
          label={`More options for ${track.title}`}
        >
          <MainMenu
            track={track}
            onClose={close}
            onInfo={() => {
              close();
              setInfoOpen(true);
            }}
          />
        </Menu>
      )}
      {infoOpen && (
        <TrackInfoDialog
          track={track}
          onClose={() => {
            setInfoOpen(false);
            trigger.current?.focus({ preventScroll: true });
          }}
        />
      )}
    </>
  );
}

function trackKey(track: Track): KeyRef {
  if (track.videoId.startsWith('local:')) {
    return { key_source: 'local', key_id: track.videoId.slice('local:'.length) };
  }
  return { key_source: track.sourceKind ?? 'youtube', key_id: track.videoId };
}

function sourceOf(track: Track): string {
  return track.videoId.startsWith('local:') ? 'local' : (track.sourceKind ?? 'youtube');
}

function MainMenu({ track, onClose, onInfo }: { track: Track; onClose: () => void; onInfo: () => void }) {
  const queryClient = useQueryClient();
  const playNext = usePlayer((s) => s.playNext);
  const addToQueue = usePlayer((s) => s.addToQueue);
  const queue = usePlayer((s) => s.queue);
  const key = trackKey(track);
  const sourceKind = sourceOf(track);
  const canAddToPlaylist = sourceKind === 'local' || sourceKind === 'youtube' || sourceKind === 'spotify';
  const canStation = sourceKind === 'youtube';
  const downloadable = sourceKind === 'local' || sourceKind === 'subsonic' || sourceKind === 'addon';

  const likes = useQuery({
    queryKey: ['likes', 'tracks'],
    queryFn: () => invoke<KeyRef[]>('liked_tracks_get'),
    staleTime: 60_000,
  });
  const dismissed = useQuery({
    queryKey: ['recommendations', 'dismissed'],
    queryFn: () => invoke<KeyRef[]>('recommendation_dismissed_get'),
    staleTime: 60_000,
  });
  const isKey = (row: KeyRef) => row.key_source === key.key_source && row.key_id === key.key_id;
  const liked = !!likes.data?.some(isKey);
  const suggestLess = !!dismissed.data?.some(isKey);

  const toggleLike = useMutation({
    mutationFn: () => invoke('liked_tracks_set', { keySource: key.key_source, keyId: key.key_id, liked: !liked }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['likes', 'tracks'] });
      void queryClient.invalidateQueries({ queryKey: ['home'] });
      onClose();
    },
  });
  const toggleDismiss = useMutation({
    mutationFn: () =>
      invoke('recommendation_dismiss', {
        keySource: key.key_source,
        keyId: key.key_id,
        dismissed: !suggestLess,
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['recommendations', 'dismissed'] });
      void queryClient.invalidateQueries({ queryKey: ['home'] });
      onClose();
    },
  });

  const [radioBusy, setRadioBusy] = useState(false);
  const [radioError, setRadioError] = useState<string | null>(null);
  const startRadio = async () => {
    setRadioBusy(true);
    setRadioError(null);
    try {
      const rows = await invoke<DiscoveryTrack[]>('discovery_radio', { videoId: track.videoId });
      const queued = new Set(queue.map((entry) => entry.track.videoId));
      const radio = rows
        .filter((row) => row.key.id !== track.videoId && !queued.has(row.key.id) && row.playable)
        .map(discoveryToTrack);
      if (!radio.length) {
        setRadioError('No related tracks were available.');
        return;
      }
      await addToQueue(radio, { title: `Radio · ${track.title}`, type: 'radio' });
      onClose();
    } catch (error) {
      setRadioError(`Station unavailable: ${String(error).replace(/^Error:\s*/, '')}`);
    } finally {
      setRadioBusy(false);
    }
  };

  const [downloadBusy, setDownloadBusy] = useState(false);
  const [downloadError, setDownloadError] = useState<string | null>(null);
  const startDownload = async () => {
    setDownloadBusy(true);
    setDownloadError(null);
    try {
      const outcome = await invoke<{ added: number; duplicates: number; rejected: string[] }>('downloads_add', {
        tracks: [
          {
            kind: 'track',
            source: sourceKind,
            trackId: track.videoId,
            title: track.title,
            artist: track.artists[0]?.name ?? '',
            album: track.album?.name ?? null,
            durationSec: track.durationSec ?? 0,
            artworkUrl: track.artworkUrl ?? null,
            quality: 'high',
          },
        ],
      });
      if (outcome.rejected.length) {
        setDownloadError(outcome.rejected.join('; '));
      } else {
        onClose();
      }
    } catch (error) {
      setDownloadError(String(error).replace(/^Error:\s*/, ''));
    } finally {
      setDownloadBusy(false);
    }
  };

  return (
    <>
      {canAddToPlaylist && (
        <MenuSubmenu label="Add to Playlist" menuLabel="Add to Playlist">
          <PlaylistChoices track={track} sourceKind={sourceKind} onDone={onClose} />
        </MenuSubmenu>
      )}
      {canStation && (
        <MenuItem icon={<Radio size={15} />} disabled={radioBusy} onSelect={() => void startRadio()}>
          {radioBusy ? 'Creating Station…' : 'Create Station'}
        </MenuItem>
      )}
      {radioError && <p className="px-2 pb-1 type-caption text-secondary">{radioError}</p>}
      {(canAddToPlaylist || canStation) && <MenuSeparator />}
      <MenuItem onSelect={onInfo}>Get Info</MenuItem>
      <MenuItem disabled={toggleLike.isPending} onSelect={() => toggleLike.mutate()}>
        {liked ? 'Unlike' : 'Like'}
      </MenuItem>
      <MenuItem disabled={toggleDismiss.isPending} onSelect={() => toggleDismiss.mutate()}>
        {suggestLess ? 'Undo Suggest Less' : 'Not Interested in Home'}
      </MenuItem>
      <MenuSeparator />
      <MenuItem onSelect={() => (onClose(), playNext([track]))}>Play Next</MenuItem>
      <MenuItem onSelect={() => (onClose(), addToQueue([track]))}>Add to Queue</MenuItem>
      {downloadable && (
        <MenuItem disabled={downloadBusy} onSelect={() => void startDownload()}>
          {downloadBusy ? 'Adding Download…' : 'Download'}
        </MenuItem>
      )}
      {downloadError && <p className="px-2 pb-1 type-caption text-secondary">{downloadError}</p>}
    </>
  );
}

/** The playlist submenu: existing playlists, or a new one made on the spot. */
function PlaylistChoices({
  track,
  sourceKind,
  onDone,
}: {
  track: Track;
  sourceKind: string;
  onDone: () => void;
}) {
  const client = useQueryClient();
  const playlists = useQuery({
    queryKey: ['local', 'playlists'],
    queryFn: () => invoke<PlaylistRow[]>('playlists_get'),
  });
  const add = useMutation({
    mutationFn: (playlistId: number) => addToPlaylist(track, sourceKind, playlistId),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ['local'] });
      onDone();
    },
  });
  const createAndAdd = useMutation({
    mutationFn: async (name: string) => {
      const id = await invoke<number>('playlist_create', { name });
      await addToPlaylist(track, sourceKind, id);
    },
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ['local'] });
      onDone();
    },
  });
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState('');
  const error = add.error ?? createAndAdd.error;

  return (
    <>
      {playlists.isPending && (
        <MenuItem disabled onSelect={() => undefined}>
          Loading…
        </MenuItem>
      )}
      {playlists.data?.length === 0 && (
        <MenuItem disabled onSelect={() => undefined}>
          No playlists yet
        </MenuItem>
      )}
      {playlists.data?.map((p) => (
        <MenuItem key={p.id} disabled={add.isPending} onSelect={() => add.mutate(p.id)}>
          {p.name}
        </MenuItem>
      ))}
      {error && <p className="px-2 pb-1 type-caption text-secondary">{String(error).replace(/^Error:\s*/, '')}</p>}
      <MenuSeparator />
      {creating ? (
        <form
          className="flex items-center gap-1 px-1 py-0.5"
          onSubmit={(event) => {
            event.preventDefault();
            if (name.trim()) createAndAdd.mutate(name.trim());
          }}
        >
          <input
            autoFocus
            value={name}
            onChange={(event) => setName(event.currentTarget.value)}
            placeholder="Playlist name"
            aria-label="Playlist name"
            className="min-w-0 flex-1 rounded-[5px] border-0 bg-quinary px-2 py-1 type-body text-primary outline-none"
          />
          <button
            type="submit"
            disabled={!name.trim() || createAndAdd.isPending}
            className="rounded-[5px] bg-key px-2 py-1 type-caption text-white disabled:opacity-50"
          >
            Add
          </button>
        </form>
      ) : (
        <MenuItem icon={<Plus size={15} />} disabled={createAndAdd.isPending} onSelect={() => setCreating(true)}>
          New Playlist…
        </MenuItem>
      )}
    </>
  );
}

/** Local rows use the numeric library id; catalogue rows use their stored key. */
async function addToPlaylist(track: Track, sourceKind: string, playlistId: number) {
  if (sourceKind === 'local') {
    const id = Number(track.videoId.slice('local:'.length));
    if (!Number.isFinite(id)) throw new Error(`Unsupported local track id: ${track.videoId}`);
    await invoke('playlist_add_tracks', { id: playlistId, ids: [id] });
    return;
  }
  await invoke('playlist_add_remote_tracks', {
    id: playlistId,
    tracks: [
      {
        key_source: sourceKind,
        key_id: track.videoId,
        title: track.title,
        artist: track.artists.map((artist) => artist.name).join(', ') || 'Unknown Artist',
        album: track.album?.name ?? null,
        duration_sec: track.durationSec ?? 0,
        artwork_url: track.artworkUrl?.startsWith('https://') ? track.artworkUrl : null,
      },
    ],
  });
}

interface DiscoveryTrack {
  key: { source: string; id: string };
  title: string;
  artist: string;
  album: string | null;
  duration_sec: number | null;
  thumbnail_url: string | null;
  artists: Array<{ name: string; browse_id: string | null }>;
  album_id: string | null;
  explicit: boolean;
  playable: boolean;
}

function discoveryToTrack(row: DiscoveryTrack): Track {
  return {
    videoId: row.key.id,
    title: row.title,
    artists: row.artists.length
      ? row.artists.map((artist) => ({ name: artist.name, id: artist.browse_id }))
      : [{ name: row.artist || 'Unknown Artist', id: null }],
    album: row.album ? { id: row.album_id, name: row.album } : null,
    durationSec: row.duration_sec,
    explicit: row.explicit,
    thumbnail: row.thumbnail_url,
    index: null,
    plays: null,
    sourceKind: row.key.source,
    artworkUrl: row.thumbnail_url,
  };
}
