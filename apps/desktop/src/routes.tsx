import { createBrowserRouter, Navigate, type RouteObject } from 'react-router';
import { AppShell } from '@/shell/AppShell';
import { RouteError } from '@/shell/AppError';
import { NotFoundPage } from '@/pages/NotFoundPage';
import { PAGE_ROUTES, pageElement } from '@/pageRegistry';

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
];

export const router = createBrowserRouter(routes);
