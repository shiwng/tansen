/*
 * Lenis smooth scrolling for the page scroller (#scrollable-page).
 *
 * The wrapper IS the page <main>, so wheel input outside its subtree
 * (sidebar, queue panel, lyrics, context menus and dialogs — all siblings or
 * body portals) never reaches Lenis and keeps native scrolling. In-page
 * nested scrollers (shelves, dropdown results) are detected by
 * allowNestedScroll and stay native too.
 *
 * One animation loop, managed here: Lenis's autoRaf would tick forever even
 * when idle, so the loop is started on demand and stops as soon as the
 * animation settles. Native momentum deltas are already smoothed by macOS —
 * the lerp below is deliberately snappy to avoid a double-smoothed tail.
 */
import Lenis from 'lenis';
import { useEffect, type RefObject } from 'react';

export function lenisOptions(el: HTMLElement) {
  return {
    wrapper: el,
    content: el.firstElementChild ?? el, // inner wrapper: route swaps re-measure via its ResizeObserver
    eventsTarget: el,
    autoRaf: false, // the hook runs one managed loop instead
    allowNestedScroll: true, // shelves and in-page scrollers keep native scrolling
    lerp: 0.12, // snappier than the 0.1 default so trackpad momentum keeps its tail
    wheelMultiplier: 1, // OS deltas already carry acceleration and momentum
    syncTouch: false,
    respectReducedMotion: true,
  };
}

export function useSmoothScroll(ref: RefObject<HTMLElement | null>): void {
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const lenis = new Lenis(lenisOptions(el));
    let rafId = 0;
    const tick = (time: number) => {
      lenis.raf(time);
      // keep ticking only while an animation is running, then go idle
      if (lenis.isScrolling === 'smooth') rafId = requestAnimationFrame(tick);
      else rafId = 0;
    };
    // fires before Lenis's internal scrollTo, so the loop is live for the first frame
    const offVirtualScroll = lenis.on('virtual-scroll', () => {
      if (!rafId) rafId = requestAnimationFrame(tick);
    });
    return () => {
      offVirtualScroll();
      if (rafId) cancelAnimationFrame(rafId);
      lenis.destroy();
    };
  }, [ref]);
}
