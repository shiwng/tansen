import { useState, type CSSProperties } from 'react';
import { convertFileSrc } from '@tauri-apps/api/core';
import { sizedArtwork } from '@/lib/artwork';

function resolveSrc(src: string): string {
  return src.startsWith('http') ? src : convertFileSrc(src, 'art');
}

interface ArtworkProps {
  src: string | null;
  /** The CSS pixel width it renders at, so the CDN is asked for that size (prompt §3). */
  displayWidth: number;
  /** width / height. */
  aspect?: number;
  radius?: CSSProperties['borderRadius'];
  className?: string;
  /** Eager for above-the-fold heroes; lazy everywhere else. */
  eager?: boolean;
}

const frame = 'relative isolate w-full overflow-hidden bg-joe artwork-stroke';

/**
 * Fixed-aspect artwork: a neutral placeholder until the image decodes, then a
 * short fade (prompt §3). The 0.5px inner stroke is Apple's "has-borders"
 * treatment (report §1).
 */
export function Artwork({ src, displayWidth, aspect = 1, radius, className, eager }: ArtworkProps) {
  const [loaded, setLoaded] = useState(false);
  // googleusercontent answers bursts with 429 (lib/artwork.ts). A failed load
  // is retried once after a pause, then falls back to YouTube's own URL.
  const [attempt, setAttempt] = useState(0);
  // Local artwork rides the art:// protocol; catalogue thumbnails are plain
  // https URLs and pass through untouched.
  const resolved = src ? resolveSrc(src) : null;
  const sized = resolved ? sizedArtwork(resolved, displayWidth, aspect) : null;
  const url = attempt >= 2 || sized === resolved ? resolved : sized;
  return (
    <div
      className={className ? `${frame} ${className}` : frame}
      style={{ aspectRatio: String(aspect), borderRadius: radius }}
    >
      {url && (
        <img
          key={`${url}#${attempt}`}
          className="absolute inset-0 size-full object-cover opacity-0 transition-opacity duration-250 ease-in data-loaded:opacity-100"
          data-loaded={loaded || undefined}
          src={url}
          alt=""
          // googleusercontent throttles hotlinked images (429) by Referer;
          // without one every artwork on a page loads (measured on Artist).
          referrerPolicy="no-referrer"
          loading={eager ? 'eager' : 'lazy'}
          decoding="async"
          draggable={false}
          onLoad={() => setLoaded(true)}
          onError={() => {
            if (attempt === 0) setTimeout(() => setAttempt(1), 1500);
            else if (attempt === 1 && url !== src) setAttempt(2);
          }}
        />
      )}
    </div>
  );
}
