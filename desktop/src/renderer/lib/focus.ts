// Focus management for overlays: remember what had focus when a modal opened
// and put focus back there when it closes; keep Tab inside the modal.
import { useLayoutEffect, useRef } from "react";

const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"]), [contenteditable="true"]';

/** Tabbable elements inside `root`, in DOM order, skipping hidden and inert ones. */
export function tabbables(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter((el) => el.tabIndex >= 0 && !el.closest("[inert]") && el.getClientRects().length > 0);
}

/** onKeyDown for a modal container: Tab and Shift+Tab wrap around inside it. */
export function trapTab(e: React.KeyboardEvent<HTMLElement>): void {
  if (e.key !== "Tab" || e.ctrlKey || e.altKey || e.metaKey) return;
  const list = tabbables(e.currentTarget);
  if (!list.length) {
    e.preventDefault();
    return;
  }
  const first = list[0] as HTMLElement;
  const last = list[list.length - 1] as HTMLElement;
  const active = document.activeElement as HTMLElement | null;
  const inside = !!active && e.currentTarget.contains(active);
  if (e.shiftKey && (!inside || active === first || active === e.currentTarget)) {
    e.preventDefault();
    last.focus();
  } else if (!e.shiftKey && (!inside || active === last)) {
    e.preventDefault();
    first.focus();
  }
}

/** Focus that a closing context menu handed on to a modal it opened. */
let handedOn: HTMLElement | null = null;
export function handOnFocus(el: HTMLElement | null): void {
  handedOn = el;
}

function currentFocus(): HTMLElement | null {
  const a = document.activeElement;
  const el = a instanceof HTMLElement && a !== document.body ? a : null;
  if (!el || el.closest("[role=menu]")) {
    const h = handedOn;
    handedOn = null;
    return h ?? el;
  }
  return el;
}

/**
 * While `open`, remembers the element focused when it opened and restores it
 * on close (falling back to the main content when that element is gone).
 */
export function useReturnFocus(open: boolean): void {
  const prev = useRef<HTMLElement | null>(null);
  useLayoutEffect(() => {
    if (!open) return;
    prev.current = currentFocus();
    return () => {
      const el = prev.current;
      prev.current = null;
      // After the commit that removes the overlay (and the shell's `inert`).
      requestAnimationFrame(() => {
        const active = document.activeElement;
        if (active && active !== document.body && active.isConnected && !active.closest("[inert]")) return;
        if (el && el.isConnected && !el.closest("[inert]")) el.focus({ preventScroll: true });
        else document.getElementById("main")?.focus({ preventScroll: true });
      });
    };
  }, [open]);
}
