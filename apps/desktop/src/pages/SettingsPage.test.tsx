// @vitest-environment happy-dom
//
// Regression for the Settings crash ("undefined is not an object (evaluating
// 't.name')"): `sources_get_with_health` returns a FLAT wire row — Rust's
// SourceWithHealth carries `#[serde(flatten)] config`, so the JSON row has
// name/kind/base_url at the top level and no nested `config` object — but
// SourcesSection destructured `{ config, health }`, so every row threw and
// the whole Settings page fell to the router error boundary.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { SettingsPage } from './SettingsPage';

// The exact flattened shape the backend serializes (SourceConfig + health).
const sourceRows = [
  {
    config_id: 'yt-1',
    kind: 'youtube',
    name: 'Legacy YouTube entry',
    base_url: 'https://music.youtube.com',
    enabled: true,
    rank: 0,
    health: { state: 'ok', detail: null as string | null },
  },
  {
    config_id: 'local-1',
    kind: 'local',
    name: 'Legacy local entry',
    base_url: '/Music',
    enabled: false,
    rank: 0,
    health: { state: 'ok', detail: null as string | null },
  },
  {
    config_id: 'sub-1',
    kind: 'subsonic',
    name: 'Navidrome',
    base_url: 'https://s',
    enabled: true,
    rank: 0,
    health: { state: 'ok', detail: null as string | null },
  },
];

// Swapped per test: the backend can answer with rows that carry no health.
let wireRows: unknown[] = sourceRows;

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === 'sources_get_with_health') return wireRows;
    if (cmd === 'settings_get') return [] as [string, string][];
    if (cmd === 'local_files_get') return true;
    if (cmd === 'quality_get') return 'high';
    if (cmd === 'audio_settings_get') return audioSettings;
    if (cmd === 'downloads_get') {
      return {
        destination: '/Users/test/Downloads/Tansen',
        connected: true,
        rows: [],
        storage: { bytes: 1234, files: 2 },
        providers: [
          { kind: 'subsonic', supported: true, reason: 'Downloads the original file.' },
          { kind: 'addon', supported: true, reason: 'Direct progressive streams.' },
          { kind: 'local', supported: true, reason: 'Copies library files into the downloads folder.' },
        ],
      };
    }
    return []; // every other query renders its empty branch
  }),
}));

const audioSettings = {
  outputBits: 32,
  gapless: true,
  crossfadeMs: 0,
  loudness: false,
  skipSilence: false,
  skipThresholdDb: -50,
  skipMinMs: 2000,
  eqEnabled: false,
  eqGains: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  eqPreset: 'flat',
  deviceName: 'Test Output',
  channels: 2,
  sampleRate: 48000,
  formats: ['f32'],
  supports16Bit: true,
  supports32Bit: true,
  atmosSupported: false,
  atmosReason: 'Tansen decodes to stereo PCM; Dolby Atmos rendering is not part of this pipeline.',
  negotiatedBits: 32,
  negotiatedFormat: 'f32',
};

let root: Root | null = null;
let host: HTMLElement | null = null;

function mount() {
  if (!('fonts' in document)) {
    (document as never as { fonts: { load: () => Promise<unknown[]> } }).fonts = { load: async () => [] };
  }
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  act(() => {
    root!.render(
      <QueryClientProvider client={new QueryClient()}>
        <SettingsPage />
      </QueryClientProvider>,
    );
  });
  return host;
}

describe('SettingsPage sources rendering (flattened wire shape)', () => {
  afterEach(() => {
    if (root) {
      act(() => root!.unmount());
      root = null;
      host?.remove();
      host = null;
    }
  });

  it('renders a row whose health is absent as unknown instead of crashing', async () => {
    // Regression: a source row without `health` (cache written by a caller of
    // `sources_get`, which has no health) threw on `health.state`.
    wireRows = [{ config_id: 'addon-1', kind: 'addon', name: 'Addon', base_url: 'https://x', enabled: true, rank: 0 }];
    const host = mount();
    for (let i = 0; i < 20 && !host.textContent?.includes('Status unavailable'); i++) {
      await act(async () => {
        await new Promise((r) => setTimeout(r, 10));
      });
    }
    expect(host.textContent).toContain('Status unavailable');
    wireRows = sourceRows;
  });

  it('renders a rejected source with its reason', async () => {
    wireRows = [
      { config_id: 'sub-1', kind: 'subsonic', name: 'Navidrome', base_url: 'https://s', enabled: true, rank: 0, health: { state: 'rejected', reason: 'bad password' } },
    ];
    const host = mount();
    for (let i = 0; i < 20 && !host.textContent?.includes('Rejected'); i++) {
      await act(async () => {
        await new Promise((r) => setTimeout(r, 10));
      });
    }
    expect(host.textContent).toContain('Rejected — bad password');
    wireRows = sourceRows;
  });

  it('shows real servers and hides the legacy youtube/local roster rows', async () => {
    const host = mount();
    for (let i = 0; i < 20 && !host.textContent?.includes('Navidrome'); i++) {
      await act(async () => {
        await new Promise((r) => setTimeout(r, 10));
      });
    }
    expect(host.textContent).toContain('Navidrome');
    expect(host.textContent).not.toContain('Legacy YouTube entry');
    expect(host.textContent).not.toContain('Legacy local entry');
    // row.enabled is read top-level: the enabled Subsonic row offers Disable.
    expect(host.textContent).toContain('Disable');
  });
});

describe('SettingsPage audio, appearance and language sections', () => {
  it('renders real audio controls and every shipped language', async () => {
    const host = mount();
    for (let i = 0; i < 20 && !host.textContent?.includes('1.2 KB'); i++) {
      await act(async () => {
        await new Promise((r) => setTimeout(r, 10));
      });
    }
    const text = host.textContent ?? '';
    // Audio: streaming ladder, honest Atmos state, real output precision.
    expect(text).toContain('Streaming quality');
    expect(text).toContain('Low (64 kbps cap)');
    expect(text).toContain('Lossless');
    expect(text).toContain('Output precision');
    expect(text).toContain('32-bit float');
    expect(text).toContain('Dolby Atmos');
    expect(text).toContain('Not available');
    // Playback: the DSP-backed controls are real rows, not pending markers.
    for (const label of [
      'Loudness normalization',
      'Skip silence',
      'Equalizer',
      'Gapless playback',
      'Crossfade / Auto Mix',
      'Preset',
      'Reset to Flat',
    ]) {
      expect(text).toContain(label);
    }
    // Downloads: destination, real filesystem storage, provider honesty.
    expect(text).toContain('Choose downloads folder…');
    expect(text).toContain('1.2 KB');
    expect(text).toContain('2 items');
    expect(text).toContain('Local files');
    expect(text).not.toContain('No downloads yet');
    expect(text).toContain('Spotify');
    // Appearance rows.
    for (const label of ['Theme', 'Liquid Glass', 'Reduce Dynamic Blur', 'Reduce Animation', 'Full Screen Cover Art', 'Mesh Gradient Backdrop']) {
      expect(text).toContain(label);
    }
    // Language: all eleven, with native names.
    for (const label of ['English', 'हिन्दी', 'Español', 'Français', 'Deutsch', 'Português', '日本語', '한국어', '简体中文', 'العربية', 'Русский']) {
      expect(text).toContain(label);
    }
    // About: the app name and version only.
    expect(text).toContain('Tansen');
  });
});
