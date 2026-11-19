// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest';

const readAppearance = vi.fn();
vi.mock('@/state/appearance', () => ({
  readAppearance: () => readAppearance(),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
}));

import { initChrome } from './chrome';

describe('initChrome theme sync', () => {
  let onChange: (() => void) | undefined;

  beforeEach(() => {
    readAppearance.mockReturnValue({ theme: 'system' });
    document.documentElement.removeAttribute('data-theme');
    onChange = undefined;
    window.matchMedia = vi.fn(
      () =>
        ({
          matches: true,
          addEventListener: (_event: string, listener: () => void) => {
            onChange = listener;
          },
        }) as unknown as MediaQueryList,
    );
  });

  it('derives the system theme from the OS preference', () => {
    initChrome();
    expect(document.documentElement.dataset.theme).toBe('dark');
  });

  it('honours an explicit theme override', () => {
    readAppearance.mockReturnValue({ theme: 'light' });
    initChrome();
    expect(document.documentElement.dataset.theme).toBe('light');
  });

  it('re-syncs on OS changes and appearance events', () => {
    const mql = {
      matches: true,
      addEventListener: (_event: string, listener: () => void) => {
        onChange = listener;
      },
    };
    window.matchMedia = vi.fn(() => mql as unknown as MediaQueryList);
    initChrome();
    expect(document.documentElement.dataset.theme).toBe('dark');

    mql.matches = false;
    onChange?.();
    expect(document.documentElement.dataset.theme).toBe('light');

    readAppearance.mockReturnValue({ theme: 'dark' });
    window.dispatchEvent(new Event('tansen:appearance'));
    expect(document.documentElement.dataset.theme).toBe('dark');
  });
});
