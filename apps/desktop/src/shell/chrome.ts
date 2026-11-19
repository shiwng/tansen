import { listen } from '@tauri-apps/api/event';
import { readAppearance } from '@/state/appearance';

/**
 * Window-chrome behaviours that live outside React: theme application (OS
 * sync plus the Settings override), the inactive-window state, and the
 * release context-menu guard.
 */
export function initChrome(): void {
  const root = document.documentElement;

  // Theme: follow the OS appearance like Music does, unless Settings picked
  // an explicit one (state/appearance.ts persists it and fires
  // `tansen:appearance` on change). The inline bootstrap in index.html
  // already set the attribute before first paint.
  const appearance = matchMedia('(prefers-color-scheme: dark)');
  const syncTheme = () => {
    const mode = readAppearance().theme;
    root.dataset.theme = mode === 'system' ? (appearance.matches ? 'dark' : 'light') : mode;
  };
  appearance.addEventListener('change', syncTheme);
  window.addEventListener('tansen:appearance', syncTheme);
  syncTheme();

  // Inactive window: CSS dims .chrome-dim surfaces (traffic lights dim natively).
  void listen('tauri://focus', () => root.removeAttribute('data-window-inactive'));
  void listen('tauri://blur', () => root.setAttribute('data-window-inactive', ''));

  // The WebView's default right-click menu is browser chrome; Music has none.
  // Debug keeps it; text fields keep it everywhere.
  if (import.meta.env.PROD) {
    window.addEventListener('contextmenu', (e) => {
      if (!(e.target instanceof Element) || !e.target.closest('input, textarea, [contenteditable]')) {
        e.preventDefault();
      }
    });
  }
}
