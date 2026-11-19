import { useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';
import type { BrowseCategory } from '@/data/browse/categories';
import { pickCategoryArt, readCachedCategoryArt, writeCachedCategoryArt } from './categoryArt';
import { discoverySearch } from './discovery';

const WEEK_MS = 7 * 24 * 60 * 60 * 1000;

/**
 * The category's playlist artwork, from the same real catalogue search the
 * detail page uses. A cached URL is treated as fresh for the whole TTL, so
 * opening Browse Categories does not hit the sidecar for every tile again.
 */
export function useCategoryArt(category: BrowseCategory | undefined): string | null {
  const cached = useMemo(
    () => (category ? readCachedCategoryArt(category.id) : null),
    [category],
  );
  const query = useQuery({
    queryKey: ['browse-category-art', category?.id],
    queryFn: async () => {
      if (!category) return null;
      // Artists, not playlists: a playlist thumbnail is a four-cover mosaic with
      // titles baked in; an artist card is one clean portrait.
      const page = await discoverySearch(category.art, 'artists', 8);
      const url = pickCategoryArt(page.cards, category.art);
      if (url) writeCachedCategoryArt(category.id, url);
      return url;
    },
    enabled: Boolean(category) && !cached,
    initialData: cached ?? undefined,
    staleTime: WEEK_MS,
    gcTime: 2 * WEEK_MS,
    retry: 1,
  });
  return query.data ?? cached;
}
