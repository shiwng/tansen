import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { useNavigate, useParams } from 'react-router';
import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { FolderPlus, Loader2, RefreshCw, SquarePen } from 'lucide-react';
import { Artwork } from '@/components/music/Artwork';
import { TrackList } from '@/components/music/TrackList';
import { trackFromRef } from '@/state/player';
import { q, rowsToTracks, sourceFor } from '@/lib/rows';
import type { ArtistRow } from '@/lib/rows';
import { EmptyLibrary } from './HomePage';
import type { Folder, ScanProgress } from '@/types';

/**
 * The library sections: Songs, Albums, Artists, Playlists, Recently Played.
 * Albums and Artists render as artwork grids, like the original's library
 * pages; Songs is the full track list.
 */
export function LibraryPage() {
  const { section = 'songs' } = useParams<{ section: string }>();
  return (
    <div>
      {section === 'songs' && <SongsSection />}
      {section === 'albums' && <AlbumsSection />}
      {section === 'artists' && <ArtistsSection />}
      {section === 'playlists' && <PlaylistsSection />}
      {section === 'recent' && <RecentSection />}
    </div>
  );
}

function Header({ title, children }: { title: string; children?: React.ReactNode }) {
  return (
    <header className="mx-(--body-gutter) flex flex-wrap items-center gap-3 pt-11 pb-5">
      <h1 className="type-header-emphasized">{title}</h1>
      <div className="ml-auto flex items-center gap-2">{children}</div>
    </header>
  );
}

function ScanButtons() {
  const client = useQueryClient();
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const [scanning, setScanning] = useState(false);
  const [summary, setSummary] = useState<string | null>(null);

  // Progress events arrive while a scan runs; the queries refresh when it ends.
  useScanEvents((scanning, progress, added, errors) => {
    setScanning(scanning);
    setProgress(progress);
    if (added !== null) {
      setSummary(added !== null && (added > 0 || (errors ?? 0) > 0) ? `Last scan: +${added} new${errors ? `, ${errors} failed` : ''}` : null);
      void client.invalidateQueries({ queryKey: ['local'] });
    }
  });

  return (
    <>
      {scanning && progress && (
        <span className="flex items-center gap-1.5 type-callout text-secondary">
          <Loader2 size={12} className="animate-spin" />
          Scanning {progress.scanned}/{progress.total || '…'}
        </span>
      )}
      {!scanning && summary && <span className="type-callout text-secondary">{summary}</span>}
      <button
        onClick={() => void invoke('scan_start')}
        disabled={scanning}
        className="flex items-center gap-1.5 rounded-pill bg-quinary px-3.5 py-2 type-callout-emphasized text-primary hover:bg-quaternary disabled:opacity-50"
      >
        {scanning ? <Loader2 size={13} className="animate-spin" /> : <RefreshCw size={13} />}
        Rescan
      </button>
      <button
        onClick={async () => {
          const sel = await open({ directory: true, multiple: false, title: 'Choose a music folder' });
          if (typeof sel === 'string') void invoke('folder_add', { path: sel }).then(() => invoke('scan_start'));
        }}
        disabled={scanning}
        className="flex items-center gap-1.5 rounded-pill bg-key px-3.5 py-2 type-callout-emphasized text-white hover:opacity-90 disabled:opacity-50"
      >
        <FolderPlus size={13} />
        Import folder
      </button>
    </>
  );
}

import { useEffect } from 'react';

function useScanEvents(
  on: (scanning: boolean, progress: ScanProgress | null, added: number | null, errors: number | null) => void,
) {
  useEffect(() => {
    void listen<ScanProgress>('scan://progress', (e) => on(true, e.payload, null, null));
    void listen<ScanSummaryDone>('scan://done', (e) => {
      on(false, null, e.payload.added, e.payload.errors);
    });
    // Listeners live for the page's life; the callbacks capture fresh setters.
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
}

interface ScanSummaryDone {
  added: number;
  updated: number;
  marked_missing: number;
  errors: number;
}

function SongsSection() {
  const tracks = useQuery(q.tracks());
  const source = sourceFor('library', 'Songs');
  return (
    <div>
      <Header title="Songs">
        <ScanButtons />
      </Header>
      {tracks.isError && <p className="mx-(--body-gutter) type-body text-secondary">{String(tracks.error)}</p>}
      {tracks.isPending && null}
      {tracks.data && tracks.data.length > 0 && (
        <div className="mx-(--body-gutter) mb-10">
          <TrackList tracks={rowsToTracks(tracks.data)} variant="playlist" source={source} />
        </div>
      )}
      {tracks.data && tracks.data.length === 0 && (
        <div className="mx-(--body-gutter)">
          <EmptyLibrary />
          <FoldersCard />
        </div>
      )}
    </div>
  );
}

function FoldersCard() {
  const folders = useQuery({ queryKey: ['local', 'folders'], queryFn: () => invoke<Folder[]>('folders_get') });
  const client = useQueryClient();
  const remove = useMutation({
    mutationFn: (id: number) => invoke('folder_remove', { id }),
    onSuccess: () => void client.invalidateQueries({ queryKey: ['local', 'folders'] }),
  });
  return (
    <section className="mb-10 max-w-2xl">
      <h2 className="mb-2 type-title-3-emphasized text-secondary">Indexed folders</h2>
      <ul className="overflow-hidden rounded-[10px] bg-quinary">
        {folders.data?.map((f) => (
          <li key={f.id} className="flex items-center gap-2 py-2 pr-2.5 pl-3 type-callout [&+&]:border-t-[0.5px] [&+&]:border-t-divider">
            <span className="min-w-0 flex-1 truncate">{f.path}</span>
            <button className="text-secondary hover:text-primary" onClick={() => remove.mutate(f.id)} aria-label="Remove folder">
              ✕
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

function AlbumsSection() {
  const albums = useQuery(q.albums());
  return (
    <div>
      <Header title="Albums">
        <ScanButtons />
      </Header>
      {albums.isError && <p className="mx-(--body-gutter) type-body text-secondary">{String(albums.error)}</p>}
      {albums.data && (
        <Grid>
          {albums.data.map((a) => (
            <Tile key={a.key} to={`/album/${encodeURIComponent(a.key)}`} art={a.art_path} title={a.album} subtitle={a.album_artist || 'Unknown Artist'} />
          ))}
        </Grid>
      )}
      {albums.data && albums.data.length === 0 && (
        <div className="mx-(--body-gutter)">
          <EmptyLibrary />
          <FoldersCard />
        </div>
      )}
    </div>
  );
}

function ArtistsSection() {
  const artists = useQuery(q.artists());
  return (
    <div>
      <Header title="Artists">
        <ScanButtons />
      </Header>
      {artists.isError && <p className="mx-(--body-gutter) type-body text-secondary">{String(artists.error)}</p>}
      {artists.data && (
        <Grid>
          {artists.data.map((a: ArtistRow) => (
            <Tile key={a.name} to={`/artist/${encodeURIComponent(a.name)}`} art={a.art_path} title={a.name} subtitle={`${a.album_count} albums • ${a.track_count} songs`} person />
          ))}
        </Grid>
      )}
      {artists.data && artists.data.length === 0 && (
        <div className="mx-(--body-gutter)">
          <EmptyLibrary />
        </div>
      )}
    </div>
  );
}

function RecentSection() {
  const history = useQuery(q.history(200));
  const source = sourceFor('library', 'Recently Played');
  return (
    <div>
      <Header title="Recently Played" />
      {history.isError && <p className="mx-(--body-gutter) type-body text-secondary">{String(history.error)}</p>}
      {history.data && history.data.length > 0 && (
        <div className="mx-(--body-gutter) mb-10">
          <TrackList
            tracks={history.data.map((h) => trackFromRef(h.track))}
            variant="playlist"
            source={source}
          />
        </div>
      )}
      {history.data && history.data.length === 0 && (
        <p className="mx-(--body-gutter) pb-10 type-body text-secondary">Nothing played yet.</p>
      )}
    </div>
  );
}

function PlaylistsSection() {
  const client = useQueryClient();
  const navigate = useNavigate();
  const playlists = useQuery({ queryKey: ['local', 'playlists'], queryFn: () => invoke<[number, string][]>('playlists_get') });
  const create = useMutation({
    mutationFn: (name: string) => invoke<number>('playlist_create', { name }),
    onSuccess: (id) => {
      void client.invalidateQueries({ queryKey: ['local', 'playlists'] });
      navigate(`/playlist/${id}`);
    },
  });
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState('');

  return (
    <div>
      <Header title="Playlists">
        <button
          onClick={() => setCreating(true)}
          className="flex items-center gap-1.5 rounded-pill bg-key px-3.5 py-2 type-callout-emphasized text-white hover:opacity-90"
        >
          <SquarePen size={13} />
          New playlist
        </button>
      </Header>
      {creating && (
        <form
          className="mx-(--body-gutter) mb-6 flex max-w-md items-center gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            create.mutate(name.trim() || 'New Playlist');
            setCreating(false);
            setName('');
          }}
        >
          <input
            autoFocus
            value={name}
            onChange={(e) => setName(e.currentTarget.value)}
            placeholder="Playlist name"
            className="min-w-0 flex-1 rounded-[10px] border-0 bg-quinary px-3 py-2 type-body text-primary outline-none"
            aria-label="Playlist name"
          />
          <button type="submit" className="rounded-pill bg-key px-4 py-2 type-callout-emphasized text-white">
            Create
          </button>
          <button type="button" className="type-callout-emphasized text-secondary" onClick={() => setCreating(false)}>
            Cancel
          </button>
        </form>
      )}
      {playlists.data && playlists.data.length > 0 && (
        <Grid>
          {playlists.data.map(([id, name]) => (
            <Tile key={id} to={`/playlist/${id}`} art={null} title={name} subtitle="Playlist" />
          ))}
        </Grid>
      )}
      {playlists.data && playlists.data.length === 0 && !creating && (
        <p className="mx-(--body-gutter) pb-10 type-body text-secondary">No playlists yet — create one above.</p>
      )}
    </div>
  );
}

const grid = 'mx-(--body-gutter) mb-12 grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-x-6 gap-y-7 max-md:grid-cols-[repeat(auto-fill,minmax(124px,1fr))]';

function Grid({ children }: { children: React.ReactNode }) {
  return <div className={grid}>{children}</div>;
}

function Tile({ to, art, title, subtitle, person }: { to: string; art: string | null; title: string; subtitle: string; person?: boolean }) {
  const navigate = useNavigate();
  return (
    <a href={to} onClick={(e) => { e.preventDefault(); navigate(to); }} className="group block">
      <div className="relative">
        <Artwork
          src={art}
          displayWidth={160}
          radius={person ? '9999px' : 'var(--radius-artwork-large)'}
          className={`aspect-square w-full ${person ? 'rounded-full' : ''}`}
        />
      </div>
      <p className="mt-2 truncate type-body-emphasized text-primary">{title}</p>
      <p className="truncate type-callout text-secondary">{subtitle}</p>
    </a>
  );
}
