import { Check, ChevronRight } from 'lucide-react';
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type CSSProperties,
  type ReactNode,
  type Ref,
} from 'react';
import { createPortal } from 'react-dom';

export type MenuPlacement = 'above-start' | 'above-end' | 'below-end' | 'left-start';

interface MenuProps {
  /** The element the menu opens from. */
  anchor: HTMLElement;
  placement?: MenuPlacement;
  onClose: () => void;
  label: string;
  /** At least as wide as the anchor (a pop-up button's menu). */
  matchWidth?: boolean;
  /** Fixed panel width in px (a settings popover, not a 185px context menu). */
  width?: number;
  /** Cap on panel height in px; longer content scrolls. */
  maxHeight?: number;
  children: ReactNode;
}

/** The shared Apple context-menu surface (report §8). */
const panelClass =
  'fixed animate-menu-in overflow-y-auto overscroll-contain rounded-[10px] bg-context-menu px-[7px] py-[5px] shadow-[0_0_0_0.5px_var(--vibrantDivider),0_10px_40px_var(--glassMaterialShadowColor)] [backdrop-filter:var(--dynamicBlurMenu)]';

const rowSelector = '[role^="menuitem"]:not(:disabled)';

/** The root menu asks an open submenu to close without toggling it. */
const closeSubmenuEvent = 'menu-submenu-close';

function itemsIn(panel: HTMLElement | null): HTMLElement[] {
  return [...(panel?.querySelectorAll<HTMLElement>(rowSelector) ?? [])];
}

/**
 * Apple's contextual menu: a glass panel, 24px rows of 13px items, hairline
 * separators, a blue selection, red for destructive items. Closes on Escape,
 * a click outside, or a scroll of the page under it. Below-edge placements
 * flip above the anchor when the room underneath is too small.
 *
 * Submenus (`MenuSubmenu`) portal their own panel. The root owns keyboard
 * routing: arrows stay inside whichever panel has focus, Escape and ArrowLeft
 * close an open submenu before the menu itself, and Tab cycles the panel.
 */
export function Menu({
  anchor,
  placement = 'below-end',
  onClose,
  label,
  matchWidth,
  width,
  maxHeight,
  children,
}: MenuProps) {
  const panel = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const [position, setPosition] = useState<CSSProperties>({ visibility: 'hidden' });

  useLayoutEffect(() => {
    const rect = anchor.getBoundingClientRect();
    const gap = 6;
    const cap = maxHeight ?? Number.POSITIVE_INFINITY;
    const aboveRoom = Math.max(0, rect.top - 2 * gap);
    const belowRoom = Math.max(0, window.innerHeight - rect.bottom - 2 * gap);
    const horizontal = width ? { width } : matchWidth ? { minWidth: rect.width } : {};
    const bounds = { maxWidth: 'calc(100vw - 16px)', ...horizontal };
    // A capped settings popover flips above as soon as it cannot show its
    // full height below; a short context menu only flips when the room left
    // underneath is genuinely cramped.
    const wantsAbove =
      placement === 'above-start' ||
      placement === 'above-end' ||
      (placement === 'below-end' &&
        aboveRoom > belowRoom &&
        (maxHeight !== undefined ? belowRoom < maxHeight : belowRoom < 240));
    setPosition(
      wantsAbove
        ? {
            ...(placement === 'above-start'
              ? { left: Math.max(8, rect.left) }
              : { right: Math.max(8, window.innerWidth - rect.right) }),
            bottom: window.innerHeight - rect.top + gap,
            maxHeight: Math.min(cap, aboveRoom),
            ...bounds,
          }
        : placement === 'left-start'
          ? {
              right: window.innerWidth - rect.left + gap,
              top: Math.max(8, rect.top),
              maxHeight: Math.min(cap, window.innerHeight - rect.top - 2 * gap),
              ...bounds,
            }
          : {
              right: Math.max(8, window.innerWidth - rect.right),
              top: rect.bottom + gap,
              maxHeight: Math.min(cap, belowRoom),
              ...bounds,
            },
    );
  }, [anchor, placement, matchWidth, width, maxHeight]);

  useEffect(() => {
    itemsIn(panel.current)[0]?.focus({ preventScroll: true });
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        // Close the deepest open panel first, returning focus to its trigger.
        const trigger = panel.current?.querySelector<HTMLElement>('[data-submenu-open="true"]');
        event.preventDefault();
        event.stopPropagation();
        if (trigger) {
          trigger.dispatchEvent(new Event(closeSubmenuEvent));
          trigger.focus({ preventScroll: true });
        } else {
          onCloseRef.current();
          anchor.focus();
        }
        return;
      }

      // Let the caret move inside a text field; only menu rows navigate.
      const target = event.target as HTMLElement | null;
      if (target && ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName)) return;

      if (event.key === 'ArrowLeft') {
        const trigger = panel.current?.querySelector<HTMLElement>('[data-submenu-open="true"]');
        if (trigger) {
          event.preventDefault();
          event.stopPropagation();
          trigger.dispatchEvent(new Event(closeSubmenuEvent));
          trigger.focus({ preventScroll: true });
        }
        return;
      }

      if (!['ArrowDown', 'ArrowUp', 'Home', 'End', 'Tab'].includes(event.key)) return;
      // Scope arrows to the panel that currently has focus: the root or an
      // open submenu.
      const active = (document.activeElement as HTMLElement | null)?.closest<HTMLElement>('[data-menu-panel]');
      const items = itemsIn(active ?? panel.current);
      if (!items.length) return;
      event.preventDefault();
      const index = items.indexOf(document.activeElement as HTMLElement);
      let next: number;
      if (event.key === 'Home') next = 0;
      else if (event.key === 'End') next = items.length - 1;
      else if (event.key === 'Tab') next = index < 0 ? 0 : (index + (event.shiftKey ? -1 : 1) + items.length) % items.length;
      else if (event.key === 'ArrowDown') next = index < 0 ? 0 : (index + 1) % items.length;
      else next = index < 0 ? items.length - 1 : (index - 1 + items.length) % items.length;
      items[next]?.focus({ preventScroll: true });
    };
    const onScroll = (event: Event) => {
      const target = event.target;
      if (target instanceof Element && target.closest('[data-menu-panel]')) return;
      onCloseRef.current();
    };
    const onResize = () => onCloseRef.current();
    document.addEventListener('keydown', onKey, true);
    document.addEventListener('scroll', onScroll, true);
    window.addEventListener('resize', onResize);
    return () => {
      document.removeEventListener('keydown', onKey, true);
      document.removeEventListener('scroll', onScroll, true);
      window.removeEventListener('resize', onResize);
    };
  }, [anchor]);

  return createPortal(
    <>
      <div
        className="fixed inset-0 z-[calc(var(--z-contextual-menus)-1)]"
        onPointerDown={() => onCloseRef.current()}
        aria-hidden="true"
      />
      <div
        ref={panel}
        data-menu-panel=""
        className={`${panelClass} z-(--z-contextual-menus) min-w-[185px]`}
        style={position}
        role="menu"
        aria-label={label}
      >
        {children}
      </div>
    </>,
    document.body,
  );
}

interface MenuItemProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'onClick' | 'children' | 'role'> {
  onSelect: () => void;
  icon?: ReactNode;
  /** Shows a checkmark (a radio-style choice); null leaves the space empty. */
  checked?: boolean | null;
  destructive?: boolean;
  disabled?: boolean;
  /** Extra text at the trailing edge. */
  trailing?: ReactNode;
  /** The underlying button, for submenu triggers that position against it. */
  buttonRef?: Ref<HTMLButtonElement>;
  children: ReactNode;
}

export function MenuItem({
  onSelect,
  icon,
  checked,
  destructive,
  disabled,
  trailing,
  buttonRef,
  children,
  className,
  ...rest
}: MenuItemProps) {
  return (
    <button
      ref={buttonRef}
      {...rest}
      type="button"
      className={`flex min-h-6 w-full items-center gap-2 rounded-[5px] px-2 py-[3px] text-start type-body text-primary enabled:hover:bg-menu-selection enabled:hover:text-white focus-visible:bg-menu-selection focus-visible:text-white disabled:text-tertiary data-destructive:text-destructive ${className ?? ''}`}
      role={checked === undefined ? 'menuitem' : 'menuitemradio'}
      aria-checked={checked === undefined ? undefined : !!checked}
      data-destructive={destructive || undefined}
      disabled={disabled}
      onClick={onSelect}
    >
      {checked !== undefined && (
        <span className="inline-grid w-3.5 flex-none place-items-center text-current">
          {checked && <Check size={14} strokeWidth={2.6} />}
        </span>
      )}
      {icon && <span className="inline-grid w-5 flex-none place-items-center opacity-70">{icon}</span>}
      <span className="min-w-0 flex-1 truncate">{children}</span>
      {trailing && <span className="flex-none opacity-70">{trailing}</span>}
    </button>
  );
}

interface MenuSubmenuProps {
  /** The row that opens the submenu. */
  label: ReactNode;
  /** Accessible name for the submenu panel; defaults to a text label. */
  menuLabel?: string;
  icon?: ReactNode;
  /** Submenu panel width in px. */
  width?: number;
  children: ReactNode;
}

/**
 * A row that opens a second menu panel beside itself. It opens on hover, a
 * click, or ArrowRight, closes on leaving both panels, and lets the root
 * `Menu` own Escape/ArrowLeft/arrow-key focus routing.
 */
export function MenuSubmenu({ label, menuLabel, icon, width = 213, children }: MenuSubmenuProps) {
  const [open, setOpen] = useState(false);
  const [focusFirst, setFocusFirst] = useState(false);
  const [position, setPosition] = useState<CSSProperties>({ visibility: 'hidden' });
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const closeTimer = useRef<number | null>(null);

  const cancelClose = () => {
    if (closeTimer.current !== null) {
      window.clearTimeout(closeTimer.current);
      closeTimer.current = null;
    }
  };
  const close = () => {
    cancelClose();
    setOpen(false);
    setFocusFirst(false);
  };
  const openMenu = (focus = false) => {
    cancelClose();
    setOpen(true);
    if (focus) setFocusFirst(true);
  };
  // Leaving a row for the panel crosses a 3px gap: allow a moment before closing.
  const scheduleClose = () => {
    cancelClose();
    closeTimer.current = window.setTimeout(() => setOpen(false), 140);
  };
  useEffect(
    () => () => {
      if (closeTimer.current !== null) window.clearTimeout(closeTimer.current);
    },
    [],
  );

  // The root menu's Escape/ArrowLeft closes the flyout through this signal;
  // a click stays an open action so the hover's pointer-enter cannot race it.
  const closeRef = useRef(close);
  closeRef.current = close;
  useEffect(() => {
    const el = trigger.current;
    if (!el) return;
    const requestClose = () => closeRef.current();
    el.addEventListener(closeSubmenuEvent, requestClose);
    return () => el.removeEventListener(closeSubmenuEvent, requestClose);
  }, []);

  useLayoutEffect(() => {
    if (!open) return;
    const rect = trigger.current?.getBoundingClientRect();
    const el = panel.current;
    if (!rect || !el) return;
    const panelWidth = el.offsetWidth || width;
    const gap = 3;
    const fitsRight = window.innerWidth - rect.right >= panelWidth + 8;
    const left = fitsRight
      ? rect.right + gap
      : Math.max(8, rect.left - panelWidth - gap);
    const top = Math.max(8, Math.min(rect.top - 5, window.innerHeight - 160));
    setPosition({
      left,
      top,
      width: panelWidth,
      maxHeight: window.innerHeight - top - 8,
      visibility: 'visible',
    });
  }, [open, width]);

  useLayoutEffect(() => {
    if (open && focusFirst) {
      itemsIn(panel.current)[0]?.focus({ preventScroll: true });
      setFocusFirst(false);
    }
  }, [open, focusFirst]);

  return (
    <>
      <MenuItem
        buttonRef={trigger}
        icon={icon}
        trailing={<ChevronRight size={14} />}
        aria-haspopup="menu"
        aria-expanded={open}
        data-submenu-trigger=""
        data-submenu-open={open || undefined}
        onPointerEnter={() => openMenu()}
        onPointerLeave={scheduleClose}
        onBlur={() => {
          // WebKit reports a null relatedTarget when the focused element is
          // replaced by a portal, so wait a tick and inspect the real target.
          window.setTimeout(() => {
            const active = document.activeElement;
            if (active && (panel.current?.contains(active) || trigger.current?.contains(active))) return;
            close();
          }, 0);
        }}
        onKeyDown={(event) => {
          if (event.key === 'ArrowRight') {
            event.preventDefault();
            event.stopPropagation();
            openMenu(true);
          }
        }}
        onSelect={() => openMenu(true)}
      >
        {label}
      </MenuItem>
      {open &&
        createPortal(
          <div
            ref={panel}
            data-menu-panel=""
            data-menu-submenu=""
            className={`${panelClass} z-[calc(var(--z-contextual-menus)+1)] min-w-[213px]`}
            style={position}
            role="menu"
            aria-label={menuLabel ?? (typeof label === 'string' ? label : 'Submenu')}
            onPointerEnter={cancelClose}
            onPointerLeave={scheduleClose}
          >
            {children}
          </div>,
          document.body,
        )}
    </>
  );
}

export function MenuSeparator() {
  return <div className="my-[5px] border-t-[0.5px] border-t-vibrant-divider" role="separator" />;
}

/** A non-interactive heading inside a menu. */
export function MenuHeader({ children }: { children: ReactNode }) {
  return <div className="px-2 pt-1.5 pb-1 type-subhead-emphasized text-secondary">{children}</div>;
}
