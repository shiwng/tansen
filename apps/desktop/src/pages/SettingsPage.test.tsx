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
    name: 'YouTube Music',
    base_url: 'https://music.youtube.com',
    enabled: true,
    rank: 0,
    health: { state: 'ok', detail: null as string | null },
  },
  {
    config_id: 'local-1',
    kind: 'local',
    name: 'Local Files',
    base_url: '/Music',
    enabled: false,
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
    return []; // every other query renders its empty branch
  }),
}));

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
    wireRows = [{ config_id: 'yt-1', kind: 'youtube', name: 'YouTube Music', base_url: 'https://x', enabled: true, rank: 0 }];
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

  it('renders every source row from the flattened row without crashing', async () => {
    const host = mount();
    // Queries resolve asynchronously; let them settle. ("YouTube Music"
    // also appears in static copy — wait on the local row instead.)
    for (let i = 0; i < 20 && !host.textContent?.includes('Local Files'); i++) {
      await act(async () => {
        await new Promise((r) => setTimeout(r, 10));
      });
    }
    expect(host.textContent).toContain('YouTube Music');
    expect(host.textContent).toContain('Local Files');
    // row.enabled is read top-level: the enabled row offers Disable,
    // the disabled row shows "(off)" and offers Enable.
    expect(host.textContent).toContain('Disable');
    expect(host.textContent).toContain('(off)');
    expect(host.textContent).toContain('Enable');
  });
});
