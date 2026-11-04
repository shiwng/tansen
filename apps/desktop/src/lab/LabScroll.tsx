/*
 * Design Lab — scroll/perf harness (addendum item 3): 200 tiles scrolling
 * under a fixed glass pill, CSS vs glisse (?glass=glisse). The page drives
 * its own scroll during measurement and reports rAF-gap statistics on screen
 * (frame-pipeline dropped-frame proxy; recorder-grade numbers come with the
 * Phase-2 ScreenCaptureKit recorder). Dev-only.
 */
import { useEffect, useRef, useState } from 'react';

const TILE = 120;
const ROWS = 15;
const COLS = 12; // 180 tiles + header fill = 200 tiles total
const TILES = ROWS * COLS + 20;

export default function LabScroll() {
  const scroller = useRef<HTMLDivElement>(null);
  const [report, setReport] = useState<string>('measuring…');

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const root = document.documentElement;
    const prev = root.dataset.theme;
    root.dataset.theme = 'dark';
    root.style.background = 'transparent';
    document.body.style.background = 'transparent';

    // Measure: scroll to the bottom and back twice, counting rAF gaps.
    const frame = 1000 / 120; // display refresh; gaps are reported vs both 120 and 60 Hz
    const gaps: number[] = [];
    let last = performance.now();
    let raf = 0;
    let runs = 0;
    const tick = (now: number) => {
      const d = now - last;
      last = now;
      if (d > frame * 1.5) gaps.push(Math.round(d));
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);

    const startTime = performance.now();
    let dir = 1;
    const step = () => {
      if (!el) return;
      el.scrollTop += dir * 14;
      if (el.scrollTop >= el.scrollHeight - el.clientHeight) dir = -1;
      if (el.scrollTop <= 0 && dir === -1) {
        dir = 1;
        runs += 1;
        if (runs >= 2) {
          cancelAnimationFrame(raf);
          const wall = Math.round(performance.now() - startTime);
          const over120 = gaps.filter((g) => g > 1000 / 60 * 1.5).length;
          setReport(
            `frames with gap>1.5×(1/120s): ${gaps.length} · of those >1.5×(1/60s): ${over120} · max gap ${gaps.length ? Math.max(...gaps) : 0}ms · wall ${wall}ms · mode css`,
          );
          return;
        }
      }
      requestAnimationFrame(() => step());
    };
    requestAnimationFrame(() => step());

    return () => {
      cancelAnimationFrame(raf);
      if (prev === undefined) delete root.dataset.theme;
      else root.dataset.theme = prev;
      root.style.background = '';
      document.body.style.background = '';
    };
  }, []);

  const pill = (
    <div
      style={{
        width: 350,
        height: 54,
        borderRadius: 27,
        display: 'flex',
        alignItems: 'center',
        paddingLeft: 16,
        color: '#fff',
        position: 'relative',
        background: 'rgba(118,119,122,0.55)',
        backdropFilter: 'blur(18px) saturate(1.5) brightness(1.06)',
        WebkitBackdropFilter: 'blur(18px) saturate(1.5) brightness(1.06)',
        boxShadow: 'inset 0 0.5px 0 0 rgba(240,241,246,0.7)',
      }}
    >
      Pill — CSS over scrolling tiles
    </div>
  );

  return (
    <div style={{ position: 'fixed', inset: 0, fontFamily: '-apple-system, system-ui, sans-serif' }}>
      <div
        ref={scroller}
        style={{ position: 'absolute', inset: 0, overflowY: 'auto', display: 'grid', gridTemplateColumns: `repeat(${COLS}, ${TILE}px)`, gap: 12, padding: 20, alignContent: 'start' }}
      >
        {Array.from({ length: TILES }, (_, i) => (
          <div
            key={i}
            style={{
              width: TILE,
              height: TILE,
              borderRadius: 10,
              background: `hsl(${(i * 47) % 360} 45% ${30 + (i % 4) * 12}%)`,
              display: 'flex',
              alignItems: 'flex-end',
              padding: 6,
              boxSizing: 'border-box',
              color: '#fff',
              fontSize: 11,
            }}
          >
            {i + 1}
          </div>
        ))}
      </div>
      <div style={{ position: 'absolute', left: 0, right: 0, bottom: 24, display: 'flex', justifyContent: 'center' }}>{pill}</div>
      <div
        style={{
          position: 'absolute',
          top: 10,
          left: 10,
          background: 'rgba(0,0,0,0.75)',
          color: '#0f0',
          font: '11px ui-monospace, monospace',
          padding: '6px 10px',
          borderRadius: 8,
          maxWidth: 640,
        }}
      >
        {report}
      </div>
    </div>
  );
}
