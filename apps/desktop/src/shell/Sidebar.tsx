import { Clock, ListMusic, Radio, Settings } from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import { PrefetchNavLink } from '@/components/PrefetchLink';
import {
  AlbumsIcon,
  AllPlaylistsIcon,
  ArtistsIcon,
  HomeIcon,
  SearchIcon,
  SongsIcon,
  type NavIconProps,
} from '@/components/icons/NavIcons';
import { invoke } from '@tauri-apps/api/core';
import { useUi } from '@/state/ui';

interface NavItem {
  to: string;
  label: string;
  Icon: (props: NavIconProps) => React.ReactNode;
}

const PRIMARY: NavItem[] = [
  { to: '/search', label: 'Search', Icon: SearchIcon },
  { to: '/home', label: 'Home', Icon: HomeIcon },
  { to: '/radio', label: 'Radio', Icon: (props) => <Radio size={16} {...props} /> },
];

const LIBRARY: NavItem[] = [
  { to: '/library/recent', label: 'Recently Added', Icon: (props) => <Clock size={16} {...props} /> },
  { to: '/library/artists', label: 'Artists', Icon: ArtistsIcon },
  { to: '/library/albums', label: 'Albums', Icon: AlbumsIcon },
  { to: '/library/songs', label: 'Songs', Icon: SongsIcon },
];

// Where Music lists Store — Tansen has no store, so Settings lives here.
const SETTINGS: NavItem[] = [{ to: '/settings', label: 'Settings', Icon: (props) => <Settings size={16} {...props} /> }];

// A sidebar row; NavLink sets aria-current on the page's own row.
// Measured (Music, light): row 32 pt, pill inset x10, icon 16 pt centred
// x≈30, label 13 pt at x54, radius 8.
const item =
  'group flex h-8 items-center gap-4 mb-px rounded-lg pl-[10px] pr-2 type-body text-primary hover:bg-quinary aria-[current=page]:bg-nav-selected';
// Glyphs at 16px in a fixed column, so every label starts at the same x.
const icon = 'inline-grid size-4 flex-none place-items-center text-key';
const navLabel = 'min-w-0 truncate';
const sectionHeader = 'mt-4 pl-[3px] pb-1 text-[11px] leading-[13px] font-normal text-secondary';
const list = 'flex flex-col';

function NavList({ items, onNavigate }: { items: NavItem[]; onNavigate: () => void }) {
  return (
    <ul className={list}>
      {items.map(({ to, label, Icon }) => (
        <li key={to}>
          <PrefetchNavLink to={to} className={item} onClick={onNavigate}>
            <Icon size={16} className={icon} />
            <span className={navLabel}>{label}</span>
          </PrefetchNavLink>
        </li>
      ))}
    </ul>
  );
}

/** The left material sidebar, with the traffic lights over its top (Toolbar.tsx). */
export function Sidebar() {
  const drawerOpen = useUi((s) => s.drawerOpen);
  const setDrawerOpen = useUi((s) => s.setDrawerOpen);
  const close = () => setDrawerOpen(false);
  const playlists = useQuery({ queryKey: ['local', 'playlists'], queryFn: () => invoke<[number, string][]>('playlists_get') });

  return (
    <>
      {/* Below 484px the sidebar is a drawer opened from the top bar's menu button. */}
      <nav
        className="chrome-dim absolute inset-y-0 left-0 z-(--z-web-chrome) flex w-(--web-navigation-width) select-none flex-col max-sm:w-[min(300px,82vw)] max-sm:-translate-x-full max-sm:bg-page max-sm:shadow-glass max-sm:transition-transform max-sm:duration-(--duration-panel) max-sm:ease-panel max-sm:data-[open=true]:translate-none"
        data-open={drawerOpen}
        aria-label="Main"
      >
        {/* The title bar is overlaid, so the first row sits below the traffic
            lights; the empty band above the rows drags the window. */}
        <div data-tauri-drag-region className="absolute inset-x-0 top-0 h-[52px]" />
        <div className="min-h-0 flex-1 overflow-y-auto pt-[52px] pr-3 pl-3 [scrollbar-width:none]">
          <NavList items={PRIMARY} onNavigate={close} />
          <h2 className={sectionHeader}>Library</h2>
          <NavList items={LIBRARY} onNavigate={close} />
          <h2 className={sectionHeader}>Tansen</h2>
          <NavList items={SETTINGS} onNavigate={close} />
          <h2 className={sectionHeader}>Playlists</h2>
          <ul className={list}>
            <li>
              <PrefetchNavLink to="/library/playlists" className={item} onClick={close}>
                <AllPlaylistsIcon size={16} className={icon} />
                <span className={navLabel}>All Playlists</span>
              </PrefetchNavLink>
            </li>
            {playlists.data?.map(([id, name]) => (
              <li key={id}>
                <PrefetchNavLink to={`/playlist/${id}`} className={item} onClick={close}>
                  <span className={icon}>
                    <ListMusic size={16} strokeWidth={1.8} />
                  </span>
                  <span className={navLabel}>{name}</span>
                </PrefetchNavLink>
              </li>
            ))}
          </ul>
        </div>
      </nav>
      {drawerOpen && <div className="absolute inset-0 z-[calc(var(--z-web-chrome)-1)] hidden bg-scrim max-sm:block" onClick={close} aria-hidden="true" />}
    </>
  );
}
