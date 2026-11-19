import { useSyncExternalStore } from 'react';
import { useAppearance } from '@/state/appearance';

const media = typeof matchMedia === 'function' ? matchMedia('(prefers-reduced-motion: reduce)') : null;

/**
 * True when the OS asks for reduced motion OR the user forced it in
 * Settings → Appearance → Reduce Animation. CSS honors the same two
 * sources via global.css's media block and appearance.css's attribute.
 */
export function useReducedMotion(): boolean {
  const forced = useAppearance((s) => s.reduceMotion);
  const os = useSyncExternalStore(
    (onChange) => {
      media?.addEventListener('change', onChange);
      return () => media?.removeEventListener('change', onChange);
    },
    () => media?.matches ?? false,
  );
  return forced || os;
}
