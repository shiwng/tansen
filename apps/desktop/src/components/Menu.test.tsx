// @vitest-environment happy-dom
//
// The shared context-menu contract used by TrackMenu: compact 24px rows,
// arrow/Tab focus routing, a real submenu that Escape closes before the menu,
// and focus returning to the submenu's trigger.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, useEffect, useRef, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { Menu, MenuItem, MenuSubmenu } from './Menu';

// React 19 reads this flag to decide whether act() is supported.
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Harness({ onClose }: { onClose: () => void }) {
  const trigger = useRef<HTMLButtonElement>(null);
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  useEffect(() => setAnchor(trigger.current), []);
  return (
    <>
      <button ref={trigger}>trigger</button>
      {anchor && (
        <Menu
          anchor={anchor}
          onClose={() => {
            onClose();
            setAnchor(null);
          }}
          label="Track options"
        >
          <MenuItem onSelect={() => undefined}>Play Next</MenuItem>
          <MenuSubmenu label="Add to Playlist">
            <MenuItem onSelect={() => undefined}>Road Trip</MenuItem>
            <MenuItem onSelect={() => undefined}>Faves</MenuItem>
          </MenuSubmenu>
        </Menu>
      )}
    </>
  );
}

let root: Root | null = null;
let host: HTMLElement | null = null;

function mount(onClose: () => void) {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  act(() => root!.render(<Harness onClose={onClose} />));
}

function key(target: Element | Document, value: string, shiftKey = false) {
  act(() => {
    target.dispatchEvent(new KeyboardEvent('keydown', { key: value, shiftKey, bubbles: true, cancelable: true }));
  });
}

const menus = () => [...document.querySelectorAll<HTMLElement>('[role="menu"]')];
const activeText = () => document.activeElement?.textContent?.trim();

afterEach(() => {
  if (root) {
    act(() => root!.unmount());
    root = null;
    host?.remove();
    host = null;
  }
});

describe('Menu', () => {
  it('focuses the first row, moves with arrows and wraps with Tab', () => {
    mount(vi.fn());
    expect(menus().length).toBe(1);
    expect(activeText()).toBe('Play Next');

    key(document, 'ArrowDown');
    expect(activeText()).toBe('Add to Playlist');

    key(document, 'ArrowDown');
    expect(activeText()).toBe('Play Next');

    key(document, 'ArrowUp');
    expect(activeText()).toBe('Add to Playlist');

    key(document, 'Tab', true);
    expect(activeText()).toBe('Play Next');
  });

  it('opens a submenu with ArrowRight and closes it first on Escape', () => {
    const onClose = vi.fn();
    mount(onClose);
    key(document, 'ArrowDown');
    key(document.activeElement!, 'ArrowRight');
    expect(menus().length).toBe(2);
    expect(activeText()).toBe('Road Trip');

    key(document, 'Escape');
    expect(menus().length).toBe(1);
    expect(activeText()).toBe('Add to Playlist');
    expect(onClose).not.toHaveBeenCalled();

    key(document, 'Escape');
    expect(menus().length).toBe(0);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('opens a submenu on click and closes the whole menu from the scrim', () => {
    const onClose = vi.fn();
    mount(onClose);
    const trigger = [...document.querySelectorAll('button')].find((b) => b.textContent?.includes('Add to Playlist'));
    act(() => trigger!.click());
    expect(menus().length).toBe(2);
    expect(document.body.textContent).toContain('Road Trip');

    const scrim = document.querySelector<HTMLElement>('[aria-hidden="true"].fixed.inset-0');
    act(() => scrim!.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true })));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
