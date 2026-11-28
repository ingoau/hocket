// Scroll positions per visit, so Back (and Forward) returns to where a page was left, as a native
// app's history does, while going to a page afresh starts it at the top.
//
// A visit is its Route object: the store keeps the very object in its history and hands it back on
// Back/Forward, and every navigation makes a new one. So positions live in a WeakMap on it and go
// when the history lets the visit go.
import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { useApp, type Route } from "../store/app";

const positions = new WeakMap<Route, Map<string, number>>();

/** Remember where [key]'s scroller is in [route]'s visit. */
export function rememberScroll(route: Route, key: string, top: number): void {
  let m = positions.get(route);
  if (!m) positions.set(route, (m = new Map()));
  m.set(key, top);
}

/** Where [key]'s scroller was left in [route]'s visit (undefined when this is a new visit). */
export function recallScroll(route: Route, key: string): number | undefined {
  return positions.get(route)?.get(key);
}

/** How long a restore keeps putting the position back while late content grows the page. */
const RESTORE_WINDOW_MS = 1500;

/**
 * Keeps [ref]'s scroll position for this visit, and puts it back once [ready] (the content that set
 * the height has loaded) when the visit is returned to. Content that arrives after [ready] (a shelf,
 * the groups above a table) may still grow the page, so the position is put back again as it grows,
 * for a moment, unless the user scrolls first: then they win. Returns the position to start from
 * (0 on a new visit), for a virtualiser's initial offset.
 */
export function useScrollMemory(ref: RefObject<HTMLElement | null>, key: string, ready: boolean): number {
  // The visit this page was mounted for (a page is mounted afresh for each one: views/Router.tsx).
  const [route] = useState(() => useApp.getState().route);
  const [initial] = useState(() => recallScroll(route, key) ?? 0);
  // While the position is being put back, scrolls are the restore's own, not the user's: not saved.
  const restoring = useRef(initial > 0);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const save = () => { if (!restoring.current) rememberScroll(route, key, el.scrollTop); };
    const takeOver = () => { restoring.current = false; };
    const inputs = ["wheel", "pointerdown", "keydown", "touchstart"] as const;
    el.addEventListener("scroll", save, { passive: true });
    for (const ev of inputs) el.addEventListener(ev, takeOver, { passive: true });
    return () => {
      el.removeEventListener("scroll", save);
      for (const ev of inputs) el.removeEventListener(ev, takeOver);
    };
  }, [ref, route, key]);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!restoring.current || !ready || !el) return;
    const restore = () => { if (restoring.current) el.scrollTop = initial; };
    restore();
    const ro = new ResizeObserver(restore);
    for (const child of el.children) ro.observe(child);
    const timer = setTimeout(() => { restoring.current = false; }, RESTORE_WINDOW_MS);
    return () => {
      ro.disconnect();
      clearTimeout(timer);
      restoring.current = false;
    };
  }, [ref, ready, initial]);
  return initial;
}
