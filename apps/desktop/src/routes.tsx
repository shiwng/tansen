import { createBrowserRouter, Navigate, type RouteObject } from 'react-router';
import { AppShell } from '@/shell/AppShell';
import { RouteError } from '@/shell/AppError';
import { NotFoundPage } from '@/pages/NotFoundPage';
import { PAGE_ROUTES, pageElement } from '@/pageRegistry';

// Design Lab: dev-only routes, dynamically imported so the release bundle
// never contains them (guarded by lab/lab-route-grep.test.ts).
const labRoutes: RouteObject[] = import.meta.env.DEV
  ? [
      {
        path: '/__lab/fullscreen-player',
        lazy: async () => ({ Component: (await import('@/lab/LabFullscreenPlayer')).default }),
      },
      {
        path: '/__lab/glass',
        lazy: async () => ({ Component: (await import('@/lab/LabGlass')).default }),
      },
      {
        path: '/__lab/scroll',
        lazy: async () => ({ Component: (await import('@/lab/LabScroll')).default }),
      },
      {
        path: '/__lab/text',
        lazy: async () => ({ Component: (await import('@/lab/LabText')).default }),
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
