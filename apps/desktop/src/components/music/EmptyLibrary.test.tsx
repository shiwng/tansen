// @vitest-environment happy-dom
//
// The shared library empty state: Tansen's own mark, a centred stack of
// heading + context + one compact action, and the discovery action really
// routes to Search (the browse page is Tansen's discovery surface).
import { afterEach, describe, expect, it, vi } from 'vitest';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { EmptyLibrary } from './EmptyLibrary';

const navigateMock = vi.hoisted(() => vi.fn());
vi.mock('react-router', () => ({ useNavigate: () => navigateMock }));

let root: Root | null = null;
let host: HTMLElement | null = null;

function mount(standalone = false) {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  act(() => root!.render(<EmptyLibrary standalone={standalone} />));
  return host;
}

afterEach(() => {
  if (root) {
    act(() => root!.unmount());
    root = null;
    host?.remove();
    host = null;
  }
  navigateMock.mockClear();
});

describe('EmptyLibrary', () => {
  it('renders the genuine Tansen mark, the library copy and one compact action', () => {
    const host = mount();
    expect(host.querySelectorAll('svg rect').length).toBe(3);
    expect(host.textContent).toContain('Add Music to Your Library');
    expect(host.textContent).toContain('Explore music and add your favourites to your Tansen library.');
    const action = host.querySelector('button');
    expect(action?.textContent).toBe('Browse Tansen');
    expect(action?.className).toContain("h-[26px]");
    expect(action?.className).toContain('rounded-[6px]');
  });

  it('navigates to Search from Browse Tansen', () => {
    const host = mount();
    act(() => host.querySelector('button')!.click());
    expect(navigateMock).toHaveBeenCalledWith('/search');
  });

  it('fills the page in standalone mode and leaves room under a section header otherwise', () => {
    const standalone = mount(true);
    expect(standalone.firstElementChild?.className).toContain('--player-pill-height');
    act(() => root!.unmount());
    root = null;
    host?.remove();

    const section = mount();
    expect(section.firstElementChild?.className).not.toContain('--player-pill-height');
  });
});
