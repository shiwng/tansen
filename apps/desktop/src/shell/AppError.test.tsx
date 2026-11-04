// @vitest-environment happy-dom
// A crashing Settings section renders its own inline message instead of
// taking the whole route to the recovery screen.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react';
import { AppError } from './AppError';

let root: Root | null = null;

function Boom(): never {
  throw new Error('section exploded');
}

describe('AppError inline', () => {
  afterEach(() => {
    if (root) {
      act(() => root!.unmount());
      root = null;
    }
    document.body.innerHTML = '';
  });

  it('catches a child render error and offers Retry', () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
    const host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
    act(() => {
      root!.render(
        <AppError inline>
          <Boom />
        </AppError>,
      );
    });
    expect(host.textContent).toContain("This section couldn't load");
    expect(host.textContent).toContain('section exploded');
    expect(host.textContent).toContain('Retry');
    spy.mockRestore();
  });
});
