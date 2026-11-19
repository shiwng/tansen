import { X } from 'lucide-react';
import { useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import type { Track } from '@/data/music/types';
import { formatDuration } from './links';

const SOURCE_LABELS: Record<string, string> = {
  local: 'Local library',
  youtube: 'YouTube Music',
  spotify: 'Spotify',
  subsonic: 'Subsonic',
  addon: 'Add-on',
};

/**
 * Track metadata, shown from the context menu's Get Info row. A small
 * material dialog over a scrim; Escape or the scrim closes it and focus
 * returns to the ⋯ button that opened it.
 */
export function TrackInfoDialog({ track, onClose }: { track: Track; onClose: () => void }) {
  const dialog = useRef<HTMLDivElement>(null);
  const closeButton = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    closeButton.current?.focus({ preventScroll: true });
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        onClose();
        return;
      }
      if (event.key !== 'Tab') return;
      const focusable = [...(dialog.current?.querySelectorAll<HTMLElement>('button') ?? [])];
      if (!focusable.length) return;
      event.preventDefault();
      const index = focusable.indexOf(document.activeElement as HTMLElement);
      const next = index < 0 ? 0 : (index + (event.shiftKey ? -1 : 1) + focusable.length) % focusable.length;
      focusable[next]?.focus({ preventScroll: true });
    };
    document.addEventListener('keydown', onKey, true);
    return () => document.removeEventListener('keydown', onKey, true);
  }, [onClose]);

  const source = track.videoId.startsWith('local:') ? 'local' : (track.sourceKind ?? 'youtube');
  const rows: Array<[string, string]> = [
    ['Title', track.title],
    ['Artist', track.artists.map((artist) => artist.name).join(', ') || 'Unknown Artist'],
    ['Album', track.album?.name ?? '—'],
    ['Duration', formatDuration(track.durationSec) || '—'],
    ['Source', SOURCE_LABELS[source] ?? source],
  ];

  return createPortal(
    <>
      <div className="fixed inset-0 z-(--z-modal) bg-scrim" onPointerDown={onClose} aria-hidden="true" />
      <div
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-label={`Get Info for ${track.title}`}
        className="glass-edge fixed top-1/2 left-1/2 z-[calc(var(--z-modal)+1)] w-[360px] max-w-[calc(100vw-32px)] -translate-x-1/2 -translate-y-1/2 rounded-[12px] bg-context-menu p-5 text-primary shadow-[0_0_0_0.5px_var(--vibrantDivider),0_18px_60px_var(--glassMaterialShadowColor)] [backdrop-filter:var(--dynamicBlurMenu)]"
      >
        <header className="flex items-start justify-between gap-4">
          <h2 className="type-title-3-emphasized">Get Info</h2>
          <button
            ref={closeButton}
            type="button"
            aria-label="Close"
            onClick={onClose}
            className="grid size-6 place-items-center rounded-full text-secondary hover:bg-quinary hover:text-primary"
          >
            <X size={15} strokeWidth={2.4} />
          </button>
        </header>
        <dl className="mt-4 grid grid-cols-[72px_1fr] gap-x-3 gap-y-2 type-body">
          {rows.map(([label, value]) => (
            <div key={label} className="contents">
              <dt className="text-secondary">{label}</dt>
              <dd className="min-w-0 break-words text-primary">{value}</dd>
            </div>
          ))}
        </dl>
        <footer className="mt-5 flex justify-end">
          <button
            type="button"
            onClick={onClose}
            className="inline-flex h-6 items-center rounded-pill bg-quaternary px-3.5 type-body-emphasized text-primary hover:bg-gray-4"
          >
            Done
          </button>
        </footer>
      </div>
    </>,
    document.body,
  );
}
