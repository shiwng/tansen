import { PrefetchLink } from '@/components/PrefetchLink';
import type { BrowseCategory } from '@/data/browse/categories';
import { rememberBrowseOrigin } from '@/lib/browseScroll';
import { useCategoryArt } from '@/lib/useCategoryArt';
import { Artwork } from './Artwork';

/**
 * An Apple Music-style category tile: one curated artist portrait from the
 * catalogue, cropped edge to edge, with a light tint and a dark legibility
 * gradient under the label. The tint doubles as the loading and failure
 * fallback, so every tile stays recognisable without an image.
 */
export function BrowseCategoryCard({
  category,
  eager = false,
}: {
  category: BrowseCategory;
  /** Above-the-fold tiles decode eagerly; the rest lazy-load. */
  eager?: boolean;
}) {
  const art = useCategoryArt(category);
  const [from, to] = category.tint;

  return (
    <PrefetchLink
      to={`/browse/${category.id}`}
      state={{ from: 'browse' }}
      onClick={() => rememberBrowseOrigin(category.id)}
      data-browse-category={category.id}
      aria-label={`Browse ${category.label}`}
      className="group/category relative block aspect-[3/2] overflow-hidden rounded-[var(--radius-artwork-large)] bg-joe [&_img]:object-[50%_30%] [&_img]:brightness-125 [&_img]:contrast-[1.08] [&_img]:grayscale [&_img]:mix-blend-multiply transition-transform duration-200 ease-out active:scale-[0.99] focus-visible:ring-[3px] focus-visible:ring-key/70 focus-visible:ring-offset-2 focus-visible:ring-offset-page focus-visible:outline-none motion-reduce:scale-100 motion-reduce:transition-none"
      style={{ backgroundImage: `linear-gradient(135deg, ${from}, ${to})` }}
    >
      <span className="absolute inset-0">
        {/* The frame's own placeholder and hairline would sit between the
            portrait and the tint it multiplies against. */}
        <span className="absolute inset-0 transition-transform duration-500 ease-out group-hover/category:scale-[1.045] motion-reduce:scale-100 motion-reduce:transition-none [&>div]:isolation-auto [&>div]:bg-transparent [&>div]:after:content-none">
          <Artwork
            src={art}
            displayWidth={220}
            radius="inherit"
            className="size-full"
            eager={eager}
          />
        </span>
        <span
          aria-hidden="true"
          className="absolute inset-0"
          style={{ background: 'linear-gradient(180deg, transparent 38%, #000000b8 100%)' }}
        />
      </span>
      <span className="absolute inset-x-0 bottom-0 z-1 px-3.5 pb-3 type-title-3-emphasized text-white [text-shadow:0_1px_6px_#0009]">
        {category.label}
      </span>
    </PrefetchLink>
  );
}
