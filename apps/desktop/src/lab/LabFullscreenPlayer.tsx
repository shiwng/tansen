/*
 * Design Lab — feasibility spike for the Apple Music full-screen player.
 * DEV-only (import.meta.env.DEV; never imported by production routes —
 * guarded by lab-route-grep.test.ts). Fixture geometry comes from the
 * measured reference (ui-replica/fullscreen-player-reference.md, window
 * 1280×820 pt): three glass-capsule implementations side by side so the
 * toolkit can diff each against the real Music.app capsule crops.
 */
import { useEffect } from 'react';

const BG = 'linear-gradient(180deg, #5c5d60 0%, #5a5b5e 100%)';

function Capsule({
  variant,
  children,
}: {
  variant: 'css' | 'glisse' | 'glisse-refract';
  children: React.ReactNode;
}) {
  const shared: React.CSSProperties = {
    width: 150,
    height: 36,
    borderRadius: 18,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 14,
    color: '#f5f7fd',
    position: 'relative',
  };
  if (variant === 'css') {
    return (
      <div
        style={{
          ...shared,
          background: 'rgba(118,119,122,0.55)',
          backdropFilter: 'blur(18px) saturate(1.5)',
          WebkitBackdropFilter: 'blur(18px) saturate(1.5)',
          boxShadow: 'inset 0 0.5px 0 0 rgba(240,241,246,0.7)',
        }}
      >
        {children}
      </div>
    );
  }
  return (
    <div
      data-variant={variant}
      style={shared}
    >
      {children}
    </div>
  );
}

/** Close × + artwork toggle glyphs, matched to the reference sizes. */
function CapsuleGlyphs() {
  return (
    <>
      <svg width="13" height="13" viewBox="0 0 13 13" aria-hidden>
        <path d="M1.5 1.5 L11.5 11.5 M11.5 1.5 L1.5 11.5" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
      </svg>
      <svg width="21.5" height="17.5" viewBox="0 0 22 18" aria-hidden>
        <rect x="0.75" y="0.75" width="14.5" height="12.5" rx="2.5" stroke="currentColor" strokeWidth="1.5" fill="none" />
        <path d="M15.5 6.5 L21 3 V11 L15.5 8 Z" fill="currentColor" />
      </svg>
    </>
  );
}

export default function LabFullscreenPlayer() {
  // The Lab page lets the native window vibrancy show through; every other
  // page keeps its opaque background. Restored on unmount.
  useEffect(() => {
    const html = document.documentElement;
    const body = document.body;
    html.style.background = 'transparent';
    body.style.background = 'transparent';
    html.dataset.theme = 'dark';
    return () => {
      html.style.background = '';
      body.style.background = '';
      delete html.dataset.theme;
    };
  }, []);

  return (
    <div
      data-glisse-backdrop
      style={{
        position: 'fixed',
        inset: 0,
        background: BG,
        fontFamily: '-apple-system, system-ui, sans-serif',
        overflow: 'hidden',
      }}
    >
      {/* Reference geometry: artwork 262.5pt at (189, 194.5), title cap-top ~543.5 */}
      <div
        style={{
          position: 'absolute',
          left: 189,
          top: 194.5,
          width: 262.5,
          height: 262.5,
          borderRadius: 10,
          background: '#262629',
          boxShadow: '0 12px 28px rgba(0,0,0,0.35)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
        }}
      >
        <svg width="88" height="108" viewBox="0 0 88 108" aria-hidden>
          <path
            d="M30 12 L74 4 V72 C74 84 66 90 56 92 C46 94 38 90 38 82 C38 74 46 68 56 66 L62 64.5 V22 L38 27 V86 C38 98 30 104 20 106 C10 108 2 104 2 96 C2 88 10 82 20 80 L30 77.5 Z"
            fill="#626267"
          />
        </svg>
      </div>
      <div
        style={{
          position: 'absolute',
          left: 141,
          top: 543.5 - 12.5,
          fontSize: 18,
          fontWeight: 700,
          color: '#ffffff',
        }}
      >
        Not Playing
      </div>
      {/* Scrubber: x 140→500, 6pt tall, y 597 */}
      <div style={{ position: 'absolute', left: 140, top: 597, width: 360, height: 6, borderRadius: 3, background: '#898a8d' }} />
      <div style={{ position: 'absolute', left: 140, top: 614, fontSize: 11, color: '#8f9093' }}>--:--</div>
      <div style={{ position: 'absolute', left: 476, top: 614, fontSize: 11, color: '#8f9093' }}>--:--</div>

      {/* THE SPIKE: three glass implementations of the same 150×36 capsule,
          stacked for one-shot pixel comparison against the reference crop.
          Variant positions recorded for the toolkit. */}
      <div style={{ position: 'absolute', left: 620, top: 120, display: 'flex', flexDirection: 'column', gap: 28 }}>
        <div data-spike-row="css"><Capsule variant="css"><CapsuleGlyphs /></Capsule></div>
        <div data-spike-row="glisse-tuned">
          <div style={{ width: 150, height: 36, borderRadius: 18, display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 14, color: '#f5f7fd', background: 'rgba(118,119,122,0.4)', backdropFilter: 'blur(18px) saturate(1.5) brightness(1.1)', WebkitBackdropFilter: 'blur(18px) saturate(1.5) brightness(1.1)', boxShadow: 'inset 0 0.5px 0 0 rgba(240,241,246,0.85)' }}>
            <CapsuleGlyphs />
          </div>
        </div>
        <div data-spike-row="glisse-refract"><Capsule variant="glisse-refract"><CapsuleGlyphs /></Capsule></div>
      </div>
      
      <div style={{ position: 'absolute', left: 820, top: 120, fontSize: 12, color: '#dfe0e3', lineHeight: 1.7 }}>
        <div>variant rows top→bottom: css | glisse | glisse-refract</div>
        <div>linear() easing support: {String(CSS.supports('animation-timing-function', 'linear(0, 1)'))}</div>
        <div>backdrop-filter support: {String(CSS.supports('backdrop-filter', 'blur(1px)'))}</div>
        <div>UA: {navigator.userAgent.slice(0, 80)}</div>
        <div>glass fill reference: #76777a on #5b5c5f (+26 lighten), rim #f0f1f6</div>
      </div>

      {/* Bottom-right reference capsule for distance comparisons */}
      <div style={{ position: 'absolute', right: 8.5, bottom: 11.5 }}>
        <Capsule variant="css">
          <svg width="17" height="12.5" viewBox="0 0 17 13" aria-hidden>
            <path d="M1 1 H11 M1 6.5 H16 M1 12 H11" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
          </svg>
        </Capsule>
      </div>
    </div>
  );
}
