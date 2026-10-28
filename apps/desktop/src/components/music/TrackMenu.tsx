import { ListPlus, ListStart, MoreHorizontal, Radio } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { Menu, MenuHeader, MenuItem, MenuSeparator, type MenuPlacement } from '@/components/Menu';
import type { Track } from '@/data/music/types';
import { usePlayer } from '@/state/player';

/**
 * The ⋯ button at the end of a track row — upstream's TrackMenu with the
 * account-coupled items (favorite, add to playlist) removed until those
 * features exist on desktop. Local-library items only.
 */
export function TrackMenu({
  track,
  className,
  placement,
  onOpenChange,
}: {
  track: Track;
  className?: string;
  placement?: MenuPlacement;
  onOpenChange?: (open: boolean) => void;
}) {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const close = useCallback(() => setAnchor(null), []);
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
          <MenuHeader>{track.title}</MenuHeader>
          {isLocal && (
            <>
              <MenuItem icon={<Radio size={15} />} onSelect={close}>
                Start radio — not yet implemented
              </MenuItem>
              <MenuSeparator />
            </>
          )}
          <MenuItem icon={<ListStart size={15} />} onSelect={() => (close(), playNext([track]))}>
            Play Next
          </MenuItem>
          <MenuItem icon={<ListPlus size={15} />} onSelect={() => (close(), addToQueue([track]))}>
            Add to Queue
          </MenuItem>
          <MenuSeparator />
          <MenuItem disabled onSelect={() => {}}>
            Add to Playlist — not yet implemented
          </MenuItem>
        </Menu>
      )}
    </>
  );
}
