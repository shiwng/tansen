import { createBrowserRouter, Navigate, type RouteObject } from 'react-router';
import { AppShell } from '@/shell/AppShell';
import { RouteError } from '@/shell/AppError';
import { NotFoundPage } from '@/pages/NotFoundPage';
import { PAGE_ROUTES, pageElement } from '@/pageRegistry';

// Design Lab: dev-only route, dynamically imported so the release bundle
// never contains it (guarded by lab/lab-route-grep.test.ts).
const labRoutes: RouteObject[] = import.meta.env.DEV
  ? [
      {
        path: '/__lab/fullscreen-player',
        // Bypasses AppShell on purpose: the Lab must render without the
        // production chrome so the fixture is the whole window.
        lazy: async () => ({ Component: (await import('@/lab/LabFullscreenPlayer')).default }),
      },
    ]
  : [];

const routes: RouteObject[] = [
  {
    element: <AppShell />,
    // A render crash shows the recovery screen instead of React Router's
    // "Unexpected Application Error!" dead end.
    errorElement: <RouteError />,
    children: [
      { index: true, element: <Navigate to="/home" replace /> },
      ...PAGE_ROUTES.map((route) => ({ path: route.path, element: pageElement(route) })),
      { path: '*', element: <NotFoundPage /> },
    ],
  },
  ...labRoutes,
];

export const router = createBrowserRouter(routes);
