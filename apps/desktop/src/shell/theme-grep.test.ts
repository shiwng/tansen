import { describe, expect, it } from 'vitest';

/**
 * Guard: appearance follows macOS. Nothing may pin dark mode; the theme is
 * derived from the OS and re-derived on change. The live-switch evidence
 * itself is a capture/owner check, not this test.
 */
const sources = import.meta.glob<string>('/src/**/*.{ts,tsx}', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

describe('appearance', () => {
  it('has no dark: variants or forced theme attributes in production source', () => {
    const offenders = Object.entries(sources)
      .filter(([path]) => !path.includes('/src/lab/'))
      .filter(([, text]) => /(^|[\s"'`])dark:[a-z[]/.test(text) || /data-theme=.dark./.test(text));
    expect(offenders.map(([path]) => path)).toEqual([]);
  });

  it('derives the theme from the OS and reacts to changes', () => {
    const chrome = sources['/src/shell/chrome.ts'] ?? '';
    expect(chrome).toMatch(/matchMedia\('\(prefers-color-scheme: dark\)'\)/);
    expect(chrome).toMatch(/addEventListener\('change'/);
  });
});
