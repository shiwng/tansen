import { useInfiniteQuery, useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { Ellipsis, Heart, Info, ListMusic, Play, Plus, Sparkles, X } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { useSearchParams } from 'react-router';
import { Menu, MenuHeader, MenuItem, MenuSeparator } from '@/components/Menu';
import { PlayIcon } from '@/components/icons/Transport';
import { RadioWavesIcon } from '@/components/icons/NavIcons';
import { ShelfScroller } from '@/components/music/ShelfView';
import { SkeletonBlock } from '@/components/Skeleton';
import { discoveryToTrack, type DiscoveryTrack } from '@/lib/discovery';
import {
  RADIO_PAGE_SIZE,
  radioArtwork,
  radioFacets,
  radioStations,
  stationDetail,
  stationQuality,
  validateStreamUrl,
  type RadioFacet,
  type RadioStation,
} from '@/lib/radio';
import { playCustomStation, playStation } from '@/lib/radioPlay';
import { usePlayer } from '@/state/player';
import { useRadio } from '@/state/radio';

/**
 * Radio: real broadcast stations from the public Radio Browser directory,
 * played live through Tansen's engine. Station favourites, recently played
 * and custom stream URLs live in local app storage. Personalized Radio is a
 * separate feature — it is not a broadcast, it is a queue built from the
 * listening profile.
 */
export function RadioPage() {
  const [params] = useSearchParams();
  const search = (params.get('q') ?? '').trim();
  const country = params.get('country') ?? '';
  const genre = params.get('genre') ?? '';
  const language = params.get('language') ?? '';
  const filtered = Boolean(search || country || genre || language);

  return (
    <div className="pb-16">
      <header className="mx-(--body-gutter) flex flex-wrap items-end justify-between gap-x-6 gap-y-2 pt-8">
        <div className="min-w-0">
          <h1 className="type-header-emphasized">Radio</h1>
          <p className="mt-1 max-w-xl type-callout text-secondary">
            Live stations from around the world. Broadcasts play through Tansen&apos;s player — pause, volume and
            Now Playing all work; a live stream has no timeline to seek.
          </p>
        </div>
      </header>

      {filtered ? (
        <StationResults search={search} country={country} genre={genre} language={language} />
      ) : (
        <>
          <PersonalStation />
          <Favourites />
          <RecentlyPlayed />
          <Featured />
          <Popular />
          <GenreShelves />
          <CountryShelves />
          <CustomStream />
        </>
      )}
    </div>
  );
}

function railClass(extra = '') {
  return `mx-(--body-gutter) mt-9 ${extra}`;
}

function SectionHeader({ title, caption }: { title: string; caption?: string }) {
  return (
    <div className="mb-3 flex items-end justify-between gap-4">
      <div className="min-w-0">
        <h2 className="type-title-2-emphasized">{title}</h2>
        {caption && <p className="mt-0.5 truncate type-callout text-secondary">{caption}</p>}
      </div>
    </div>
  );
}

const gridClass =
  'mx-(--body-gutter) mt-4 grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-x-5 gap-y-7 max-md:grid-cols-[repeat(auto-fill,minmax(126px,1fr))]';

function Featured() {
  const featured = useQuery({
    queryKey: ['radio', 'featured'],
    queryFn: () => radioStations({ order: 'clickcount' }),
    staleTime: 10 * 60_000,
    retry: 1,
  });
  const stations = featured.data?.slice(0, 14) ?? [];
  if (featured.isError) return null;
  if (featured.isPending) {
    return (
      <div className={railClass()}>
        <SectionHeader title="Featured Stations" />
        <div className="flex gap-5">
          {Array.from({ length: 6 }).map((_, i) => (
            <SkeletonBlock key={i} width={150} height={150} radius={10} />
          ))}
        </div>
      </div>
    );
  }
  if (!stations.length) return null;
  return (
    <section className={railClass()} aria-label="Featured stations">
      <SectionHeader title="Featured Stations" caption="Most-played live stations on Radio Browser" />
      <ShelfScroller key={stations.length} kind="square">
        {stations.map((station) => (
          <li key={station.id}>
            <StationCard station={station} />
          </li>
        ))}
      </ShelfScroller>
    </section>
  );
}

function Popular() {
  const popular = useQuery({
    queryKey: ['radio', 'popular'],
    queryFn: () => radioStations({ order: 'votes' }),
    staleTime: 10 * 60_000,
    retry: 1,
  });
  const stations = popular.data?.slice(0, 14) ?? [];
  if (!stations.length) return null;
  return (
    <section className={railClass()} aria-label="Popular stations">
      <SectionHeader title="Popular Stations" caption="Highest-rated by listeners" />
      <ShelfScroller key={stations.length} kind="square">
        {stations.map((station) => (
          <li key={station.id}>
            <StationCard station={station} />
          </li>
        ))}
      </ShelfScroller>
    </section>
  );
}

function Favourites() {
  const favourites = useRadio((s) => s.favourites);
  const remove = useRadio((s) => s.toggleFavourite);
  if (!favourites.length) return null;
  return (
    <section className={railClass()} aria-label="Favourite stations">
      <SectionHeader title="Favourites" caption="Saved on this Mac" />
      <ShelfScroller key={favourites.length} kind="square">
        {favourites.map((station) => (
          <li key={station.id}>
            <StationCard station={station} onRemove={() => remove(station)} />
          </li>
        ))}
      </ShelfScroller>
    </section>
  );
}

function RecentlyPlayed() {
  const recent = useRadio((s) => s.recent);
  if (!recent.length) return null;
  return (
    <section className={railClass()} aria-label="Recently played stations">
      <SectionHeader title="Recently Played" />
      <ShelfScroller key={recent.length} kind="square">
        {recent.map((station) => (
          <li key={station.id}>
            <StationCard station={station} />
          </li>
        ))}
      </ShelfScroller>
    </section>
  );
}

function facetsQuery() {
  return useQuery({
    queryKey: ['radio', 'facets'],
    queryFn: radioFacets,
    staleTime: 10 * 60_000,
    retry: 1,
  });
}

function GenreShelves() {
  const facets = facetsQuery();
  const [params, setParams] = useSearchParams();
  const genres = (facets.data?.genres ?? []).slice(0, 16);
  if (!genres.length) return null;
  return (
    <section className={railClass()} aria-label="Genres">
      <SectionHeader title="Genres" caption="Pick a genre to see its live stations" />
      <div className="flex flex-wrap gap-2">
        {genres.map((genre) => (
          <FacetPill
            key={genre.name}
            facet={genre}
            onClick={() => {
              const next = new URLSearchParams(params);
              next.set('genre', genre.name);
              setParams(next);
            }}
          />
        ))}
      </div>
    </section>
  );
}

function CountryShelves() {
  const facets = facetsQuery();
  const [params, setParams] = useSearchParams();
  const countries = (facets.data?.countries ?? []).slice(0, 14);
  const languages = (facets.data?.languages ?? []).slice(0, 10);
  if (!countries.length && !languages.length) return null;
  return (
    <section className={railClass()} aria-label="Countries and languages">
      <SectionHeader title="Around the World" caption="Live radio by country and language" />
      <div className="flex flex-wrap gap-2">
        {countries.map((country) => (
          <FacetPill
            key={country.code || country.name}
            facet={country}
            onClick={() => {
              const next = new URLSearchParams(params);
              next.set('country', country.code || country.name);
              setParams(next);
            }}
          />
        ))}
        {languages.map((language) => (
          <FacetPill
            key={`lang-${language.name}`}
            facet={language}
            onClick={() => {
              const next = new URLSearchParams(params);
              next.set('language', language.name);
              setParams(next);
            }}
          />
        ))}
      </div>
    </section>
  );
}

function FacetPill({ facet, onClick }: { facet: RadioFacet; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="flex items-center gap-2 rounded-pill bg-quinary px-3.5 py-1.5 type-callout text-primary transition-colors hover:bg-quaternary"
    >
      {facet.name}
      <span className="type-caption tabular-nums text-secondary">{facet.count.toLocaleString()}</span>
    </button>
  );
}

/** A station tile: favicon art, name, country/tags, play on hover/menu. */
function StationCard({ station, onRemove }: { station: RadioStation; onRemove?: () => void }) {
  const art = useStationArt(station.favicon);
  const playingUrl = usePlayer((s) => s.nowPlaying?.videoId);
  const status = usePlayer((s) => s.status);
  const isCurrent = playingUrl === station.url;
  const menuButton = useRef<HTMLButtonElement>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const [infoOpen, setInfoOpen] = useState(false);
  const favourite = useRadio((s) => s.favourites.some((f) => f.id === station.id));
  const toggleFavourite = useRadio((s) => s.toggleFavourite);

  return (
    <div className="group block select-none">
      <div className="relative">
        <button
          className="relative block aspect-square w-full overflow-hidden rounded-[var(--radius-artwork-large)] bg-joe artwork-stroke"
          aria-label={`Play ${station.name}, live`}
          onClick={() => void playStation(station)}
        >
          {art ? (
            <img src={art} alt="" className="size-full object-cover" referrerPolicy="no-referrer" decoding="async" />
          ) : (
            <StationFallback name={station.name} />
          )}
          <span className="pointer-events-none absolute inset-x-0 top-0 z-1 flex items-center gap-1 bg-gradient-to-b from-black/60 to-transparent px-2 py-1.5">
            <span className="size-1.5 rounded-full bg-[#ff453a] shadow-[0_0_6px_#ff453a]" aria-hidden />
            <span className="type-[600_9px/11px] tracking-[0.08em] text-white">LIVE</span>
          </span>
          <span
            className={`pointer-events-none absolute right-2 bottom-2 z-2 grid size-8 place-items-center rounded-full bg-black/45 text-white backdrop-blur-sm transition-opacity duration-150 ${
              isCurrent ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'
            }`}
          >
            {(isCurrent && status === 'playing') || status === 'buffering' ? (
              <span className="flex items-end gap-[2px]" aria-hidden>
                <span className="h-2 w-[2px] animate-pulse bg-white" />
                <span className="h-3 w-[2px] animate-pulse bg-white [animation-delay:120ms]" />
                <span className="h-1.5 w-[2px] animate-pulse bg-white [animation-delay:240ms]" />
              </span>
            ) : (
              <PlayIcon size={15} />
            )}
          </span>
        </button>
        <button
          ref={menuButton}
          aria-label={`More for ${station.name}`}
          aria-haspopup="menu"
          aria-expanded={menuOpen}
          onClick={() => setMenuOpen((v) => !v)}
          className="absolute top-1.5 right-1.5 z-3 grid size-7 place-items-center rounded-full bg-black/35 text-white opacity-0 backdrop-blur-sm transition-opacity duration-150 group-hover:opacity-100 focus-visible:opacity-100 data-open:opacity-100"
          data-open={menuOpen || undefined}
        >
          <Ellipsis size={15} strokeWidth={2.4} />
        </button>
        {menuOpen && menuButton.current && (
          <Menu anchor={menuButton.current} placement="below-end" onClose={() => setMenuOpen(false)} label={`Station menu for ${station.name}`}>
            <MenuHeader>
              {station.name}
              {station.country ? ` · ${station.country}` : ''}
            </MenuHeader>
            <MenuItem icon={<Play size={14} />} onSelect={() => { setMenuOpen(false); void playStation(station); }}>
              Play Station
            </MenuItem>
            {onRemove ? (
              <MenuItem destructive icon={<Heart size={14} />} onSelect={onRemove}>
                Remove from Favourites
              </MenuItem>
            ) : (
              <MenuItem icon={<Heart size={14} />} checked={favourite} onSelect={() => toggleFavourite(station)}>
                {favourite ? 'Remove from Favourites' : 'Add to Favourites'}
              </MenuItem>
            )}
            <MenuSeparator />
            <MenuItem icon={<Info size={14} />} onSelect={() => { setMenuOpen(false); setInfoOpen(true); }}>
              Station Info
            </MenuItem>
          </Menu>
        )}
        {infoOpen && <StationInfoDialog station={station} onClose={() => setInfoOpen(false)} />}
      </div>
      <p className="mt-2 truncate type-body-emphasized text-primary">{station.name}</p>
      <p className="truncate type-callout text-secondary">
        {isCurrent && status !== 'idle' ? 'On air now' : stationDetail(station) || 'Live radio'}
      </p>
    </div>
  );
}

/** Deterministic monogram for stations whose favicon is missing or broken. */
function StationFallback({ name }: { name: string }) {
  let hue = 0;
  for (const ch of name) hue = (hue * 31 + ch.charCodeAt(0)) % 360;
  return (
    <span
      className="grid size-full place-items-center text-white/90"
      style={{ background: `linear-gradient(145deg, hsl(${hue} 42% 34%), hsl(${(hue + 40) % 360} 48% 18%))` }}
    >
      <RadioWavesIcon size={44} />
    </span>
  );
}

function useStationArt(favicon: string): string | null {
  const query = useQuery({
    queryKey: ['radio-art', favicon],
    queryFn: () => radioArtwork(favicon),
    enabled: favicon.startsWith('https://'),
    staleTime: Infinity,
    retry: false,
  });
  return query.data ?? null;
}

/** Strictly the station's own record: no invented now-playing track. */
function StationInfoDialog({ station, onClose }: { station: RadioStation; onClose: () => void }) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey, true);
    return () => document.removeEventListener('keydown', onKey, true);
  }, [onClose]);
  const rows: Array<[string, string]> = [
    ['Station', station.name],
    ['Country', station.country || '—'],
    ['Language', station.language || '—'],
    ['Tags', station.tags || '—'],
    ['Stream', stationQuality(station)],
    ['Homepage', station.homepage || '—'],
  ];
  return (
    <>
      <div className="fixed inset-0 z-(--z-modal) bg-scrim" onPointerDown={onClose} aria-hidden="true" />
      <div
        role="dialog"
        aria-modal="true"
        aria-label={`Station Info for ${station.name}`}
        className="glass-edge fixed top-1/2 left-1/2 z-[calc(var(--z-modal)+1)] w-[380px] max-w-[calc(100vw-32px)] -translate-x-1/2 -translate-y-1/2 rounded-[12px] bg-context-menu p-5 text-primary shadow-[0_0_0_0.5px_var(--vibrantDivider),0_18px_60px_var(--glassMaterialShadowColor)] [backdrop-filter:var(--dynamicBlurMenu)]"
      >
        <header className="flex items-start justify-between gap-4">
          <div>
            <h2 className="type-title-3-emphasized">Station Info</h2>
            <p className="mt-1 type-callout text-secondary">Live broadcast · metadata from Radio Browser</p>
          </div>
          <button
            type="button"
            aria-label="Close"
            onClick={onClose}
            className="grid size-6 place-items-center rounded-full text-secondary hover:bg-quinary hover:text-primary"
          >
            <X size={15} strokeWidth={2.4} />
          </button>
        </header>
        <dl className="mt-4 grid grid-cols-[76px_1fr] gap-x-3 gap-y-2 type-body">
          {rows.map(([label, value]) => (
            <div key={label} className="contents">
              <dt className="text-secondary">{label}</dt>
              <dd className="min-w-0 break-words text-primary">{value}</dd>
            </div>
          ))}
        </dl>
        <footer className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            onClick={() => {
              void playStation(station);
              onClose();
            }}
            className="inline-flex h-6 items-center rounded-pill bg-key px-3.5 type-body-emphasized text-white hover:opacity-90"
          >
            Play
          </button>
          <button
            type="button"
            onClick={onClose}
            className="inline-flex h-6 items-center rounded-pill bg-quaternary px-3.5 type-body-emphasized text-primary hover:bg-gray-4"
          >
            Done
          </button>
        </footer>
      </div>
    </>
  );
}

function CustomStream() {
  const [url, setUrl] = useState('');
  const [name, setName] = useState('');
  const [error, setError] = useState<string | null>(null);
  const custom = useRadio((s) => s.custom);
  const addCustom = useRadio((s) => s.addCustom);
  const removeCustom = useRadio((s) => s.removeCustom);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    const invalid = validateStreamUrl(url);
    if (invalid) {
      setError(invalid);
      return;
    }
    setError(null);
    addCustom(name, url.trim());
    await playCustomStation(name, url.trim());
    setUrl('');
    setName('');
  };

  return (
    <section className={railClass()} aria-label="Custom stream">
      <SectionHeader title="Add a Station" caption="A direct MP3/AAC stream URL" />
      <form onSubmit={(e) => void submit(e)} className="flex max-w-2xl flex-wrap items-center gap-2">
        <input
          value={name}
          onChange={(e) => setName(e.currentTarget.value)}
          placeholder="Name (optional)"
          aria-label="Station name"
          className="w-40 rounded-[10px] border-0 bg-quinary px-3 py-2 type-body text-primary outline-none placeholder:text-tertiary"
        />
        <input
          value={url}
          onChange={(e) => {
            setUrl(e.currentTarget.value);
            setError(null);
          }}
          placeholder="https://stream.example.com/live.mp3"
          aria-label="Stream URL"
          className="min-w-0 flex-1 rounded-[10px] border-0 bg-quinary px-3 py-2 type-body text-primary outline-none placeholder:text-tertiary"
        />
        <button type="submit" className="flex items-center gap-1.5 rounded-pill bg-key px-3.5 py-2 type-callout-emphasized text-white hover:opacity-90">
          <Plus size={13} /> Add &amp; Play
        </button>
      </form>
      {error && <p className="mt-2 type-callout text-key">{error}</p>}
      {custom.length > 0 && (
        <ul className="mt-4 flex max-w-2xl flex-wrap gap-2">
          {custom.map((station) => (
            <li key={station.id}>
              <div className="flex items-center gap-1 rounded-pill bg-quinary pr-1 pl-3">
                <button
                  className="type-callout text-primary hover:underline"
                  onClick={() => void playCustomStation(station.name, station.url)}
                >
                  {station.name}
                </button>
                <button
                  aria-label={`Remove ${station.name}`}
                  onClick={() => removeCustom(station.id)}
                  className="grid size-5 place-items-center rounded-full text-secondary hover:bg-quaternary hover:text-primary"
                >
                  <X size={11} />
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

// Filtered results

function StationResults({ search, country, genre, language }: { search: string; country: string; genre: string; language: string }) {
  const [params, setParams] = useSearchParams();
  const query = useInfiniteQuery({
    queryKey: ['radio', 'results', search, country, genre, language],
    initialPageParam: 0,
    queryFn: ({ pageParam }) =>
      radioStations({
        search,
        countryCode: country,
        tag: genre,
        language,
        order: 'clickcount',
        offset: pageParam,
      }),
    getNextPageParam: (last, pages) => (last.length < RADIO_PAGE_SIZE ? undefined : pages.length * RADIO_PAGE_SIZE),
    staleTime: 5 * 60_000,
    retry: 1,
  });
  const stations = query.data?.pages.flat() ?? [];
  const clear = (key: string) => {
    const next = new URLSearchParams(params);
    next.delete(key);
    setParams(next);
  };
  return (
    <section className="mt-7" aria-label="Station results">
      <div className="mx-(--body-gutter) flex flex-wrap items-center gap-2">
        <h2 className="mr-2 type-title-2-emphasized">
          {search ? `Stations matching “${search}”` : 'Live Stations'}
        </h2>
        {country && <FilterChip label={country} onClear={() => clear('country')} />}
        {genre && <FilterChip label={genre} onClear={() => clear('genre')} />}
        {language && <FilterChip label={language} onClear={() => clear('language')} />}
        {(country || genre || language) && (
          <button
            onClick={() => setParams(new URLSearchParams(search ? { q: search } : {}))}
            className="type-callout text-secondary hover:text-primary"
          >
            Clear all
          </button>
        )}
      </div>

      {query.isPending && (
        <div className={gridClass}>
          {Array.from({ length: 12 }).map((_, i) => (
            <SkeletonBlock key={i} width="100%" height={150} radius={10} />
          ))}
        </div>
      )}

      {query.isError && (
        <div className="mx-(--body-gutter) mt-6 max-w-lg">
          <p className="type-body text-secondary">
            The station directory could not be reached: {String(query.error).replace(/^Error:\s*/, '')}
          </p>
          <button
            className="mt-3 rounded-pill bg-quinary px-4 py-2 type-callout-emphasized text-primary hover:bg-quaternary"
            onClick={() => void query.refetch()}
          >
            Try again
          </button>
        </div>
      )}

      {query.data && stations.length === 0 && (
        <p className="mx-(--body-gutter) mt-6 max-w-lg type-body text-secondary">
          No playable stations match these filters. Try a broader genre or another country.
        </p>
      )}

      {stations.length > 0 && (
        <div className={gridClass}>
          {stations.map((station) => (
            <StationCard key={station.id} station={station} />
          ))}
        </div>
      )}

      {query.hasNextPage && (
        <div className="mx-(--body-gutter) mt-7">
          <button
            onClick={() => void query.fetchNextPage()}
            disabled={query.isFetchingNextPage}
            className="rounded-pill bg-quinary px-4 py-2 type-callout-emphasized text-primary hover:bg-quaternary disabled:opacity-50"
          >
            {query.isFetchingNextPage ? 'Loading…' : 'Load More Stations'}
          </button>
        </div>
      )}
    </section>
  );
}

function FilterChip({ label, onClear }: { label: string; onClear: () => void }) {
  return (
    <span className="flex items-center gap-1 rounded-pill bg-key px-2.5 py-1 type-callout-emphasized text-white">
      {label}
      <button onClick={onClear} aria-label={`Remove ${label} filter`} className="grid size-4 place-items-center rounded-full hover:bg-white/20">
        <X size={10} strokeWidth={3} />
      </button>
    </span>
  );
}

// Personalized radio (not a broadcast)

interface HomeItem {
  kind: string;
  id: string;
  title: string;
  subtitle: string;
  thumbnail_url: string | null;
  duration_sec: number | null;
  is_video: boolean;
}

interface HomeFeed {
  personalized: boolean;
  plays: number;
  top_artists: string[];
  featured: HomeItem[];
  sections: Array<{ id: string; title: string; items: HomeItem[] }>;
  catalogue_error: string | null;
}

interface LikedRef {
  key_source: string;
  key_id: string;
}

/**
 * A continuous queue from the listening profile — liked songs, followed
 * artists and listening history feed the same recommendation service Home
 * uses, and the station extends itself with related catalogue tracks. It is
 * explicitly not a broadcast station: nothing here is live.
 */
function PersonalStation() {
  const home = useQuery({
    queryKey: ['radio', 'personal-feed'],
    queryFn: () => invoke<HomeFeed>('home_feed', { refresh: false }),
    staleTime: 5 * 60_000,
    retry: false,
  });
  const liked = useQuery({ queryKey: ['radio', 'liked'], queryFn: () => invoke<LikedRef[]>('liked_tracks_get'), retry: false });
  const followed = useQuery({
    queryKey: ['radio', 'followed'],
    queryFn: () => invoke<Array<[LikedRef, string]>>('followed_artists_get'),
    retry: false,
  });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const items = [
    ...(home.data?.featured ?? []),
    ...(home.data?.sections ?? []).flatMap((section) => section.items),
  ].filter((item) => item.kind === 'song' || item.kind === 'video');

  const signals = liked.data?.length ?? 0;
  const artists = followed.data?.length ?? 0;
  const plays = home.data?.plays ?? 0;

  const start = async () => {
    if (!items.length) {
      setError('No catalogue songs are available yet. Like or play a few songs first.');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const seen = new Set<string>();
      const seeds: HomeItem[] = [];
      for (const item of items) {
        if (seen.has(item.id)) continue;
        seen.add(item.id);
        seeds.push(item);
        if (seeds.length >= 3) break;
      }
      const related = await Promise.allSettled(
        seeds.map((seed) => invoke<DiscoveryTrack[]>('discovery_radio', { videoId: seed.id })),
      );
      const tracks = new Map<string, ReturnType<typeof discoveryToTrack>>();
      for (const result of related) {
        if (result.status !== 'fulfilled') continue;
        for (const row of result.value) tracks.set(row.key.id, discoveryToTrack(row));
      }
      for (const item of items) {
        if (tracks.size >= 60) break;
        if (tracks.has(item.id)) continue;
        tracks.set(item.id, {
          videoId: item.id,
          title: item.title,
          artists: [{ name: item.subtitle || 'Unknown Artist', id: null }],
          album: null,
          durationSec: item.duration_sec,
          explicit: false,
          thumbnail: item.thumbnail_url,
          index: null,
          plays: null,
          sourceKind: 'youtube',
          artworkUrl: item.thumbnail_url,
        });
      }
      await usePlayer.getState().playTracks([...tracks.values()], 0, null);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ''));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className={railClass()} aria-label="Personalized radio">
      <div className="relative overflow-hidden rounded-[var(--radius-artwork-large)] border-[0.5px] border-[var(--vibrantDivider)] bg-[linear-gradient(120deg,#3b2a5e_0%,#5d3b6e_45%,#8a4a55_100%)] p-5 text-white">
        <div className="pointer-events-none absolute -top-16 -right-10 size-56 rounded-full bg-white/10 blur-2xl" aria-hidden />
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div className="min-w-0">
            <p className="flex items-center gap-1.5 type-callout-emphasized uppercase tracking-[0.08em] text-white/70">
              <Sparkles size={13} /> Personalized
            </p>
            <h2 className="mt-1 type-title-1-emphasized">Your Station</h2>
            <p className="mt-1 max-w-xl type-callout text-white/80">
              {plays > 0 || signals > 0 || artists > 0
                ? `Built from ${plays} ${plays === 1 ? 'play' : 'plays'}, ${signals} liked ${signals === 1 ? 'song' : 'songs'} and ${artists} followed ${artists === 1 ? 'artist' : 'artists'}. Not a live broadcast.`
                : 'Like songs, follow artists or just listen for a while — this station grows from your own signals. It is not a live broadcast.'}
            </p>
          </div>
          <button
            onClick={() => void start()}
            disabled={busy || home.isPending}
            className="flex flex-none items-center gap-2 rounded-pill bg-white px-4 py-2 type-body-emphasized text-black shadow-sm transition-opacity hover:opacity-90 disabled:opacity-60"
          >
            <ListMusic size={15} /> {busy ? 'Building…' : 'Start Station'}
          </button>
        </div>
        {error && <p className="mt-3 type-callout text-white/85">{error}</p>}
        {home.data && !home.data.personalized && plays === 0 && signals === 0 && (
          <p className="mt-2 type-caption text-white/60">Currently showing general catalogue picks until your profile builds up.</p>
        )}
      </div>
    </section>
  );
}
