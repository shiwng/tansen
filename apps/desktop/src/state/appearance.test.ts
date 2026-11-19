// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from 'vitest';
import { readAppearance, useAppearance } from './appearance';

beforeEach(() => {
  localStorage.clear();
  document.documentElement.removeAttribute('data-glass');
  document.documentElement.removeAttribute('data-reduce-blur');
  document.documentElement.removeAttribute('data-reduce-motion');
});

describe('appearance store', () => {
  it('defaults to system theme and every visual on', () => {
    expect(readAppearance()).toEqual({
      theme: 'system',
      glass: true,
      reduceBlur: false,
      reduceMotion: false,
      fullScreenArt: true,
      meshBackdrop: true,
    });
  });

  it('sanitizes hostile or corrupt stored values back to defaults', () => {
    localStorage.setItem('bitchord:appearance', '{"theme":"neon","glass":"yes","reduceBlur":1}');
    expect(readAppearance().theme).toBe('system');
    expect(readAppearance().glass).toBe(true);
    expect(readAppearance().reduceBlur).toBe(false);
    localStorage.setItem('bitchord:appearance', 'not json');
    expect(readAppearance().theme).toBe('system');
  });

  it('persists changes and reflects the visual attributes immediately', () => {
    useAppearance.getState().setGlass(false);
    useAppearance.getState().setReduceBlur(true);
    useAppearance.getState().setReduceMotion(true);
    useAppearance.getState().setTheme('dark');
    const root = document.documentElement;
    expect(root.dataset.glass).toBe('off');
    expect(root.dataset.reduceBlur).toBe('on');
    expect(root.dataset.reduceMotion).toBe('on');
    const stored = JSON.parse(localStorage.getItem('bitchord:appearance') ?? '{}');
    expect(stored).toMatchObject({ glass: false, reduceBlur: true, reduceMotion: true, theme: 'dark' });
    // The persisted blob is data only — the setters must not leak into it.
    expect(Object.keys(stored)).toEqual(['theme', 'glass', 'reduceBlur', 'reduceMotion', 'fullScreenArt', 'meshBackdrop']);
  });

  it('re-reads from storage on the next module read (restart path)', () => {
    localStorage.setItem(
      'bitchord:appearance',
      JSON.stringify({ theme: 'light', fullScreenArt: false, meshBackdrop: false }),
    );
    expect(readAppearance().theme).toBe('light');
    expect(readAppearance().fullScreenArt).toBe(false);
  });
});
