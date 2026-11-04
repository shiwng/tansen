/*
 * Design Lab — glass A/B for the addendum (item 2): the two Music pill
 * shapes (main-window pill 700×54 R27, full-screen capsule 150×36 R18)
 * rendered CSS-only vs glisse over four backdrops (dark / mid / light /
 * busy tile grid). The busy backdrop carries a 2 px vertical line through
 * every capsule so edge refraction is measurable as line displacement.
 * Geometry per ui-replica/fullscreen-player-reference.md and the measured
 * main-window pill (700×54, bottom 19).
 */
import { useEffect } from 'react';

type Variant = 'css' | 'glisse';

const BACKDROPS: { name: string; style: React.CSSProperties }[] = [
  { name: 'dark', style: { background: '#1f1f22' } },
  { name: 'mid', style: { background: 'linear-gradient(180deg, #5c5d60 0%, #5a5b5e 100%)' } },
  { name: 'light', style: { background: '#f6f6f6' } },
  { name: 'busy', style: { background: 'repeating-linear-gradient(90deg, #b0413e 0 40px, #3e6cb0 40px 80px, #3ea055 80px 120px, #dedee0 120px 160px)' } },
];

function Pill({ variant, capsule }: { variant: Variant; capsule?: boolean }) {
  const shared: React.CSSProperties = capsule
    ? { width: 150, height: 36, borderRadius: 18, display: 'flex', alignItems: 'center', justifyContent: 'center', color: '#f5f7fd', position: 'relative' }
    : { width: 350, height: 54, borderRadius: 27, display: 'flex', alignItems: 'center', color: 'var(--systemPrimary, #fff)', position: 'relative', padding: '0 16px', fontSize: 13 };
  if (variant === 'css') {
    return (
      <div
        style={{
          ...shared,
          background: 'rgba(118,119,122,0.55)',
          backdropFilter: 'blur(18px) saturate(1.5) brightness(1.06)',
          WebkitBackdropFilter: 'blur(18px) saturate(1.5) brightness(1.06)',
          boxShadow: 'inset 0 0.5px 0 0 rgba(240,241,246,0.7)',
        }}
      >
        {capsule ? '× ▢' : 'Pill 700×54 — CSS'}
      </div>
    );
  }
  // glisse was removed (addendum item 2/3: pass-through on this WKWebView);
  // the ghost column documents the measured comparison.
  return (
    <div style={{ ...shared, border: '1px dashed rgba(128,128,128,0.5)' }}>
      {capsule ? '× ▢' : 'Pill — glisse (removed)'}
    </div>
  );
}

export default function LabGlass() {
  useEffect(() => {
    const root = document.documentElement;
    const prev = root.dataset.theme;
    root.dataset.theme = 'dark';
    root.style.background = 'transparent';
    document.body.style.background = 'transparent';
    return () => {
      if (prev === undefined) delete root.dataset.theme;
      else root.dataset.theme = prev;
      root.style.background = '';
      document.body.style.background = '';
    };
  }, []);

  return (
    <div
      data-glisse-backdrop
      style={{ position: 'fixed', inset: 0, overflow: 'hidden', fontFamily: '-apple-system, system-ui, sans-serif', fontSize: 11, color: '#9a9a9e' }}
    >
      {BACKDROPS.map(({ name, style }) => (
        <section key={name} data-backdrop={name} style={{ ...style, height: 200, position: 'relative', borderBottom: '1px solid #000' }}>
          {/* the refraction probe line passes through every capsule in this section */}
          <div style={{ position: 'absolute', left: 373, top: 0, width: 2, height: '100%', background: '#ffffff' }} />
          <div style={{ position: 'absolute', left: 12, top: 6 }}>PILL 350 (half of 700 shown) — {name}</div>
          <div style={{ position: 'absolute', left: 200, top: 30, display: 'flex', gap: 20 }}>
            <Pill variant="css" />
            <Pill variant="glisse" />
          </div>
          <div style={{ position: 'absolute', left: 12, top: 110 }}>CAPSULE — {name}</div>
          <div style={{ position: 'absolute', left: 300, top: 100, display: 'flex', gap: 20 }}>
            <Pill variant="css" capsule />
            <Pill variant="glisse" capsule />
          </div>
        </section>
      ))}
    </div>
  );
}
