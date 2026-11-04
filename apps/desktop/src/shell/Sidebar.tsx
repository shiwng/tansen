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
  { to: '/radio', label: 'Radio', Icon: (props) => <Radio size={20} {...props} /> },
];

const LIBRARY: NavItem[] = [
  { to: '/library/recent', label: 'Recently Added', Icon: (props) => <Clock size={20} {...props} /> },
  { to: '/library/artists', label: 'Artists', Icon: ArtistsIcon },
  { to: '/library/albums', label: 'Albums', Icon: AlbumsIcon },
  { to: '/library/songs', label: 'Songs', Icon: SongsIcon },
];

// Where Music lists Store — Tansen has no store, so Settings lives here.
const SETTINGS: NavItem[] = [{ to: '/settings', label: 'Settings', Icon: (props) => <Settings size={20} {...props} /> }];

// A sidebar row; NavLink sets aria-current on the page's own row.
const item =
  'group flex h-7 items-center gap-2 mb-px rounded-[6px] px-2 type-body-reduced text-primary hover:bg-quinary aria-[current=page]:bg-nav-selected';
// Glyphs at 20px in a fixed column, so every label starts at the same x.
const icon = 'inline-grid size-5 flex-none place-items-center text-key';
const navLabel = 'min-w-0 truncate';
const sectionHeader = 'mt-4 px-2 pb-1 type-callout-emphasized text-secondary';
const list = 'flex flex-col';

function NavList({ items, onNavigate }: { items: NavItem[]; onNavigate: () => void }) {
  return (
    <ul className={list}>
      {items.map(({ to, label, Icon }) => (
        <li key={to}>
          <PrefetchNavLink to={to} className={item} onClick={onNavigate}>
            <Icon size={20} className={icon} />
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
        className="absolute inset-y-0 left-0 z-(--z-web-chrome) flex w-(--web-navigation-width) flex-col glass-material max-sm:w-[min(300px,82vw)] max-sm:-translate-x-full max-sm:shadow-glass max-sm:transition-transform max-sm:duration-(--duration-panel) max-sm:ease-panel max-sm:data-[open=true]:translate-none"
        data-open={drawerOpen}
        aria-label="Main"
      >
        {/* The title bar is overlaid, so the first row sits below the traffic lights. */}
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
                <AllPlaylistsIcon size={20} className={icon} />
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
