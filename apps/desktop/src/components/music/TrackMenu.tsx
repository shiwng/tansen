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
  const close = useCallback(() => {
    setAnchor(null);
    setView('main');
  }, []);
  useEffect(() => {
    onOpenChange?.(anchor !== null);
  }, [anchor, onOpenChange]);

  const playNext = usePlayer((s) => s.playNext);
  const addToQueue = usePlayer((s) => s.addToQueue);
  const isLocal = track.videoId.startsWith('local:');

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
}: {
  track: Track;
  isLocal: boolean;
  onClose: () => void;
  onPlaylists: () => void;
  playNext: (tracks: Track[]) => void;
  addToQueue: (tracks: Track[]) => void;
}) {
  return (
    <>
      <MenuHeader>{track.title}</MenuHeader>
      {isLocal && (
        <>
          <MenuItem icon={<Radio size={15} />} onSelect={onClose}>
            Start radio — not yet implemented
          </MenuItem>
          <MenuSeparator />
        </>
      )}
      <MenuItem icon={<ListStart size={15} />} onSelect={() => (onClose(), playNext([track]))}>
        Play Next
      </MenuItem>
      <MenuItem icon={<ListPlus size={15} />} onSelect={() => (onClose(), addToQueue([track]))}>
        Add to Queue
      </MenuItem>
      <MenuSeparator />
      <MenuItem icon={<ChevronRight size={14} />} onSelect={onPlaylists}>
        Add to Playlist
      </MenuItem>
    </>
  );
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
