import { describe, expect, it } from 'vitest';

/**
 * Guard: the Design Lab must be dev-only. Production code must never
 * import it, and the routes gate must stay on import.meta.env.DEV.
 * Source texts are read via Vite's glob-raw (no node types needed).
 * The dist check lives in scripts/ui-compare/check-release-lab.sh and
 * runs as part of each batch gate.
 */
const sources = import.meta.glob<string>('/src/**/*.{ts,tsx}', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

describe('design lab guard', () => {
  it('is not imported by production source outside routes.tsx and the lab', () => {
    const offenders = Object.entries(sources)
      .filter(([path]) => !path.includes('/src/lab/') && !path.endsWith('/src/routes.tsx'))
      .filter(([, text]) => /from\s+['"]@?\/?lab\//.test(text));
    expect(offenders.map(([path]) => path)).toEqual([]);
  });

  it('routes.tsx gates the lab on import.meta.env.DEV', () => {
    const routes = sources['/src/routes.tsx'] ?? '';
    const guarded = /labRoutes[^;]*import\.meta\.env\.DEV/.test(routes);
    expect(guarded, 'lab routes must be gated on import.meta.env.DEV').toBe(true);
  });
});
