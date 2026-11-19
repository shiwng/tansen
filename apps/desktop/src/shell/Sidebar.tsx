import { CircleUserRound, ListMusic, Settings } from 'lucide-react';
import { useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { PrefetchNavLink } from '@/components/PrefetchLink';
import { Menu, MenuItem, MenuHeader, MenuSeparator } from '@/components/Menu';
import { useArtworkUrl } from '@/components/music/Artwork';
import {
  AlbumsIcon,
  AllPlaylistsIcon,
  ArtistsIcon,
  HomeIcon,
  RadioWavesIcon,
  RecentlyAddedIcon,
  SearchIcon,
  SongsIcon,
  type NavIconProps,
} from '@/components/icons/NavIcons';
import { invoke } from '@tauri-apps/api/core';
import { useGoogle } from '@/state/google';
import { useUi } from '@/state/ui';
import { useT, type MessageKey } from '@/i18n';
import type { PlaylistRow } from '@/types';

interface NavItem {
  to: string;
  label: MessageKey;
  Icon: (props: NavIconProps) => React.ReactNode;
}

const PRIMARY: NavItem[] = [
  { to: '/search', label: 'nav.search', Icon: (props) => <SearchIcon size={28} {...props} /> },
  { to: '/home', label: 'nav.home', Icon: (props) => <HomeIcon size={28} {...props} /> },
  { to: '/radio', label: 'nav.radio', Icon: (props) => <RadioWavesIcon size={28} {...props} /> },
];

const LIBRARY: NavItem[] = [
  { to: '/library/recent', label: 'nav.recent', Icon: (props) => <RecentlyAddedIcon size={28} {...props} /> },
  { to: '/library/artists', label: 'nav.artists', Icon: (props) => <ArtistsIcon size={28} {...props} /> },
  { to: '/library/albums', label: 'nav.albums', Icon: (props) => <AlbumsIcon size={28} {...props} /> },
  { to: '/library/songs', label: 'nav.songs', Icon: (props) => <SongsIcon size={28} {...props} /> },
];

// Where Music lists Store — Tansen has no store, so Settings lives here.
const SETTINGS: NavItem[] = [
  { to: '/settings', label: 'nav.settings', Icon: (props) => <Settings size={20} strokeWidth={1.8} {...props} /> },
];

// Reference geometry (Apple Music, 1397×820): 200px sidebar, 10px list inset,
// 32px rows, 13px labels, icon centre x36, label x52. NavLink sets
// aria-current on the page's own row; it wins over hover/focus below.
const item =
  'group flex h-8 items-center gap-[2px] rounded-lg pr-2 pl-3 type-body text-primary transition-colors duration-100 hover:bg-quinary active:bg-quaternary focus-visible:bg-quaternary aria-[current=page]:bg-nav-selected';
// Custom glyphs keep their 28px boxes (16-19px of ink, as in the reference);
// the box centres them, so every label starts at x52.
const iconColumn = 'grid size-7 flex-none place-items-center text-key';
const navLabel = 'min-w-0 truncate';
// Section label x12; 13px medium in a 16px line, 16px above, rows resume after.
const sectionHeader = 'mt-4 pl-[2px] text-[13px] leading-4 font-medium text-secondary';
const list = 'flex flex-col';

// Account row: 28px avatar at x16, name at x50, 13px medium.
const accountItem =
  'flex h-8 w-full items-center gap-[6px] rounded-lg type-[500_13px/16px] text-primary transition-colors duration-100 hover:bg-quinary active:bg-quaternary focus-visible:bg-quaternary';
const avatar = 'grid size-7 flex-none place-items-center rounded-full';

function NavList({ items, onNavigate }: { items: NavItem[]; onNavigate: () => void }) {
  const t = useT();
  return (
    <ul className={list}>
      {items.map(({ to, label, Icon }) => (
        <li key={to}>
          <PrefetchNavLink to={to} className={item} onClick={onNavigate}>
            <span className={iconColumn}>
              <Icon />
            </span>
            <span className={navLabel}>{t(label)}</span>
          </PrefetchNavLink>
        </li>
      ))}
    </ul>
  );
}

// Google profiles can have a name but no picture: fall back to initials, or
// the email's first letter when the profile has no name either.
function accountInitials(name: string, email: string) {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length >= 2) return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return email.trim().slice(0, 1).toUpperCase() || '?';
}

/**
 * The bottom-left account row. Signed out it is the one-click entry to the
 * Google sign-in flow (the browser opens; this window never leaves the
 * sidebar); signed in it shows the Google profile and a menu with the email
 * and Sign Out. The app is fully usable without signing in.
 */
function ProfileArea() {
  const profile = useGoogle((s) => s.profile);
  const signingIn = useGoogle((s) => s.signingIn);
  const error = useGoogle((s) => s.error);
  const signIn = useGoogle((s) => s.signIn);
  const signOut = useGoogle((s) => s.signOut);
  const switchAccount = useGoogle((s) => s.switchAccount);
  const [menuOpen, setMenuOpen] = useState(false);
  const rowRef = useRef<HTMLButtonElement>(null);
  // Google avatars come from lh3.googleusercontent.com — inside the
  // fetch_artwork allowlist, so they cache like every other remote image.
  const picture = useArtworkUrl(profile?.picture.startsWith('https://') ? profile.picture : null);

  if (!profile) {
    return (
      <div>
        <button className={accountItem} onClick={() => void signIn()} disabled={signingIn} aria-disabled={signingIn}>
          <span className={`${avatar} text-key`}>
            <CircleUserRound size={18} strokeWidth={1.6} />
          </span>
          <span className={navLabel}>{signingIn ? 'Signing in…' : 'Continue with Google'}</span>
        </button>
        {/* Honest failure feedback (no client ID, cancelled, offline…) —
            never a silent click. */}
        {error && (
          <p className="mt-0.5 truncate pl-[34px] type-footnote text-secondary" title={error}>
            {error}
          </p>
        )}
      </div>
    );
  }
  return (
    <>
      <button
        ref={rowRef}
        className={accountItem}
        onClick={() => setMenuOpen(true)}
        aria-haspopup="menu"
        aria-expanded={menuOpen}
      >
        {picture ? (
          <img src={picture} alt="" className="size-7 flex-none rounded-full object-cover" />
        ) : (
          <span className={`${avatar} bg-key type-[600_11px/14px] text-white`}>
            {accountInitials(profile.name, profile.email)}
          </span>
        )}
        <span className={navLabel}>{profile.name || profile.email}</span>
      </button>
      {menuOpen && rowRef.current && (
        <Menu anchor={rowRef.current} placement="above-start" onClose={() => setMenuOpen(false)} label="Account">
          <MenuHeader>{profile.email}</MenuHeader>
          <MenuSeparator />
          <MenuItem
            onSelect={() => {
              setMenuOpen(false);
              void switchAccount();
            }}
          >
            Switch Account
          </MenuItem>
          <MenuItem
            destructive
            onSelect={() => {
              setMenuOpen(false);
              void signOut();
            }}
          >
            Sign Out
          </MenuItem>
        </Menu>
      )}
    </>
  );
}

/** The left material sidebar, with the traffic lights over its top (Toolbar.tsx). */
export function Sidebar() {
  const t = useT();
  const drawerOpen = useUi((s) => s.drawerOpen);
  const setDrawerOpen = useUi((s) => s.setDrawerOpen);
  const close = () => setDrawerOpen(false);
  const playlists = useQuery({ queryKey: ['local', 'playlists'], queryFn: () => invoke<PlaylistRow[]>('playlists_get') });

  return (
    <>
      {/* Below 484px the sidebar is a drawer opened from the top bar's menu button. */}
      <nav
        className="chrome-dim absolute inset-y-0 left-0 z-(--z-web-chrome) flex w-(--web-navigation-width) select-none flex-col max-sm:w-[min(300px,82vw)] max-sm:-translate-x-full max-sm:bg-page max-sm:shadow-glass max-sm:transition-transform max-sm:duration-(--duration-panel) max-sm:ease-panel max-sm:data-[open=true]:translate-none"
        data-open={drawerOpen}
        aria-label="Main"
      >
        {/* The title bar is overlaid, so the first row sits below the traffic
            lights; the empty band above the rows drags the window. Music's
            first row top is 52pt, level with the toolbar's foot. */}
        <div data-tauri-drag-region className="absolute inset-x-0 top-0 h-[52px]" />
        <div className="min-h-0 flex-1 overflow-y-auto px-[10px] pt-[52px] [scrollbar-width:none]">
          <NavList items={PRIMARY} onNavigate={close} />
          <h2 className={sectionHeader}>{t('nav.librarySection')}</h2>
          <NavList items={LIBRARY} onNavigate={close} />
          <h2 className={sectionHeader}>Tansen</h2>
          <NavList items={SETTINGS} onNavigate={close} />
          <h2 className={sectionHeader}>{t('nav.playlists')}</h2>
          <ul className={list}>
            <li>
              <PrefetchNavLink to="/library/playlists" className={item} onClick={close}>
                <span className={iconColumn}>
                  <AllPlaylistsIcon size={28} />
                </span>
                <span className={navLabel}>{t('nav.allPlaylists')}</span>
              </PrefetchNavLink>
            </li>
            {playlists.data?.map((p) => (
              <li key={p.id}>
                <PrefetchNavLink to={`/playlist/${p.id}`} className={item} onClick={close}>
                  <span className={iconColumn}>
                    <ListMusic size={20} strokeWidth={1.8} />
                  </span>
                  <span className={navLabel}>{p.name}</span>
                </PrefetchNavLink>
              </li>
            ))}
          </ul>
        </div>
        {/* The account row sits below the scroll area, pinned bottom-left:
            hairline divider, 16px inset, 28px avatar, 12px around the row. */}
        <div className="flex-none border-t-[0.5px] border-t-vibrant-divider/40 px-4 py-3">
          <ProfileArea />
        </div>
      </nav>
      {drawerOpen && <div className="absolute inset-0 z-[calc(var(--z-web-chrome)-1)] hidden bg-scrim max-sm:block" onClick={close} aria-hidden="true" />}
    </>
  );
}
