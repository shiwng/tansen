/**
 * The Browse Categories navigation memory. The category grid lives in the
 * page scroller, so its scroll offset and the card that opened it are saved
 * before navigating; returning from a category restores both instead of
 * dropping the user at the top.
 */
const BROWSE_SCROLL_KEY = 'tansen.browse.categories.scroll';
const BROWSE_LAST_KEY = 'tansen.browse.categories.last';

const CATEGORY_SCROLL_PREFIX = 'tansen.browse.category-scroll.';

function scroller(): HTMLElement | null {
  return document.getElementById('scrollable-page');
}

export function rememberBrowseOrigin(categoryId: string): void {
  try {
    const page = scroller();
    if (page) sessionStorage.setItem(BROWSE_SCROLL_KEY, String(page.scrollTop));
    sessionStorage.setItem(BROWSE_LAST_KEY, categoryId);
  } catch {
    // Session storage can be unavailable; navigation still works.
  }
}

/** Restores the grid after a back navigation. Returns an effect cleanup. */
export function restoreBrowseOrigin(): () => void {
  let scrollTop: number | null = null;
  let last: string | null = null;
  try {
    const raw = sessionStorage.getItem(BROWSE_SCROLL_KEY);
    const parsed = raw == null ? Number.NaN : Number(raw);
    scrollTop = Number.isFinite(parsed) ? parsed : null;
    last = sessionStorage.getItem(BROWSE_LAST_KEY);
    sessionStorage.removeItem(BROWSE_SCROLL_KEY);
    sessionStorage.removeItem(BROWSE_LAST_KEY);
  } catch {
    return () => {};
  }
  if (scrollTop == null) return () => {};

  const top = Math.max(0, scrollTop);
  let second = 0;
  // Two frames: the first lets the grid lay out, the second lets Lenis settle
  // before the offset is written.
  const first = requestAnimationFrame(() => {
    second = requestAnimationFrame(() => {
      const page = scroller();
      if (page) page.scrollTop = top;
      if (last) {
        document
          .querySelector<HTMLElement>(`[data-browse-category="${last}"]`)
          ?.focus({ preventScroll: true });
      }
    });
  });
  return () => {
    cancelAnimationFrame(first);
    cancelAnimationFrame(second);
  };
}

/** Per-category scroll, so a playlist visit and back keeps the shelf position. */
export function categoryScrollKey(id: string): string {
  return `${CATEGORY_SCROLL_PREFIX}${id}`;
}

export function readPageScroll(key: string): number | null {
  try {
    const raw = sessionStorage.getItem(key);
    const parsed = raw == null ? Number.NaN : Number(raw);
    return Number.isFinite(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

export function writePageScroll(key: string): void {
  try {
    const page = scroller();
    if (page) sessionStorage.setItem(key, String(page.scrollTop));
  } catch {
    // Nothing to restore next time; the page still works.
  }
}
