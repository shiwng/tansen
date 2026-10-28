import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { LyricsSettings } from '@/shell/LyricsSettings';
import { usePlayer, CROSSFADE_CHOICES } from '@/state/player';
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
      <div className="mx-(--body-gutter) mb-12 flex max-w-3xl flex-col gap-8">
        <PlaybackSection />
        <LibrarySection />
        <LyricsSection />
        <SourcesSection />
        <IntegrationsSection />
        <AboutSection />
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
          <input type="checkbox" className={player_settings_checkbox} checked={player.shuffle} onChange={player.toggleShuffle} />
        </Row>
        <Row label="Crossfade" hint="Stored now; the two-deck fade arrives with phase 6">
          <select
            className={player_settings_button}
            value={player.crossfadeSec}
            onChange={(e) => player.setCrossfade(Number(e.currentTarget.value))}
          >
            {CROSSFADE_CHOICES.map((sec) => (
              <option key={sec} value={sec}>
                {sec === 0 ? 'Off' : `${sec} seconds`}
              </option>
            ))}
          </select>
        </Row>
        <Row label="Gapless playback" hint="Always on for local files">
          <span className="type-callout text-secondary">Always on</span>
        </Row>
        <Row label="Equalizer / loudness / skip silence" hint="The DSP chain lands in phase 6">
          <span className="type-callout text-secondary">Not yet implemented</span>
        </Row>
      </div>
    </section>
  );
}

function LibrarySection() {
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
        <Row label="Watch folders" hint="Re-index automatically when files change on disk">
          <input
            type="checkbox"
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
    mutationFn: (base_url: string) => invoke<SourceRow['config']>('source_add', { new: { base_url } }),
    onSuccess: () => {
      setUrl('');
      setError(null);
      void client.invalidateQueries({ queryKey: ['sources'] });
    },
    onError: (e) => setError(String(e)),
  });

  return (
    <section>
      <h2 className="mb-3 type-title-2-emphasized">Sources</h2>
      <p className="mb-2 type-callout text-secondary">
        Addon servers speak the original's manifest/search/stream protocol. Search and playback through them land with
        the phase-4 resolver; management and health are live now.
      </p>
      <div className="flex flex-col gap-2">
        {sources.data?.map(({ config, health }) => (
          <div key={config.config_id} className="flex items-center gap-3 rounded-[10px] bg-quinary px-3 py-2.5">
            <span
              className="size-2 flex-none rounded-full"
              style={{ background: health.state === 'ok' ? 'var(--systemGreen)' : health.state === 'rejected' ? 'var(--systemRed)' : 'var(--systemGray)' }}
              title={health.state}
            />
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2 type-body">
                <span className="truncate">{config.name}</span>
                {!config.enabled && <span className="type-callout text-secondary">(off)</span>}
              </div>
              <div className="truncate type-callout text-secondary">
                {health.state === 'ok' ? config.base_url : health.state === 'unreachable' ? `Unreachable — ${health.reason}` : `Rejected — ${'reason' in health ? health.reason : ''}`}
              </div>
            </div>
            <button
              className="type-callout-emphasized text-secondary hover:text-primary"
              onClick={() => void invoke('source_set_enabled', { configId: config.config_id, enabled: !config.enabled }).then(() => client.invalidateQueries({ queryKey: ['sources'] }))}
            >
              {config.enabled ? 'Disable' : 'Enable'}
            </button>
            <button
              className="type-callout-emphasized text-key"
              onClick={() => void invoke('source_remove', { configId: config.config_id }).then(() => client.invalidateQueries({ queryKey: ['sources'] }))}
            >
              Remove
            </button>
          </div>
        ))}
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
      </div>
    </section>
  );
}

interface SourceRow {
  config: { config_id: string; name: string; base_url: string; enabled: boolean; rank: number };
  health: { state: 'ok' | 'unreachable' | 'rejected'; detail?: string | null; reason?: string };
}

function IntegrationsSection() {
  return (
    <section>
      <h2 className="mb-3 type-title-2-emphasized">Integrations</h2>
      <div className="flex flex-col gap-2">
        <Row label="Last.fm / ListenBrainz scrobbling" hint="Phase 7">
          <span className="type-callout text-secondary">Not yet implemented</span>
        </Row>
        <Row label="Discord Rich Presence" hint="Phase 7">
          <span className="type-callout text-secondary">Not yet implemented</span>
        </Row>
        <Row label="Listening parties" hint="Protocol exists upstream; desktop client is phase 7">
          <span className="type-callout text-secondary">Not yet implemented</span>
        </Row>
      </div>
    </section>
  );
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
        <Row label="Version" hint="Phase 3 slice + M2 library work; lyrics providers are wired (phase 5 data layer live)">
          <span className="type-callout text-secondary">0.2.0</span>
        </Row>
        <Row label="Stored settings" hint="SQLite (WAL) in the app data directory">
          <span className="type-callout text-secondary">{settings.length} keys</span>
        </Row>
        <p className="type-callout text-secondary">
          BitChord Desktop is an independent GPL-3.0 re-implementation inspired by BitChord; not affiliated with
          YouTube or Google. Design and logic reference the upstream GPL project — see docs/audit-bitchord.md.
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
