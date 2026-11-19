import { Suspense, lazy, useRef } from 'react';
import { Outlet } from 'react-router';
import { PlayerBar } from './PlayerBar';
import { Sidebar } from './Sidebar';
import { Toolbar } from './Toolbar';
import { useSmoothScroll } from './smoothScroll';

/**
 * The desktop shell, as the upstream web player composes it: a scrolling page
 * under a fixed glass sidebar, the player pill floating over the page's foot,
 * the side panel (Up Next / lyrics) over the right edge, and the full-screen
 * player over everything but menus.
 */
const SidePanel = lazy(() => import('./SidePanel').then((m) => ({ default: m.SidePanel })));
const FullScreenPlayer = lazy(() => import('./FullScreenPlayer').then((m) => ({ default: m.FullScreenPlayer })));
const MobileTopBar = lazy(() => import('./MobileTopBar').then((m) => ({ default: m.MobileTopBar })));

export function AppShell() {
  const pageScroller = useRef<HTMLElement>(null);
  useSmoothScroll(pageScroller);
  return (
    <div className="grid h-full grid-rows-[minmax(0,1fr)_auto]">
      <div className="relative min-h-0 overflow-hidden">
        <Sidebar />
        <div className="absolute inset-0 flex flex-col bg-page bg-clip-content pl-(--web-navigation-width) max-sm:bg-clip-border max-sm:pl-0">
          <MobileTopBar />
          {/* Music's toolbar: the centred search field and the scope control. */}
          <Toolbar />
          <main
            ref={pageScroller}
            id="scrollable-page"
            className="relative min-h-0 flex-1 overflow-x-hidden overflow-y-auto [scrollbar-gutter:stable]"
          >
            <div className="min-h-full pb-[calc(var(--player-pill-height)+var(--player-bottom-offset)+40px)]">
              <Outlet />
            </div>
          </main>
        </div>
        <PlayerBar />
        <SidePanel />
      </div>
      <Suspense>
        <FullScreenPlayer />
      </Suspense>
    </div>
  );
}
