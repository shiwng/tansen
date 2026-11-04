/*
 * Design Lab — text parity (addendum item 5): the sidebar strings at the
 * measured sidebar geometry (13 pt labels, icon column, x inset) on the
 * vibrancy backdrop, so OCR boxes can be diffed 1:1 against the live
 * Music.app sidebar capture (same window size).
 */
import { useEffect } from 'react';

const ROWS = ['Search', 'Home', 'Radio', 'Recently Added', 'Artists', 'Albums', 'Songs', 'Settings', 'Playlists'];

export default function LabText() {
  useEffect(() => {
    const root = document.documentElement;
    root.style.background = 'transparent';
    document.body.style.background = 'transparent';
    return () => {
      root.style.background = '';
      document.body.style.background = '';
    };
  }, []);

  return (
    <div style={{ position: 'fixed', inset: 0, background: '#26262b', fontFamily: '-apple-system, system-ui, sans-serif' }}>
      <nav style={{ position: 'absolute', inset: '0 auto 0 0', width: 208, paddingTop: 52, paddingLeft: 12, paddingRight: 12 }}>
        {ROWS.map((label) => (
          <div key={label} style={{ height: 32, display: 'flex', alignItems: 'center', borderRadius: 8, padding: '0 10px' }}>
            <span style={{ width: 16, height: 16, marginRight: 6, background: '#3a3a3e', borderRadius: 4 }} />
            <span style={{ fontSize: 13, color: '#ffffffeb' }}>{label}</span>
          </div>
        ))}
      </nav>
      <div style={{ position: 'absolute', left: 300, top: 20, fontSize: 12, color: '#888' }}>
        rows at the measured sidebar geometry — OCR-diff against Music's sidebar labels
      </div>
    </div>
  );
}
