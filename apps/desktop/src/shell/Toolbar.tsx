import { useLayoutEffect, useRef, useState } from 'react';
import { Glass } from 'glisse';
import { useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { useLocation, useNavigate, useSearchParams } from 'react-router';
import { ChevronRight, ListFilter } from 'lucide-react';
import { SearchField } from '@/components/music/SearchField';
import { Menu, MenuItem, MenuSeparator } from '@/components/Menu';
import { matchRoute } from '@/pageRegistry';

interface SourceConfig {
  id: string;
  name: string;
  kind: string;
  enabled: boolean;
}

/**
 * The page toolbar: a centred search field and the scope control, as Music's
 * window has. The scopes are the real enabled catalogue sources; the local
 * library is its own scope.
 */
export function Toolbar() {
  const [params, setParams] = useSearchParams();
  const navigate = useNavigate();
  const location = useLocation();
  // Library and Radio pages search themselves in place instead of navigating
  // to /search (Music's "Find in Albums", "Find in Recently Added"): the
  // field sits at the toolbar's right edge above those pages.
  const routePath = matchRoute(location.pathname)?.path ?? '';
  const onPlaylists = routePath === '/library/playlists';
  const inPlaceLabel: Record<string, string> = {
    '/library/albums': 'Find in Albums',
    '/library/recent': 'Find in Recently Added',
    '/radio': 'Find Stations',
  };
  const inPlace = onPlaylists || routePath in inPlaceLabel;
  const query = params.get('q') ?? '';
  const scopeParam = params.get('scope') ?? '';
  const sources = useQuery({
    // Not `['sources']`: Settings caches `sources_get_with_health` under that
    // key, and a shared key would hand it rows without `health`.
    queryKey: ['sources', 'config'],
    queryFn: () => invoke<SourceConfig[]>('sources_get'),
    staleTime: 60_000,
    retry: false,
  });

  const scopes = [
    { value: 'library', label: 'Library' },
    ...(sources.data ?? [])
      .filter((source) => source.enabled && source.kind !== 'local')
      .map((source) => ({ value: source.kind, label: source.name })),
  ];
  const selected = scopes.some((scope) => scope.value === scopeParam) ? scopeParam : '';
  const placeholder = scopes.find((scope) => scope.value === selected)?.label ?? 'Search';

  // Sliding selection indicator: geometry is written straight to the DOM node
  // in the layout effect — no React state round-trip, which delayed the
  // transition start under the navigate()+refetch render load and made the
  // movement snap. The selection stays owned by the URL; this only mirrors it.
  const buttonRefs = useRef<Map<string, HTMLButtonElement>>(new Map());
  const indicatorRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const el = selected ? buttonRefs.current.get(selected) : undefined;
    const ind = indicatorRef.current;
    if (!ind) return;
    if (!el) {
      ind.style.opacity = '0';
      return;
    }
    ind.style.opacity = '1';
    ind.style.transform = `translateX(${el.offsetLeft}px)`;
    ind.style.width = `${el.offsetWidth}px`;
  }, [selected, scopes.length, sources.data]);

  const search = (value: string) => {
    if (inPlace) return; // already applied per keystroke below; Enter just confirms
    const trimmed = value.trim();
    if (!trimmed) return navigate('/search');
    navigate(`/search?q=${encodeURIComponent(trimmed)}${selected ? `&scope=${selected}` : ''}`);
  };

  const searchInPlace = (value: string) => {
    const next = new URLSearchParams(params);
    if (value.trim()) next.set('q', value.trim());
    else next.delete('q');
    setParams(next, { replace: true });
  };

  return (
    <div data-tauri-drag-region className="chrome-dim relative z-10 flex h-(--toolbar-height) flex-none select-none items-center justify-center px-6">
      {!inPlace && (
        <SearchField
          initial={query}
          placeholder={placeholder}
          onSearch={search}
          suggest
        />
      )}
      <div className="absolute right-5 flex items-center gap-2">
      {onPlaylists && <PlaylistsFilterMenu filter={params.get('filter') ?? ''} sort={params.get('sort') ?? ''} />}
      {inPlace && (
        <div className="w-[260px]">
          <SearchField
            initial={query}
            placeholder={onPlaylists ? 'Find in All Playlists' : inPlaceLabel[routePath]}
            onSearch={search}
            suggest={false}
            onInputChange={searchInPlace}
          />
        </div>
      )}
      {!inPlace && <Glass
        className="header-glass relative flex items-center gap-px rounded-pill p-[3px]"
        role="tablist"
        aria-label="Search scope"
      >
        <div
          ref={indicatorRef}
          aria-hidden
          className="absolute left-0 bottom-[3px] top-[3px] rounded-pill bg-segmented-selected shadow-[0_1px_3px_#00000026,0_0_0_0.5px_#0000001a,inset_0_0.5px_0_#ffffff66]"
          style={{
            transform: 'translateX(0)',
            transition: 'transform 250ms cubic-bezier(0.32, 0.72, 0.24, 1), width 250ms cubic-bezier(0.32, 0.72, 0.24, 1), opacity 150ms ease',
            willChange: 'transform',
          }}
        />
        {scopes.map((scope) => (
          <button
            key={scope.value}
            ref={(el) => {
              if (el) buttonRefs.current.set(scope.value, el);
              else buttonRefs.current.delete(scope.value);
            }}
            role="tab"
            aria-selected={scope.value === selected}
            onClick={() => navigate(scope.value === selected ? '/search' : `/search?scope=${scope.value}`)}
            className={`relative z-[1] h-[24px] rounded-pill px-3 type-callout-emphasized transition-colors ${
              scope.value === selected ? 'text-primary' : 'text-secondary hover:text-primary'
            }`}
          >
            {scope.label}
          </button>
        ))}
      </Glass>}
      </div>
    </div>
  );
}

/**
 * The All Playlists filter menu (Music's library filter): All Playlists /
 * Only Favourites, then a Sort Options flyout. "Recently Played" is not
 * offered because the model carries no per-playlist play data. State lives in
 * the page's URL params, so the toolbar and the page read one source.
 */
function PlaylistsFilterMenu({ filter, sort }: { filter: string; sort: string }) {
  const [params, setParams] = useSearchParams();
  const [open, setOpen] = useState(false);
  const [sortOpen, setSortOpen] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const sortRowRef = useRef<HTMLDivElement>(null);

  const set = (key: string, value: string) => {
    const next = new URLSearchParams(params);
    if (value) next.set(key, value);
    else next.delete(key);
    setParams(next, { replace: true });
  };

  return (
    <>
      <button
        ref={buttonRef}
        onClick={() => setOpen((v) => !v)}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label="Filter and sort playlists"
        className="grid size-8 flex-none place-items-center rounded-full bg-segmented text-secondary hover:text-primary"
      >
        <ListFilter size={15} strokeWidth={2} />
      </button>
      {open && buttonRef.current && (
        <Menu
          anchor={buttonRef.current}
          placement="below-end"
          onClose={() => {
            setOpen(false);
            setSortOpen(false);
          }}
          label="Filter and sort playlists"
        >
          <MenuItem
            checked={filter !== 'favourites'}
            onSelect={() => {
              set('filter', '');
              setOpen(false);
            }}
          >
            All Playlists
          </MenuItem>
          <MenuItem
            checked={filter === 'favourites'}
            onSelect={() => {
              set('filter', 'favourites');
              setOpen(false);
            }}
          >
            Only Favourites
          </MenuItem>
          <MenuSeparator />
          <div ref={sortRowRef}>
            <MenuItem checked={undefined} trailing={<ChevronRight size={14} />} onSelect={() => setSortOpen((v) => !v)}>
              Sort Options
            </MenuItem>
          </div>
        </Menu>
      )}
      {open && sortOpen && sortRowRef.current && (
        <Menu anchor={sortRowRef.current} placement="left-start" onClose={() => setSortOpen(false)} label="Sort playlists">
          <MenuItem
            checked={sort === 'name'}
            onSelect={() => {
              set('sort', 'name');
              setOpen(false);
              setSortOpen(false);
            }}
          >
            Name
          </MenuItem>
          <MenuItem
            checked={sort === 'added'}
            onSelect={() => {
              set('sort', 'added');
              setOpen(false);
              setSortOpen(false);
            }}
          >
            Date Added
          </MenuItem>
        </Menu>
      )}
    </>
  );
}
