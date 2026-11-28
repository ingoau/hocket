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

/**
 * Keeps [ref]'s scroll position for this visit, and puts it back once [ready] (the content that set
 * the height has loaded) when the visit is returned to. Returns the position to start from (0 on a
 * new visit), for a virtualiser's initial offset.
 */
export function useScrollMemory(ref: RefObject<HTMLElement | null>, key: string, ready: boolean): number {
  // The visit this page was mounted for (a page is mounted afresh for each one: views/Router.tsx).
  const [route] = useState(() => useApp.getState().route);
  const [initial] = useState(() => recallScroll(route, key) ?? 0);
  const restored = useRef(initial === 0);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const save = () => { if (restored.current) rememberScroll(route, key, el.scrollTop); };
    el.addEventListener("scroll", save, { passive: true });
    return () => el.removeEventListener("scroll", save);
  }, [ref, route, key]);
  useLayoutEffect(() => {
    const el = ref.current;
    if (restored.current || !ready || !el) return;
    restored.current = true;
    el.scrollTop = initial;
  }, [ref, ready, initial]);
  return initial;
}
