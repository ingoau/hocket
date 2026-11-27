// A press held for LONG_PRESS_MS runs `onLongPress`; when that returns true (it did
// something) the click the release makes is swallowed. A shorter press is an ordinary
// click, and so is a keyboard activation. Spread the handlers onto a button.
import { useEffect, useRef } from "react";
import type { MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent } from "react";

export const LONG_PRESS_MS = 500;
/** Moving further than this (px) while pressed cancels the long press. */
const SLOP = 10;

export function useLongPress(onLongPress: () => boolean) {
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const fired = useRef(false);
  const start = useRef<{ x: number; y: number } | undefined>(undefined);
  const latest = useRef(onLongPress);
  latest.current = onLongPress;
  const cancel = () => {
    if (timer.current !== undefined) clearTimeout(timer.current);
    timer.current = undefined;
    start.current = undefined;
  };
  useEffect(() => cancel, []);
  return {
    onPointerDown: (e: ReactPointerEvent<HTMLElement>) => {
      if (e.button !== 0) return;
      cancel();
      fired.current = false;
      start.current = { x: e.clientX, y: e.clientY };
      timer.current = setTimeout(() => {
        timer.current = undefined;
        fired.current = latest.current();
      }, LONG_PRESS_MS);
    },
    onPointerMove: (e: ReactPointerEvent<HTMLElement>) => {
      const s = start.current;
      if (s && Math.hypot(e.clientX - s.x, e.clientY - s.y) > SLOP) cancel();
    },
    onPointerUp: cancel,
    onPointerLeave: cancel,
    onPointerCancel: cancel,
    onClickCapture: (e: ReactMouseEvent<HTMLElement>) => {
      // A keyboard click (detail 0) is never the release of the hold, even when a release off
      // the button left the flag set.
      const swallow = fired.current && e.detail !== 0;
      fired.current = false;
      if (!swallow) return;
      e.preventDefault();
      e.stopPropagation();
    },
  };
}
