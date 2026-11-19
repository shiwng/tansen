import React from 'react';
import ReactDOM from 'react-dom/client';
import { RouterProvider } from 'react-router';
import { QueryClientProvider } from '@tanstack/react-query';
import { router } from './routes';
import { queryClient } from './lib/queryClient';
import { initPlayerStore } from './state/player';
import { initGoogle } from './state/google';
import { initChrome } from './shell/chrome';
import 'glisse/styles.css';
import './styles/global.css';

// Engine events must flow into the player store before the first render reads it.
void initPlayerStore();
initChrome();
// Google session restore (profile only; tokens live in the Keychain).
void initGoogle();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </React.StrictMode>,
);
