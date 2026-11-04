import { useEffect, useState, type CSSProperties } from 'react';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { sizedArtwork } from '@/lib/artwork';

const remoteCache = new Map<string, string>();

/**
 * True for a URL that already points at the `art://` protocol: queue rows and
 * library rows store the converted URL, so it must not be converted twice
 * (a double conversion 404s in `serve_art`).
 */
export function isArtworkUrl(src: string): boolean {
  return src.startsWith('art://') || src.startsWith('http://art.localhost/');
}

/** The local `art://` URL for cached artwork, or null when it is not ready. */
export function useArtworkUrl(src: string | null): string | null {
  const [resolved, setResolved] = useState<string | null>(() => {
    if (!src) return null;
    if (isArtworkUrl(src)) return src;
    return src.startsWith('http') ? remoteCache.get(src) ?? null : convertFileSrc(src, 'art');
  });
  useEffect(() => {
    if (!src) {
      setResolved(null);
      return;
    }
    if (isArtworkUrl(src)) {
      setResolved(src);
      return;
    }
    if (!src.startsWith('http')) {
      setResolved(convertFileSrc(src, 'art'));
      return;
    }
    const cached = remoteCache.get(src);
    if (cached) {
      setResolved(cached);
      return;
    }
    let active = true;
    void invoke<string>('fetch_artwork', { url: src })
      .then((path) => {
        if (!active) return;
        const local = convertFileSrc(path, 'art');
        remoteCache.set(src, local);
        setResolved(local);
      })
      .catch(() => {
        if (active) setResolved(null);
      });
    return () => {
      active = false;
    };
  }, [src]);
  return resolved;
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
  const [attempt, setAttempt] = useState(0);
  const resolved = useArtworkUrl(src);
  useEffect(() => {
    setLoaded(false);
    setAttempt(0);
  }, [src]);
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
          onError={() => setAttempt((value) => Math.min(value + 1, 2))}
        />
      )}
    </div>
  );
}
