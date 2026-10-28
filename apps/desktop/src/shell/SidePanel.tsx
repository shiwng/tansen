import { Suspense, lazy } from 'react';
import { useUi } from '@/state/ui';

const UpNext = lazy(() => import('./UpNext').then((m) => ({ default: m.UpNext })));
const LyricsPanel = lazy(() => import('./LyricsPanel').then((m) => ({ default: m.LyricsPanel })));

/** The right glass panel: Up Next or the lyrics, as the pill's buttons choose. */
export function SidePanel() {
  const panel = useUi((s) => s.sidePanel);
  if (!panel) return null;
  return (
    <aside
      className="absolute inset-y-0 right-0 z-(--z-web-chrome) flex w-(--side-panel-width) animate-slide-in flex-col shadow-glass glass-material"
      aria-label={panel === 'up-next' ? 'Up Next' : 'Lyrics'}
    >
      {panel === 'up-next' ? (
        <Suspense>
          <UpNext />
        </Suspense>
      ) : (
        <Suspense>
          <LyricsPanel />
        </Suspense>
      )}
    </aside>
  );
}
