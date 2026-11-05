import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { AppError } from '@/shell/AppError';
import { LyricsSettings } from '@/shell/LyricsSettings';
import { usePlayer } from '@/state/player';
import { open } from '@tauri-apps/plugin-dialog';
import { q } from '@/lib/rows';


/**
 * Settings, in the original's shape: Playback, Library, Lyrics, Integrations,
 * About. Every control is wired to real state; features that don't exist yet
 * say so instead of pretending.
 */
export function SettingsPage() {
  return (
    <div>
      <h1 className="mx-(--body-gutter) pt-11 type-header-emphasized">Settings</h1>
      {/* One broken section must not blank the page: each is its own boundary. */}
      <div className="mx-(--body-gutter) mb-12 flex max-w-3xl flex-col gap-8">
        <AppError inline><PlaybackSection /></AppError>
        <AppError inline><LibrarySection /></AppError>
        <AppError inline><LyricsSection /></AppError>
        <AppError inline><SourcesSection /></AppError>
        <AppError inline><AboutSection /></AppError>
      </div>
    </div>
  );
}

function PlaybackSection() {
  const player = usePlayer();
  return (
    <section>
      <h2 className="mb-3 type-title-2-emphasized">Playback</h2>
      <div className="flex flex-col gap-2">
        <Row label="Repeat" hint="Cycles off, all, one — the pill's repeat button does the same">
          <button className={player_settings_button} onClick={player.cycleRepeat}>
            <span className="capitalize">{player.repeat}</span>
          </button>
        </Row>
        <Row label="Shuffle" hint="Shuffles the next selection when you press play on a list">
          <input type="checkbox" aria-label="Shuffle" className={player_settings_checkbox} checked={player.shuffle} onChange={player.toggleShuffle} />
        </Row>
        <QualityRow />
        <AutoplayRow />
        <Row label="Gapless playback" hint="Always on for local files">
          <span className="type-callout text-secondary">Always on</span>
        </Row>
      </div>
    </section>
  );
}

/** S11: autoplay related tracks when the queue tail plays (default off). */
function AutoplayRow() {
  const [on, setOn] = useState(false);
  useEffect(() => {
    void invoke<boolean>('autoplay_get').then(setOn);
  }, []);
  return (
    <Row label="Autoplay" hint="Adds related tracks when the queue ends (catalogue sources)">
      <input
        type="checkbox"
        className={player_settings_checkbox}
        checked={on}
        onChange={(e) => {
          const next = e.currentTarget.checked;
          setOn(next);
          void invoke('autoplay_set', { on: next }).catch(() => setOn(!next));
        }}
      />
    </Row>
  );
}

/** Audio ceiling: maps 1:1 onto the catalogue's StreamRequest. */
function QualityRow() {
  const [quality, setQuality] = useState<string | null>(null);
  useEffect(() => {
    void invoke<string>('quality_get').then(setQuality);
  }, []);
  return (
    <Row label="Audio quality" hint="Data-saver caps streams at 64 kbps; Lossless requests bit-exact copies">
      <select
        className={player_settings_button}
        value={quality ?? 'best'}
        onChange={(e) => {
          const v = e.currentTarget.value;
          setQuality(v);
          void invoke('quality_set', { quality: v }).catch(() => setQuality('best'));
        }}
      >
        <option value="data-saver">Data saver</option>
        <option value="best">Best</option>
        <option value="lossless">Lossless</option>
      </select>
    </Row>
  );
}

function LibrarySection() {
  const queryClient = useQueryClient();
  const folders = useQuery({ queryKey: ['local', 'folders'], queryFn: () => invoke<{ id: number; path: string }[]>('folders_get') });
  const albums = useQuery(q.albums());
  const [watch, setWatch] = useState<boolean | null>(null);
  useEffect(() => {
    void invoke<[string, string][]>('settings_get').then((rows) => {
      const value = rows.find(([k]) => k === 'watch_folders')?.[1];
      setWatch(value !== 'false'); // default on
    });
  }, []);
  return (
    <section>
      <h2 className="mb-3 type-title-2-emphasized">Library</h2>
      <p className="mb-2 type-callout text-secondary">
        {albums.data ? `${albums.data.length} albums indexed` : '…'}
      </p>
      <div className="flex flex-col gap-2">
        <Row
          label="Add folder"
          hint="Indexes a folder into the library — the same import as the Library page"
        >
          <button
            className={player_settings_button}
            onClick={async () => {
              const sel = await open({ directory: true, multiple: false, title: 'Choose a music folder' });
              if (typeof sel === 'string') {
                await invoke('folder_add', { path: sel });
                await invoke('scan_start');
                void queryClient.invalidateQueries({ queryKey: ['local', 'folders'] });
              }
            }}
          >
            Choose…
          </button>
        </Row>
        <Row label="Watch folders" hint="Re-index automatically when files change on disk">
          <input
            type="checkbox"
            aria-label="Watch folders"
            className={player_settings_checkbox}
            checked={watch ?? true}
            onChange={(e) => {
              const on = e.currentTarget.checked;
              setWatch(on);
              void invoke('settings_set', { key: 'watch_folders', value: on ? 'true' : 'false' });
            }}
          />
        </Row>
        {folders.data?.map((f) => (
          <div key={f.id} className="flex items-center gap-3 rounded-[10px] bg-quinary px-3 py-2">
            <span className="min-w-0 flex-1 truncate type-callout">{f.path}</span>
            <button
              className="type-callout-emphasized text-key"
              onClick={() => void invoke('folder_remove', { id: f.id }).then(() => invoke('scan_start'))}
            >
              Remove & rescan
            </button>
          </div>
        ))}
      </div>
    </section>
  );
}

function LyricsSection() {
  const [open, setOpen] = useState(false);
  return (
    <section>
      <h2 className="mb-3 type-title-2-emphasized">Lyrics</h2>
      <div className="flex flex-col gap-2">
        <Row label="Providers" hint="Enable, order, and time the lyric sources">
          <button className={player_settings_button} onClick={() => setOpen(true)}>
            Configure…
          </button>
        </Row>
      </div>
      {open && <LyricsSettings onClose={() => setOpen(false)} />}
    </section>
  );
}

function SourcesSection() {
  const client = useQueryClient();
  const sources = useQuery({
    queryKey: ['sources'],
    queryFn: () => invoke<SourceRow[]>('sources_get_with_health'),
    refetchInterval: false,
  });
  const [url, setUrl] = useState('');
  const [error, setError] = useState<string | null>(null);
  const add = useMutation({
    mutationFn: (base_url: string) => invoke<SourceConfig>('source_add', { new: { base_url } }),
    onSuccess: () => {
      setUrl('');
      setError(null);
      void client.invalidateQueries({ queryKey: ['sources'] });
    },
    onError: (e) => setError(String(e)),
  });
  // S9 Subsonic: the password lives only in this transient form field until
  // submit hands it to the backend, which stores it in the macOS Keychain.
  // It is never written to settings, the query cache, or local storage.
  const [subUrl, setSubUrl] = useState('');
  const [subUser, setSubUser] = useState('');
  const [subPass, setSubPass] = useState('');
  const [subAllowHttp, setSubAllowHttp] = useState(false);
  const [subError, setSubError] = useState<string | null>(null);
  const addSubsonic = useMutation({
    mutationFn: () =>
      invoke<SourceConfig>('subsonic_add', {
        baseUrl: subUrl.trim(),
        username: subUser.trim(),
        password: subPass,
        allowHttp: subAllowHttp,
      }),
    onSuccess: () => {
      setSubUrl('');
      setSubUser('');
      setSubPass('');
      setSubAllowHttp(false);
      setSubError(null);
      void client.invalidateQueries({ queryKey: ['sources'] });
    },
    onError: (e) => setSubError(String(e)),
  });

  return (
    <section>
      <h2 className="mb-3 type-title-2-emphasized">Sources</h2>
      <p className="mb-2 type-callout text-secondary">
        Addon servers speak the original's manifest/search/stream protocol. Search and playback through them land with
        the phase-4 resolver; management and health are live now.
      </p>
      <div className="flex flex-col gap-2">
        <CatalogueToggle />
        {sources.data?.map((row, i, all) => {
          const { config_id, kind, name, base_url, enabled, health } = row;
          // Health is optional on the wire: a cache entry written by a
          // health-less caller, or a backend that could not probe, is unknown.
          const healthState = health?.state ?? null;
          return (
          <div key={config_id} className="flex items-center gap-3 rounded-[10px] bg-quinary px-3 py-2.5">
            <span
              className="size-2 flex-none rounded-full"
              style={{ background: healthState === 'ok' ? 'var(--healthDot)' : healthState === 'rejected' ? 'var(--systemRed)' : 'var(--systemGray)' }}
              title={healthState ?? 'Status unavailable'}
            />
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2 type-body">
                <span className="truncate">{name}</span>
                {!enabled && <span className="type-callout text-secondary">(off)</span>}
              </div>
              <div className="truncate type-callout text-secondary">
                {healthState === 'ok'
                  ? base_url
                  : healthState === 'unreachable'
                    ? `Unreachable — ${health?.reason ?? ''}`
                    : healthState === 'rejected'
                      ? `Rejected — ${health?.reason ?? ''}`
                      : 'Status unavailable'}
              </div>
            </div>
            {/* F5: user rank order (source_reorder takes the full id list). */}
            {/* F5: rank order is per-kind (the roster sorts by kind rank
                first), so cross-kind moves are invisible — disable at the
                kind boundaries instead of pretending. */}
            {i > 0 && all[i - 1].kind === kind && (
              <button
                aria-label={`Move ${name} up`}
                className="type-callout-emphasized text-secondary hover:text-primary"
                onClick={() => {
                  const ids = all.map((s) => s.config_id);
                  [ids[i - 1], ids[i]] = [ids[i], ids[i - 1]];
                  void invoke('source_reorder', { ids }).then(() => client.invalidateQueries({ queryKey: ['sources'] }));
                }}
              >
                ↑
              </button>
            )}
            {i < all.length - 1 && all[i + 1].kind === kind && (
              <button
                aria-label={`Move ${name} down`}
                className="type-callout-emphasized text-secondary hover:text-primary"
                onClick={() => {
                  const ids = all.map((s) => s.config_id);
                  [ids[i], ids[i + 1]] = [ids[i + 1], ids[i]];
                  void invoke('source_reorder', { ids }).then(() => client.invalidateQueries({ queryKey: ['sources'] }));
                }}
              >
                ↓
              </button>
            )}
            <button
              className="type-callout-emphasized text-secondary hover:text-primary"
              onClick={() => void invoke('source_set_enabled', { configId: config_id, enabled: !enabled }).then(() => client.invalidateQueries({ queryKey: ['sources'] }))}
            >
              {enabled ? 'Disable' : 'Enable'}
            </button>
            <button
              className="type-callout-emphasized text-key"
              onClick={() => void invoke('source_remove', { configId: config_id }).then(() => client.invalidateQueries({ queryKey: ['sources'] }))}
            >
              Remove
            </button>
          </div>
          );
        })}
        {sources.data && sources.data.length === 0 && (
          <p className="type-callout text-secondary">No sources yet — add an addon server below.</p>
        )}
        <form
          className="flex items-center gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (url.trim()) add.mutate(url.trim());
          }}
        >
          <input
            value={url}
            onChange={(e) => setUrl(e.currentTarget.value)}
            placeholder="https://my-addon-server.example"
            className="min-w-0 flex-1 rounded-[10px] border-0 bg-quinary px-3 py-2 type-body text-primary outline-none"
            aria-label="Addon server URL"
          />
          <button type="submit" disabled={add.isPending || !url.trim()} className="rounded-pill bg-key px-4 py-2 type-callout-emphasized text-white disabled:opacity-50">
            {add.isPending ? 'Checking…' : 'Add source'}
          </button>
        </form>
        {error && <p className="type-callout text-key">{error}</p>}
        {/* S9: Subsonic / OpenSubsonic server. No bundled server list — the
            user enters their own server. */}
        <form
          className="mt-2 flex flex-col gap-2 border-t border-quaternary pt-3"
          onSubmit={(e) => {
            e.preventDefault();
            if (subUrl.trim() && subUser.trim() && subPass) addSubsonic.mutate();
          }}
        >
          <div className="flex items-center gap-2">
            <input
              value={subUrl}
              onChange={(e) => setSubUrl(e.currentTarget.value)}
              placeholder="https://music.my-server.example"
              className="min-w-0 flex-1 rounded-[10px] border-0 bg-quinary px-3 py-2 type-body text-primary outline-none"
              aria-label="Subsonic server URL"
            />
          </div>
          <div className="flex items-center gap-2">
            <input
              value={subUser}
              onChange={(e) => setSubUser(e.currentTarget.value)}
              placeholder="Username"
              autoComplete="off"
              className="min-w-0 flex-1 rounded-[10px] border-0 bg-quinary px-3 py-2 type-body text-primary outline-none"
              aria-label="Subsonic username"
            />
            <input
              type="password"
              value={subPass}
              onChange={(e) => setSubPass(e.currentTarget.value)}
              placeholder="Password"
              autoComplete="new-password"
              className="min-w-0 flex-1 rounded-[10px] border-0 bg-quinary px-3 py-2 type-body text-primary outline-none"
              aria-label="Subsonic password"
            />
          </div>
          <label className="flex items-center gap-2 type-callout text-secondary">
            <input
              type="checkbox"
              checked={subAllowHttp}
              onChange={(e) => setSubAllowHttp(e.currentTarget.checked)}
            />
            Allow http (LAN / localhost servers only)
          </label>
          <div className="flex items-center gap-2">
            <button
              type="submit"
              disabled={addSubsonic.isPending || !subUrl.trim() || !subUser.trim() || !subPass}
              className="rounded-pill bg-key px-4 py-2 type-callout-emphasized text-white disabled:opacity-50"
            >
              {addSubsonic.isPending ? 'Adding…' : 'Add Subsonic server'}
            </button>
            <span className="type-callout text-secondary">Password is stored in the macOS Keychain.</span>
          </div>
        </form>
        {subError && <p className="type-callout text-key">{subError}</p>}
      </div>
    </section>
  );
}

/** The master switch for catalogue playback (YouTube Music via the sidecar). */
function CatalogueToggle() {
  const [on, setOn] = useState<boolean | null>(null);
  useEffect(() => {
    void invoke<[string, string][]>('settings_get').then((rows) => {
      setOn(rows.find(([k]) => k === 'catalogue_sources')?.[1] === 'true');
    });
  }, []);
  return (
    <Row
      label="YouTube Music (catalogue)"
      hint="Streams through the local Kotlin sidecar; needs the sidecar jar and Java 17"
    >
      <input
        type="checkbox"
        className={player_settings_checkbox}
        checked={on ?? false}
        onChange={(e) => {
          const value = e.currentTarget.checked;
          setOn(value);
          void invoke('settings_set', { key: 'catalogue_sources', value: value ? 'true' : 'false' });
        }}
      />
    </Row>
  );
}

// F9: the wire shape is FLATTENED — SourceWithHealth flattens `config`
// into the row (serde), so `config` never exists as a nested object.
interface SourceRow {
  config_id: string;
  kind: string;
  name: string;
  base_url: string;
  enabled: boolean;
  rank: number;
  username?: string | null;
  allow_http?: boolean;
  health: { state: 'ok' | 'unreachable' | 'rejected'; detail?: string | null; reason?: string };
}

/** `source_add`/`subsonic_add` return the bare flat SourceConfig. */
interface SourceConfig {
  config_id: string;
  kind: string;
  name: string;
  base_url: string;
  enabled: boolean;
  rank: number;
}

function AboutSection() {
  const [settings, setSettings] = useState<[string, string][]>([]);
  useEffect(() => {
    void invoke<[string, string][]>('settings_get').then(setSettings).catch(() => {});
  }, []);
  return (
    <section>
      <h2 className="mb-3 type-title-2-emphasized">About</h2>
      <div className="flex flex-col gap-2">
        <Row label="Version" hint="Local library, playback, lyrics providers; YouTube Music via the sidecar behind the catalogue flag">
          <span className="type-callout text-secondary">0.3.0</span>
        </Row>
        <Row label="Stored settings" hint="SQLite (WAL) in the app data directory">
          <span className="type-callout text-secondary">{settings.length} keys</span>
        </Row>
        <p className="type-callout text-secondary">
          Tansen is a GPL-3.0 desktop music player. Its design and logic follow BitChord (upstream, GPL-3.0) and
          borrow from its code under the same license; it is not affiliated with YouTube or Google.
        </p>
      </div>
    </section>
  );
}

const player_settings_button =
  'rounded-[10px] border-0 bg-quinary px-3.5 py-2 type-callout-emphasized text-primary outline-none';
const player_settings_checkbox = 'size-4 accent-key';

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 rounded-[10px] bg-quinary px-3 py-2.5">
      <div>
        <div className="type-body">{label}</div>
        {hint && <div className="type-callout text-secondary">{hint}</div>}
      </div>
      {children}
    </div>
  );
}
