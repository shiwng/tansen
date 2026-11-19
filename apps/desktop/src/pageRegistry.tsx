import { lazy, type ComponentType, type ReactNode, Suspense } from 'react';

/** The app's pages, all lazy, as the upstream registry does. */
export interface PageRoute {
  path: string;
  load: () => Promise<{ default: ComponentType }>;
  skeleton?: ReactNode;
}

const home = () => import('@/pages/HomePage').then((m) => ({ default: m.HomePage }));
const newPage = () => import('@/pages/HomePage').then((m) => ({ default: m.HomePage }));
const library = () => import('@/pages/LibraryPage').then((m) => ({ default: m.LibraryPage }));
const search = () => import('@/pages/SearchPage').then((m) => ({ default: m.SearchPage }));
const album = () => import('@/pages/AlbumPage').then((m) => ({ default: m.AlbumPage }));
const artist = () => import('@/pages/ArtistPage').then((m) => ({ default: m.ArtistPage }));
const playlist = () => import('@/pages/PlaylistPage').then((m) => ({ default: m.PlaylistPage }));
const reorder = () => import('@/pages/ReorderPlaylistPage').then((m) => ({ default: m.ReorderPlaylistPage }));
const settings = () => import('@/pages/SettingsPage').then((m) => ({ default: m.SettingsPage }));
const discover = () => import('@/pages/DiscoveryPage').then((m) => ({ default: m.DiscoveryPage }));
const browseCategory = () => import('@/pages/BrowseCategoryPage').then((m) => ({ default: m.BrowseCategoryPage }));
const radio = () => import('@/pages/RadioPage').then((m) => ({ default: m.RadioPage }));

export const PAGE_ROUTES: PageRoute[] = [
  { path: '/home', load: home },
  { path: '/new', load: newPage },
  { path: '/search', load: search },
  { path: '/album/:id', load: album },
  { path: '/artist/:id', load: artist },
  { path: '/playlist/:id', load: playlist },
  { path: '/playlist/:id/reorder', load: reorder },
  { path: '/library/:section', load: library },
  { path: '/settings', load: settings },
  { path: '/discover', load: discover },
  { path: '/browse/:category', load: browseCategory },
  { path: '/radio', load: radio },
];

export function pageElement(route: PageRoute): ReactNode {
  const Page = lazy(route.load);
  return (
    <Suspense fallback={route.skeleton ?? null}>
      <Page />
    </Suspense>
  );
}

export function matchRoute(pathname: string): PageRoute | null {
  for (const route of PAGE_ROUTES) {
    const routeSegments = route.path.split('/').filter(Boolean);
    const pathSegments = pathname.split('/').filter(Boolean);
    if (routeSegments.length !== pathSegments.length) continue;
    const ok = routeSegments.every((seg, i) => seg.startsWith(':') || seg === pathSegments[i]);
    if (ok) return route;
  }
  return null;
}

export function preloadRoute(to: string): void {
  const route = matchRoute(to);
  if (route) void route.load();
}
