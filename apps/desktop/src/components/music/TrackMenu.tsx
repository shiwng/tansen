import { ChevronLeft, ChevronRight, ListPlus, ListStart, MoreHorizontal, Plus, Radio } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { Menu, MenuHeader, MenuItem, MenuSeparator, type MenuPlacement } from '@/components/Menu';
import type { Track } from '@/data/music/types';
import { usePlayer } from '@/state/player';

/**
 * The ⋯ button at the end of a track row (report §14): Play Next and Add to
 * Queue for everyone, Add to Playlist via the picker view — upstream's
 * TrackMenu shape with the account-coupled items (favorite) removed until
 * that feature exists on desktop.
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
  const [view, setView] = useState<'main' | 'playlists'>('main');
  const [radioBusy, setRadioBusy] = useState(false);
  const [radioError, setRadioError] = useState<string | null>(null);
  const close = useCallback(() => {
    setAnchor(null);
    setView('main');
  }, []);
  useEffect(() => {
    onOpenChange?.(anchor !== null);
  }, [anchor, onOpenChange]);

  const playNext = usePlayer((s) => s.playNext);
  const addToQueue = usePlayer((s) => s.addToQueue);
  const queue = usePlayer((s) => s.queue);
  const isLocal = track.videoId.startsWith('local:');

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
      close();
    } catch (error) {
      setRadioError(`Radio unavailable: ${String(error)}`);
    } finally {
      setRadioBusy(false);
    }
  };

  return (
    <>
      <button
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
        <Menu anchor={anchor} placement={placement} onClose={close} label={track.title}>
          {view === 'main' ? (
            <MainMenu
              track={track}
              isLocal={isLocal}
              onClose={close}
              onPlaylists={() => setView('playlists')}
              playNext={playNext}
              addToQueue={addToQueue}
              onRadio={() => void startRadio()}
              radioBusy={radioBusy}
              radioError={radioError}
            />
          ) : (
            <PlaylistChoices trackId={track.videoId} onDone={close} onBack={() => setView('main')} />
          )}
        </Menu>
      )}
    </>
  );
}

function MainMenu({
  track,
  isLocal,
  onClose,
  onPlaylists,
  playNext,
  addToQueue,
  onRadio,
  radioBusy,
  radioError,
}: {
  track: Track;
  isLocal: boolean;
  onClose: () => void;
  onPlaylists: () => void;
  playNext: (tracks: Track[]) => void;
  addToQueue: (tracks: Track[]) => void;
  onRadio: () => void;
  radioBusy: boolean;
  radioError: string | null;
}) {
  return (
    <>
      <MenuHeader>{track.title}</MenuHeader>
      {!isLocal && (
        <>
          <MenuItem icon={<Radio size={15} />} disabled={radioBusy} onSelect={onRadio}>
            {radioBusy ? 'Loading radio…' : 'Start radio'}
          </MenuItem>
          {radioError && <p className="px-2 pb-1 type-caption text-secondary">{radioError}</p>}
          <MenuSeparator />
        </>
      )}
      <MenuItem icon={<ListStart size={15} />} onSelect={() => (onClose(), playNext([track]))}>
        Play Next
      </MenuItem>
      <MenuItem icon={<ListPlus size={15} />} onSelect={() => (onClose(), addToQueue([track]))}>
        Add to Queue
      </MenuItem>
      {isLocal && (
        <>
          <MenuSeparator />
          <MenuItem icon={<ChevronRight size={14} />} onSelect={onPlaylists}>
            Add to Playlist
          </MenuItem>
        </>
      )}
    </>
  );
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

/** The playlist picker: an existing playlist, or a new one made on the spot. */
function PlaylistChoices({ trackId, onDone, onBack }: { trackId: string; onDone: () => void; onBack: () => void }) {
  const client = useQueryClient();
  const playlists = useQuery({ queryKey: ['local', 'playlists'], queryFn: () => invoke<[number, string][]>('playlists_get') });
  const trackNum = Number(trackId.replace('local:', ''));
  const add = useMutation({
    mutationFn: ({ playlistId }: { playlistId: number }) =>
      invoke('playlist_add_tracks', { id: playlistId, ids: [trackNum] }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ['local'] });
      onDone();
    },
  });
  const createAndAdd = useMutation({
    mutationFn: async (name: string) => {
      const id = await invoke<number>('playlist_create', { name });
      await invoke('playlist_add_tracks', { id, ids: [trackNum] });
      return id;
    },
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ['local'] });
      onDone();
    },
  });

  return (
    <>
      <MenuItem icon={<ChevronLeft size={14} />} onSelect={onBack}>
        Back
      </MenuItem>
      <MenuSeparator />
      <MenuHeader>Add to Playlist</MenuHeader>
      {playlists.isPending && (
        <MenuItem disabled onSelect={() => {}}>
          Loading…
        </MenuItem>
      )}
      {playlists.data?.length === 0 && (
        <MenuItem disabled onSelect={() => {}}>
          No playlists yet
        </MenuItem>
      )}
      {playlists.data?.map(([id, name]) => (
        <MenuItem key={id} onSelect={() => add.mutate({ playlistId: id })}>
          {name}
        </MenuItem>
      ))}
      <MenuSeparator />
      <MenuItem
        icon={<Plus size={15} />}
        onSelect={() => {
          const name = window.prompt('Playlist name', 'New Playlist');
          if (name && name.trim()) createAndAdd.mutate(name.trim());
        }}
      >
        New Playlist…
      </MenuItem>
    </>
  );
}
