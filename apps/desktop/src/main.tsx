import React from 'react';
import ReactDOM from 'react-dom/client';
import { RouterProvider } from 'react-router';
import { QueryClientProvider } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { router } from './routes';
import { queryClient } from './lib/queryClient';
import { initPlayerStore } from './state/player';
import { initChrome } from './shell/chrome';
import './styles/global.css';

// Engine events must flow into the player store before the first render reads it.
void initPlayerStore();
initChrome();

// M1 bench instrumentation: log after the first painted frame (double rAF),
// so the bench script can measure process-start → first frontend paint.
let benchSent = false;
function benchFirstPaint() {
  if (benchSent) return;
  benchSent = true;
  invoke("bench_marker", { stage: "first-paint" }).catch(() => {});
}
requestAnimationFrame(() => requestAnimationFrame(benchFirstPaint));
// rAF is throttled for occluded windows; the timeout guarantees the marker.
setTimeout(benchFirstPaint, 500);

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </React.StrictMode>,
);
