import { listen } from '@tauri-apps/api/event';

/**
 * Window-chrome behaviours that live outside React: theme sync with the OS,
 * the inactive-window state, and the release context-menu guard.
 */
export function initChrome(): void {
  const root = document.documentElement;

  // Follow the OS appearance like Music does; the inline bootstrap in
  // index.html already set the attribute before first paint.
  const appearance = matchMedia('(prefers-color-scheme: dark)');
  const syncTheme = () => {
    root.dataset.theme = appearance.matches ? 'dark' : 'light';
  };
  appearance.addEventListener('change', syncTheme);
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
