// Where each page was scrolled, by history entry: Back and Forward bring a page back where it was
// left (as a browser or a native player does), while going somewhere new starts at the top (each
// page mounts afresh, see Router). History entries are the route objects themselves (back() and
// forward() move the same objects between the stacks), so they key a WeakMap and are forgotten
// with the history.
import { useLayoutEffect } from "react";
import type { Route } from "../store/app";

/** A scroller's offsets, by its key within the content pane. */
type Positions = Map<string, number>;

const saved = new WeakMap<Route, Positions>();

/** How long a restore keeps trying while the page's rows load in under it. */
const RESTORE_MS = 1500;

/** A scroller's key: its test id, else its class list and which of the pane's elements with it it is. */
export function scrollKey(el: Element, root: ParentNode): string {
  const id = el.getAttribute("data-testid");
  if (id) return `#${id}`;
  const cls = typeof el.className === "string" ? el.className.trim() : "";
  if (!cls) return el.tagName;
  const same = Array.from(root.querySelectorAll(`.${CSS.escape(cls.split(/\s+/)[0]!)}`)).filter((e) => (e as Element).className === el.className);
  return `.${cls}@${same.indexOf(el)}`;
}

function find(key: string, root: HTMLElement): HTMLElement | null {
  if (key.startsWith("#")) return root.querySelector<HTMLElement>(`[data-testid="${CSS.escape(key.slice(1))}"]`);
  const at = key.lastIndexOf("@");
  if (!key.startsWith(".") || at < 0) return null;
  const cls = key.slice(1, at);
  const same = Array.from(root.querySelectorAll<HTMLElement>(`.${CSS.escape(cls.split(/\s+/)[0]!)}`)).filter((e) => e.className.trim() === cls);
  return same[Number(key.slice(at + 1))] ?? null;
}

/**
 * Remembers how far the scrollers in `#main` are scrolled while `route` is shown, and on showing a
 * route again puts them back, retrying while its content loads, until the user scrolls or presses
 * a key themselves.
 */
export function useScrollMemory(route: Route): void {
  useLayoutEffect(() => {
    const root = document.getElementById("main");
    if (!root) return;
    let positions = saved.get(route);
    const pending = new Map([...(positions ?? [])].filter(([, top]) => top > 0));
    if (!positions) {
      positions = new Map();
      saved.set(route, positions);
    }
    const keys = new WeakMap<Element, string>();
    const onScroll = (e: Event) => {
      const el = e.target;
      if (!(el instanceof HTMLElement) || !root.contains(el)) return;
      let key = keys.get(el);
      if (!key) {
        key = scrollKey(el, root);
        keys.set(el, key);
      }
      // A restore still under way owns its scroller (the browser clamps it while rows load).
      if (!pending.has(key)) positions.set(key, el.scrollTop);
    };
    root.addEventListener("scroll", onScroll, { capture: true, passive: true });
    const started = performance.now();
    let frame = 0;
    const stop = () => {
      pending.clear();
      cancelAnimationFrame(frame);
    };
    const restore = () => {
      for (const [key, top] of pending) {
        const el = find(key, root);
        if (!el) continue;
        el.scrollTop = top;
        if (Math.abs(el.scrollTop - top) < 1) pending.delete(key);
      }
      if (pending.size && performance.now() - started < RESTORE_MS) frame = requestAnimationFrame(restore);
      else stop();
    };
    // The user taking over ends it: their scroll, not the remembered one.
    const intents = ["wheel", "pointerdown", "keydown", "touchstart"] as const;
    if (pending.size) {
      restore();
      for (const t of intents) root.addEventListener(t, stop, { capture: true, passive: true });
    }
    return () => {
      stop();
      root.removeEventListener("scroll", onScroll, { capture: true });
      for (const t of intents) root.removeEventListener(t, stop, { capture: true });
    };
  }, [route]);
}
