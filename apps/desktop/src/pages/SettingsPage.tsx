import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getVersion } from '@tauri-apps/api/app';
import { open } from '@tauri-apps/plugin-dialog';
import { Switch } from '@/components/Switch';
import { Link } from 'react-router';
import { AppError } from '@/shell/AppError';
import { useAppearance } from '@/state/appearance';
import { usePlayer } from '@/state/player';
import {
  EQ_BANDS_HZ,
  EQ_PRESETS,
  bandLabel,
  useAudioSettings,
  type AudioSettingsPatch,
} from '@/state/audioSettings';
import { LOCALES, useLocale, useT } from '@/i18n';
import { q } from '@/lib/rows';
import {
  ImportCancelled,
  buildImportRows,
  dedupeTracks,
  parseSpotifyPlaylistId,
  type CatalogRow,
  type SpotifyImportTrack,
} from '@/pages/spotifyImport';

/**
 * Settings: Audio, Playback, Downloads, Appearance, Language, then Library,
 * Sources, Spotify Import and About. Every control is wired to real
 * persisted state; features the pipeline cannot honestly support say so
 * instead of pretending.
 */
export function SettingsPage() {
  const t = useT();
  return (
    <div>
      <h1 className="mx-(--body-gutter) pt-11 type-header-emphasized">{t('settings.title')}</h1>
      {/* One broken section must not blank the page: each is its own boundary. */}
      <div className="mx-(--body-gutter) mb-12 flex max-w-3xl flex-col gap-8">
        <AppError inline><AudioSection /></AppError>
        <AppError inline><PlaybackSection /></AppError>
        <AppError inline><EqualizerSection /></AppError>
        <AppError inline><DownloadsSection /></AppError>
        <AppError inline><AppearanceSection /></AppError>
        <AppError inline><LanguageSection /></AppError>
        <AppError inline><LibrarySection /></AppError>
        <AppError inline><SourcesSection /></AppError>
        <AppError inline><SpotifyImportSection /></AppError>
        <AppError inline><AboutSection /></AppError>
      </div>
    </div>
  );
}

/** Streaming ceiling, downloads capability, Atmos and real output format. */
function AudioSection() {
  const t = useT();
  const [quality, setQuality] = useState<string | null>(null);
  useEffect(() => {
    void invoke<string>('quality_get').then(setQuality).catch(() => setQuality('high'));
  }, []);
  const audio = useAudioSettings();
  const s = audio.settings;
  const supports16 = s?.supports16Bit ?? true;
  const supports32 = s?.supports32Bit ?? true;

  return (
    <section id="audio">
      <h2 className="mb-3 type-title-2-emphasized">{t('settings.audio')}</h2>
      <div className="flex flex-col gap-2">
        <Row label={t('audio.streamingQuality')} hint={t('audio.streamingQualityHint')}>
          <select
            className={player_settings_button}
            value={quality ?? 'high'}
            onChange={(e) => {
              const v = e.currentTarget.value;
              setQuality(v);
              void invoke('quality_set', { quality: v }).catch(() => setQuality('high'));
            }}
          >
            <option value="low">Low (64 kbps cap)</option>
            <option value="medium">Medium (128 kbps cap)</option>
            <option value="high">High</option>
            <option value="lossless">Lossless</option>
          </select>
        </Row>
        <Row label={t('audio.downloads')} hint={t('audio.downloadsHint')}>
          <button
            className={player_settings_button}
            onClick={() => document.getElementById('downloads')?.scrollIntoView({ behavior: 'smooth' })}
          >
            {t('settings.downloads')} →
          </button>
        </Row>
        <Row label={t('audio.atmos')} hint={t('audio.atmosHint')}>
          <span className="type-callout text-secondary">
            {s?.atmosSupported ? 'Dolby Atmos' : t('audio.atmosUnavailable')}
          </span>
        </Row>
        {s && !s.atmosSupported && s.atmosReason && (
          <p className="-mt-1 pl-1 type-callout text-secondary">{s.atmosReason}</p>
        )}
        <Row
          label={t('audio.outputPrecision')}
          hint={t('audio.outputPrecisionHint')}
        >
          <select
            className={player_settings_button}
            value={s?.outputBits ?? 32}
            disabled={!s || audio.loading}
            onChange={(e) => patch(audio.set, { outputBits: Number(e.currentTarget.value) })}
          >
            <option value={16} disabled={!supports16}>{t('audio.output16')}</option>
            <option value={32} disabled={!supports32}>{t('audio.output32')}</option>
          </select>
        </Row>
        <Row label={t('audio.negotiated')} hint={s?.deviceName ?? undefined}>
          <span className="type-callout text-secondary">
            {s
              ? s.negotiatedFormat ??
                `${s.negotiatedBits}-bit${
                  s.sampleRate ? ` · ${t.number(s.sampleRate / 1000)} kHz` : ''
                }${s.channels ? ` · ${t.number(s.channels)} ch` : ''}`
              : '…'}
          </span>
        </Row>
        {audio.error && <ErrorLine error={audio.error} />}
      </div>
    </section>
  );
}

/** Playback toggles, gapless/crossfade and the two analysis-backed features. */
function PlaybackSection() {
  const t = useT();
  const player = usePlayer();
  const audio = useAudioSettings();
  const s = audio.settings;
  return (
    <section id="playback">
      <h2 className="mb-3 type-title-2-emphasized">{t('settings.playback')}</h2>
      <div className="flex flex-col gap-2">
        <Row label={t('playback.repeat')} hint={t('playback.repeatHint')}>
          <button className={player_settings_button} onClick={player.cycleRepeat}>
            <span className="capitalize">{player.repeat}</span>
          </button>
        </Row>
        <Row label={t('playback.shuffle')} hint={t('playback.shuffleHint')}>
          <Switch label={t('playback.shuffle')} checked={player.shuffle} onChange={player.toggleShuffle} />
        </Row>
        <AutoplayRow />
        <Row label={t('playback.gapless')} hint={t('playback.gaplessHint')}>
          <Switch
            label={t('playback.gapless')}
            checked={s?.gapless ?? true}
            onChange={(gapless) => patch(audio.set, { gapless })}
          />
        </Row>
        <Row label={t('playback.crossfade')} hint={t('playback.crossfadeHint')}>
          <select
            className={player_settings_button}
            value={s?.crossfadeMs ?? 0}
            onChange={(e) => patch(audio.set, { crossfadeMs: Number(e.currentTarget.value) })}
          >
            <option value={0}>{t('playback.crossfadeOff')}</option>
            <option value={3000}>3 s</option>
            <option value={6000}>6 s</option>
            <option value={9000}>9 s</option>
            <option value={12000}>12 s</option>
          </select>
        </Row>
        {s && s.crossfadeMs > 0 && (
          <Row label={t('playback.crossfadeCurve')} hint={t('playback.crossfadeCurveHint')}>
            <select
              className={player_settings_button}
              value={s.crossfadeCurve ?? 'linear'}
              onChange={(e) =>
                patch(audio.set, {
                  crossfadeCurve: e.currentTarget.value as 'linear' | 'equal-power',
                })
              }
            >
              <option value="linear">{t('playback.curveLinear')}</option>
              <option value="equal-power">{t('playback.curveEqualPower')}</option>
            </select>
          </Row>
        )}
        <Row label={t('playback.loudness')} hint={t('playback.loudnessHint')}>
          <Switch
            label={t('playback.loudness')}
            checked={s?.loudness ?? false}
            onChange={(loudness) => patch(audio.set, { loudness })}
          />
        </Row>
        <Row label={t('playback.skipSilence')} hint={t('playback.skipSilenceHint')}>
          <Switch
            label={t('playback.skipSilence')}
            checked={s?.skipSilence ?? false}
            onChange={(skipSilence) => patch(audio.set, { skipSilence })}
          />
        </Row>
        {s?.skipSilence && (
          <>
            <Row label={t('playback.skipThreshold')} hint="-60 dBFS is very quiet; -30 dBFS skips quiet passages">
              <select
                className={player_settings_button}
                value={s.skipThresholdDb}
                onChange={(e) => patch(audio.set, { skipThresholdDb: Number(e.currentTarget.value) })}
              >
                <option value={-60}>-60 dBFS</option>
                <option value={-50}>-50 dBFS</option>
                <option value={-40}>-40 dBFS</option>
                <option value={-30}>-30 dBFS</option>
              </select>
            </Row>
            <Row label={t('playback.skipMin')} hint="Longer minimums avoid skipping quiet musical passages">
              <select
                className={player_settings_button}
                value={s.skipMinMs}
                onChange={(e) => patch(audio.set, { skipMinMs: Number(e.currentTarget.value) })}
              >
                <option value={1000}>1 s</option>
                <option value={2000}>2 s</option>
                <option value={3000}>3 s</option>
                <option value={5000}>5 s</option>
              </select>
            </Row>
          </>
        )}
        <Row label={t('playback.nerdStats')} hint={t('playback.nerdStatsHint')}>
          <Switch
            label={t('playback.nerdStats')}
            checked={player.showNerdStats}
            onChange={(checked) => player.setShowNerdStats(checked)}
          />
        </Row>
        {audio.error && <ErrorLine error={audio.error} />}
      </div>
    </section>
  );
}

/** Autoplay related tracks when the queue tail plays (default off). */
function AutoplayRow() {
  const t = useT();
  const autoplay = usePlayer((s) => s.autoplay);
  const setAutoplay = usePlayer((s) => s.setAutoplay);
  return (
    <Row label={t('playback.autoplay')} hint={t('playback.autoplayHint')}>
      <Switch label={t('playback.autoplay')} checked={autoplay} onChange={setAutoplay} />
    </Row>
  );
}

/**
 * Ten-band peaking EQ. Slider moves are debounced; the backend smooths gain
 * changes inside the callback so there is never a click.
 */
function EqualizerSection() {
  const t = useT();
  const audio = useAudioSettings();
  const s = audio.settings;
  const [gains, setGains] = useState<number[]>(() =>
    Array.isArray(s?.eqGains) ? s.eqGains : Array<number>(10).fill(0),
  );
  const [preset, setPreset] = useState<string | null>(s?.eqPreset ?? 'flat');
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const gainsKey = Array.isArray(s?.eqGains) ? s.eqGains.join(',') : '';

  useEffect(() => {
    if (s && Array.isArray(s.eqGains)) {
      setGains(s.eqGains);
      setPreset(s.eqPreset ?? null);
    }
    // Hydrate only when the backend snapshot actually changes (not on every
    // render): dragging updates local state first.
  }, [gainsKey, s?.eqPreset]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => () => {
    if (timer.current) clearTimeout(timer.current);
  }, []);

  function changeBand(index: number, value: number) {
    const next = gains.map((g, i) => (i === index ? value : g));
    setGains(next);
    setPreset(null);
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => audio.set({ eqGains: next }), 120);
  }

  function reset() {
    const flat = Array<number>(10).fill(0);
    setGains(flat);
    setPreset('flat');
    audio.set({ eqEnabled: true, eqPreset: 'flat' });
  }

  return (
    <section id="equalizer">
      <h2 className="mb-3 type-title-2-emphasized">{t('playback.equalizer')}</h2>
      <div className="flex flex-col gap-2">
        <Row label={t('playback.equalizer')} hint={t('playback.equalizerHint')}>
          <Switch
            label={t('playback.equalizer')}
            checked={s?.eqEnabled ?? false}
            onChange={(eqEnabled) => patch(audio.set, { eqEnabled })}
          />
        </Row>
        <Row label={t('playback.eqPreset')}>
          <div className="flex items-center gap-2">
            <select
              className={player_settings_button}
              value={preset ?? ''}
              disabled={!s?.eqEnabled}
              onChange={(e) => {
                const value = e.currentTarget.value;
                if (!value) return;
                setPreset(value);
                audio.set({ eqPreset: value });
              }}
            >
              {preset === null && <option value="" disabled>Custom</option>}
              {EQ_PRESETS.map((name) => (
                <option key={name} value={name}>
                  {name.split('-').map((p) => p.charAt(0).toUpperCase() + p.slice(1)).join(' ')}
                </option>
              ))}
            </select>
            <button
              className="type-callout-emphasized text-key disabled:opacity-50"
              disabled={!s?.eqEnabled}
              onClick={reset}
            >
              {t('playback.eqReset')}
            </button>
          </div>
        </Row>
        <div
          className={`flex flex-col gap-1 rounded-[10px] bg-quinary px-3 py-3 transition-opacity ${
            s?.eqEnabled ? '' : 'pointer-events-none opacity-40'
          }`}
        >
          {gains.map((gain, i) => (
            <div key={EQ_BANDS_HZ[i]} className="flex items-center gap-3">
              <span className="w-9 flex-none type-callout text-secondary">{bandLabel(EQ_BANDS_HZ[i]!)}</span>
              <input
                type="range"
                min={-12}
                max={12}
                step={0.5}
                value={gain}
                aria-label={`${bandLabel(EQ_BANDS_HZ[i]!)} Hz gain`}
                onChange={(e) => changeBand(i, Number(e.currentTarget.value))}
                className="min-w-0 flex-1 accent-[var(--key)]"
              />
              <span className="w-12 flex-none text-right type-callout tabular-nums">
                {gain > 0 ? '+' : ''}
                {gain.toFixed(1)} dB
              </span>
            </div>
          ))}
        </div>
        {audio.error && <ErrorLine error={audio.error} />}
      </div>
    </section>
  );
}

// Downloads

type DownloadStatus =
  | 'queued'
  | 'downloading'
  | 'paused'
  | 'complete'
  | 'failed'
  | 'cancelled';

interface DownloadRow {
  id: number;
  kind: string;
  source: string;
  trackId: string;
  title: string;
  artist: string;
  album: string | null;
  durationSec: number;
  artworkUrl: string | null;
  status: DownloadStatus;
  progress: number;
  total: number;
  path: string | null;
  error: string | null;
  quality: string;
  createdAt: number;
  updatedAt: number;
  /** Real on-disk size from the filesystem; absent when the file is gone. */
  sizeBytes?: number;
}

interface DownloadCapabilities {
  destination: string | null;
  connected: boolean;
  rows: DownloadRow[];
  storage: { bytes: number; files: number; error?: string | null };
  providers: Array<{ kind: string; supported: boolean; reason: string }>;
}

/**
 * Persistent download queue. Subsonic servers and direct addon streams can
 * be downloaded; YouTube Music and Spotify audio cannot, and say so.
 */
function DownloadsSection() {
  const t = useT();
  const client = useQueryClient();
  const query = useQuery({
    queryKey: ['downloads'],
    queryFn: () => invoke<DownloadCapabilities>('downloads_get'),
    refetchInterval: (q) =>
      q.state.data?.rows.some((r) => r.status === 'downloading' || r.status === 'queued') ? 1500 : false,
  });
  const [error, setError] = useState<string | null>(null);
  const localFiles = useQuery({
    queryKey: ['local-files'],
    queryFn: () => invoke<boolean>('local_files_get'),
  });
  const setLocalFiles = useMutation({
    mutationFn: (enabled: boolean) => invoke('local_files_set', { enabled }),
    onSuccess: (_result, enabled) => {
      void client.invalidateQueries({ queryKey: ['local-files'] });
      void client.invalidateQueries({ queryKey: ['downloads'] });
      if (enabled) {
        // Turning the source back on re-indexes what is already on disk.
        void invoke('scan_start').catch(() => {});
      }
    },
    onError: (e) => setError(String(e).replace(/^Error:\s*/, '')),
  });

  useEffect(() => {
    try {
      const unlisten = listen('downloads_changed', () => {
        void client.invalidateQueries({ queryKey: ['downloads'] });
      });
      unlisten.catch(() => {});
      return () => {
        void unlisten.then((off) => off()).catch(() => {});
      };
    } catch {
      // No Tauri event bridge (unit tests): polling still drives the UI.
      return undefined;
    }
  }, [client]);

  const data =
    query.data && typeof query.data === 'object' && !Array.isArray(query.data)
      ? query.data
      : undefined;
  const rows = data?.rows ?? [];
  const active = rows.filter((r) => r.status === 'queued' || r.status === 'downloading').length;

  async function run(promise: Promise<unknown>) {
    try {
      setError(null);
      await promise;
      await client.invalidateQueries({ queryKey: ['downloads'] });
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ''));
    }
  }

  return (
    <section id="downloads">
      <h2 className="mb-3 type-title-2-emphasized">{t('settings.downloads')}</h2>
      <div className="flex flex-col gap-2">
        <Row label={t('downloads.chooseFolder')} hint={data?.destination ?? undefined}>
          <button
            className={player_settings_button}
            onClick={async () => {
              const sel = await open({ directory: true, multiple: false, title: t('downloads.chooseFolder') });
              if (typeof sel === 'string') {
                await run(invoke('downloads_set_destination', { path: sel }));
              }
            }}
          >
            {t('downloads.chooseAction')}
          </button>
        </Row>
        <Row
          label={t('downloads.storage')}
          hint={
            data
              ? t.plural('downloads.itemCount', data.storage?.files ?? 0)
              : undefined
          }
        >
          <span className="type-callout text-secondary">
            {data ? formatBytes(data.storage?.bytes ?? 0) : '…'}
          </span>
        </Row>
        {data?.storage?.error && <p className="pl-1 type-callout text-key">{data.storage.error}</p>}
        {data?.providers?.map((p) => (
          <Row key={p.kind} label={providerLabel(p.kind)} hint={p.reason}>
            {p.kind === 'local' ? (
              <Switch
                label={providerLabel(p.kind)}
                checked={localFiles.data ?? true}
                onChange={(checked) => setLocalFiles.mutate(checked)}
              />
            ) : (
              <span className="type-callout text-secondary">
                {p.supported ? t('downloads.supported') : t('downloads.unavailable')}
              </span>
            )}
          </Row>
        ))}
        {active > 0 && (
          <p className="pl-1 type-callout text-secondary">
            {t('downloads.activeCount', { count: t.number(active) })}
          </p>
        )}
        {rows.map((row) => (
          <div key={row.id} className="flex flex-col gap-2 rounded-[10px] bg-quinary px-3 py-2.5">
            <div className="flex items-center gap-3">
              <div className="min-w-0 flex-1">
                <div className="truncate type-body">{row.title || row.trackId}</div>
                <div className="truncate type-callout text-secondary">
                  {[row.artist, row.album].filter(Boolean).join(' — ') || providerLabel(row.source)}
                </div>
              </div>
              <StatusPill status={row.status} />
            </div>
            {(row.status === 'downloading' || row.status === 'paused') && (
              <div className="flex items-center gap-2">
                <div className="h-1 flex-1 overflow-hidden rounded-pill bg-quaternary">
                  <div
                    className="h-full rounded-pill bg-key transition-[width] duration-300"
                    style={{ width: `${row.total > 0 ? Math.min(100, (row.progress / row.total) * 100) : 0}%` }}
                  />
                </div>
                <span className="w-24 text-right type-callout tabular-nums text-secondary">
                  {row.total > 0
                    ? `${t.number(Math.round((row.progress / row.total) * 100))}%`
                    : formatBytes(row.progress)}
                </span>
              </div>
            )}
            {row.error && <p className="type-callout text-key">{row.error}</p>}
            {row.status === 'complete' && (
              <p className="type-callout text-secondary">
                {row.sizeBytes != null ? `${formatBytes(row.sizeBytes)} · ` : 'File missing · '}
                {t('downloads.completedAt', { date: t.date(row.updatedAt * 1000) })}
              </p>
            )}
            <div className="flex flex-wrap items-center gap-3">
              {row.status === 'downloading' && (
                <Action onClick={() => run(invoke('downloads_pause', { id: row.id }))}>{t('downloads.pause')}</Action>
              )}
              {row.status === 'paused' && (
                <Action onClick={() => run(invoke('downloads_resume', { id: row.id }))}>{t('downloads.resume')}</Action>
              )}
              {(row.status === 'failed' || row.status === 'cancelled') && (
                <Action onClick={() => run(invoke('downloads_retry', { id: row.id }))}>{t('downloads.retry')}</Action>
              )}
              {(row.status === 'queued' || row.status === 'downloading' || row.status === 'paused') && (
                <Action onClick={() => run(invoke('downloads_cancel', { id: row.id }))}>{t('downloads.cancel')}</Action>
              )}
              {row.status !== 'downloading' && (
                <Action destructive onClick={() => run(invoke('downloads_remove', { id: row.id }))}>
                  {t('downloads.remove')}
                </Action>
              )}
            </div>
          </div>
        ))}
        {rows.some((r) => r.status === 'complete' || r.status === 'failed' || r.status === 'cancelled') && (
          <div className="flex justify-end">
            <button
              className="type-callout-emphasized text-secondary hover:text-primary"
              onClick={() => run(invoke('downloads_clear_completed'))}
            >
              {t('downloads.clearCompleted')}
            </button>
          </div>
        )}
        {error && <ErrorLine error={error} />}
      </div>
    </section>
  );
}

function providerLabel(kind: string): string {
  if (kind === 'subsonic') return 'Subsonic';
  if (kind === 'addon') return 'Addon servers';
  if (kind === 'local') return 'Local files';
  return kind;
}

function StatusPill({ status }: { status: DownloadStatus }) {
  const label: Record<DownloadStatus, string> = {
    queued: 'Queued',
    downloading: 'Downloading',
    paused: 'Paused',
    complete: 'Complete',
    failed: 'Failed',
    cancelled: 'Cancelled',
  };
  const tone =
    status === 'complete'
      ? 'text-key'
      : status === 'failed'
        ? 'text-key'
        : status === 'downloading'
          ? 'text-primary'
          : 'text-secondary';
  return <span className={`flex-none type-callout-emphasized ${tone}`}>{label[status]}</span>;
}

function Action({
  children,
  onClick,
  destructive,
}: {
  children: React.ReactNode;
  onClick: () => void;
  destructive?: boolean;
}) {
  return (
    <button
      className={`type-callout-emphasized ${destructive ? 'text-key' : 'text-secondary hover:text-primary'}`}
      onClick={onClick}
    >
      {children}
    </button>
  );
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  const i = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
  return `${(bytes / 1024 ** i).toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

function ErrorLine({ error }: { error: unknown }) {
  return <p className="type-callout text-key">{String(error).replace(/^Error:\s*/, '')}</p>;
}

/** Sends one patch, swallowing the mutation's own error (shown inline). */
function patch(set: (patch: AudioSettingsPatch) => void, value: AudioSettingsPatch) {
  set(value);
}

/** Appearance: every toggle drives real CSS/components (state/appearance.ts). */
function AppearanceSection() {
  const t = useT();
  const appearance = useAppearance();
  return (
    <section>
      <h2 className="mb-3 type-title-2-emphasized">{t('settings.appearance')}</h2>
      <div className="flex flex-col gap-2">
        <Row label="Theme" hint="Follows the system, or pins light or dark">
          <div className="flex flex-none rounded-pill bg-segmented p-[3px]" role="radiogroup" aria-label="Theme">
            {(['system', 'light', 'dark'] as const).map((theme) => (
              <button
                key={theme}
                type="button"
                role="radio"
                aria-checked={appearance.theme === theme}
                onClick={() => appearance.setTheme(theme)}
                className={`h-[24px] rounded-pill px-3 type-callout-emphasized capitalize transition-colors ${
                  appearance.theme === theme ? 'bg-segmented-selected text-primary' : 'text-secondary hover:text-primary'
                }`}
              >
                {theme}
              </button>
            ))}
          </div>
        </Row>
        <Row label="Liquid Glass" hint="The frosted toolbar, panel and pill materials — off means solid surfaces">
          <Switch label="Liquid Glass" checked={appearance.glass} onChange={appearance.setGlass} />
        </Row>
        <Row label="Reduce Dynamic Blur" hint="Drops the large decorative blurs (menus, hero washes); keeps the frosted material">
          <Switch label="Reduce Dynamic Blur" checked={appearance.reduceBlur} onChange={appearance.setReduceBlur} />
        </Row>
        <Row label="Reduce Animation" hint="Forces the reduced-motion kill switch; the system setting keeps working too">
          <Switch label="Reduce Animation" checked={appearance.reduceMotion} onChange={appearance.setReduceMotion} />
        </Row>
        <Row label="Full Screen Cover Art" hint="Shows the cover art in the full-screen player">
          <Switch label="Full Screen Cover Art" checked={appearance.fullScreenArt} onChange={appearance.setFullScreenArt} />
        </Row>
        <Row label="Mesh Gradient Backdrop" hint="The drifting artwork mesh behind the full-screen player">
          <Switch label="Mesh Gradient Backdrop" checked={appearance.meshBackdrop} onChange={appearance.setMeshBackdrop} />
        </Row>
      </div>
    </section>
  );
}

/** Eleven locales; switching is live and persists. Arabic flips to RTL. */
function LanguageSection() {
  const t = useT();
  const locale = useLocale((s) => s.locale);
  const setLocale = useLocale((s) => s.setLocale);
  return (
    <section>
      <h2 className="mb-3 type-title-2-emphasized">{t('settings.language')}</h2>
      <div className="flex flex-col gap-2">
        <Row label={t('language.appLanguage')} hint={t('language.appLanguageHint')}>
          <select
            className={player_settings_button}
            value={locale}
            onChange={(e) => setLocale(e.currentTarget.value as typeof locale)}
            aria-label={t('language.appLanguage')}
          >
            {LOCALES.map((l) => (
              <option key={l.id} value={l.id}>
                {l.native} — {l.name}
              </option>
            ))}
          </select>
        </Row>
      </div>
    </section>
  );
}

function LibrarySection() {
  const t = useT();
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
      <h2 className="mb-3 type-title-2-emphasized">{t('settings.library')}</h2>
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
          <Switch
            label="Watch folders"
            checked={watch ?? true}
            onChange={(checked) => {
              setWatch(checked);
              void invoke('settings_set', { key: 'watch_folders', value: checked ? 'true' : 'false' });
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

function SourcesSection() {
  const t = useT();
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
  // Subsonic: the password lives only in this transient form field until
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

  // Legacy seeded rows: YouTube Music runs through Tansen's own InnerTube
  // path, local files through the Local files switch. Neither is an addon
  // server, so they do not belong in this roster.
  const visible = (sources.data ?? []).filter((row) => row.kind !== 'youtube' && row.kind !== 'local');

  return (
    <section>
      <h2 className="mb-3 type-title-2-emphasized">{t('settings.sources')}</h2>
      <p className="mb-2 type-callout text-secondary">
        Addon servers speak the original's manifest/search/stream protocol. Registration validates the manifest;
        search, playback, health, and removal are live.
      </p>
      <div className="flex flex-col gap-2">
        {visible.map((row, i, all) => {
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
            {/* Rank order is per-kind (the roster sorts by kind rank
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
        {sources.data && visible.length === 0 && (
          <p className="type-callout text-secondary">No addon or Subsonic servers yet — add one below.</p>
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
        {/* Subsonic / OpenSubsonic server. No bundled server list — the
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
            <Switch label="Allow http" checked={subAllowHttp} onChange={(checked) => setSubAllowHttp(checked)} />
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

// The wire shape is FLATTENED — SourceWithHealth flattens `config`
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

/**
 * Spotify playlist import: public playlists, fetched without sign-in through
 * Spotify's embed surface (the Web API requires a user token since late
 * 2024), then every row past the embed's 100 via the web player's own paged
 * graph. Tracks are matched against YouTube Music for playback; nothing is
 * downloaded from Spotify. Private playlists are refused with an explanation.
 */
function SpotifyImportSection() {
  const t = useT();
  const client = useQueryClient();
  const [url, setUrl] = useState('');
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [summary, setSummary] = useState<{ text: string; playlistId: number } | null>(null);
  // Cancellation: flips to true when the user presses Cancel; the matching
  // loop checks it between tracks and aborts before anything is saved.
  const cancelRef = useRef(false);

  async function importPlaylist() {
    const spotifyId = parseSpotifyPlaylistId(url);
    if (!spotifyId) {
      setError("That doesn't look like a Spotify playlist link.");
      return;
    }
    setBusy(true);
    setError(null);
    setSummary(null);
    cancelRef.current = false;
    // Partial failures: a lookup that errors counts, its row imports
    // unmatched — one dead sidecar doesn't kill a 90-track import.
    let lookupsFailed = 0;
    try {
      setStatus('Fetching playlist from Spotify…');
      const fetched = await invoke<{ name: string; artwork_url: string | null; skipped: number; truncated: boolean; tracks: SpotifyImportTrack[] }>(
        'spotify_playlist_fetch',
        { playlistId: spotifyId },
      );
      const { rows, duplicates } = dedupeTracks(fetched.tracks);
      const unavailableOnSpotify = rows.filter((t) => !t.playable).length;
      setStatus(`Matching ${rows.length} tracks on YouTube Music…`);
      const { rows: importRows, matched } = await buildImportRows(
        rows,
        (query) =>
          invoke<CatalogRow[]>('yt_search', { query }).catch(() => {
            lookupsFailed += 1;
            return [];
          }),
        4,
        (n, total) => setStatus(`Matching ${n}/${total} tracks on YouTube Music…`),
        () => cancelRef.current,
      );
      if (cancelRef.current) throw new ImportCancelled();
      setStatus('Saving playlist…');
      const playlistId = await invoke<number>('playlist_create', { name: fetched.name });
      await invoke('playlist_add_remote_tracks', { id: playlistId, tracks: importRows });
      void client.invalidateQueries({ queryKey: ['local', 'playlists'] });
      const notFound = rows.length - unavailableOnSpotify - matched;
      const parts = [`Imported “${fetched.name}” — ${matched} matched on YouTube Music`];
      if (notFound > 0) parts.push(`${notFound} not found`);
      if (unavailableOnSpotify > 0) parts.push(`${unavailableOnSpotify} unavailable on Spotify`);
      if (lookupsFailed > 0) parts.push(`${lookupsFailed} lookup failures`);
      if (duplicates > 0) parts.push(`${duplicates} duplicates skipped`);
      if (fetched.skipped > 0) parts.push(`${fetched.skipped} non-music items skipped`);
      if (fetched.truncated) parts.push('Spotify hid tracks past the first 100');
      setSummary({ text: `${parts.join(', ')}.`, playlistId });
      setUrl('');
    } catch (e) {
      if (e instanceof ImportCancelled) {
        setSummary(null);
        setStatus('Import cancelled — nothing was saved.');
      } else {
        setError(String(e).replace(/^Error:\s*/, ''));
      }
    } finally {
      setBusy(false);
      setStatus(null);
    }
  }

  return (
    <section>
      <h2 className="mb-3 type-title-2-emphasized">{t('settings.spotify')}</h2>
      <p className="mb-2 type-callout text-secondary">
        Imports a public Spotify playlist — names, artists, order and cover — and matches it against YouTube Music for
        playback. Private playlists cannot be read; everything else imports in full, however long it is.
      </p>
      <form
        className="flex items-center gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (url.trim() && !busy) void importPlaylist();
        }}
      >
        <input
          value={url}
          onChange={(e) => setUrl(e.currentTarget.value)}
          placeholder="https://open.spotify.com/playlist/…"
          className="min-w-0 flex-1 rounded-[10px] border-0 bg-quinary px-3 py-2 type-body text-primary outline-none"
          aria-label="Spotify playlist URL"
        />
        <button
          type="submit"
          disabled={busy || !url.trim()}
          className="rounded-pill bg-key px-4 py-2 type-callout-emphasized text-white disabled:opacity-50"
        >
          {busy ? 'Importing…' : 'Import'}
        </button>
        {busy && (
          <button
            type="button"
            className="rounded-pill bg-quinary px-4 py-2 type-callout-emphasized text-primary hover:bg-quaternary"
            onClick={() => {
              cancelRef.current = true;
            }}
          >
            Cancel
          </button>
        )}
      </form>
      {status && <p className="type-callout text-secondary">{status}</p>}
      {error && <p className="type-callout text-key">{error}</p>}
      {summary && (
        <p className="type-callout text-secondary">
          {summary.text}{' '}
          <Link className="type-callout-emphasized text-key" to={`/playlist/${summary.playlistId}`}>
            Open playlist →
          </Link>
        </p>
      )}
    </section>
  );
}

/** Requirement: app name and the real version, nothing else. */
function AboutSection() {
  const t = useT();
  const [version, setVersion] = useState('');
  useEffect(() => {
    void getVersion().then(setVersion).catch(() => setVersion(''));
  }, []);
  return (
    <section>
      <h2 className="mb-3 type-title-2-emphasized">{t('settings.about')}</h2>
      <div className="rounded-[10px] bg-quinary px-3 py-2.5">
        <div className="flex items-baseline gap-3">
          <span className="type-body">Tansen</span>
          <span className="type-callout text-secondary">{version || '…'}</span>
        </div>
        <p className="mt-1.5 type-callout-emphasized">
          The first Apple Music-style local music player built in Rust.
        </p>
        <p className="mt-1 type-callout text-secondary">
          Local-first — your library stays on disk and a native Rust audio
          engine plays it. No telemetry, no account, no backend. Not
          affiliated with Apple.
        </p>
      </div>
    </section>
  );
}

const player_settings_button =
  'rounded-[10px] border-0 bg-quinary px-3.5 py-2 type-callout-emphasized text-primary outline-none';

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
