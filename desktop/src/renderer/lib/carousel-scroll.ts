// A carousel whose track scrolls sideways under a trackpad (or Shift + wheel) and comes to rest on a
// slide. Not CSS scroll snapping: without the trackpad's phases (Linux) Chromium snaps after every
// wheel event, so a small one springs straight back and the track never moves. Here it scrolls
// freely and, once it stops, settles on a slide: on to the next one past a small nudge.
import { useEffect, useRef, type RefObject } from "react";

/** How far (in slides) a scroll has to go before it moves on rather than springing back. */
export const NUDGE = 0.15;
/** No scrolling for this long: the stream (momentum included) has ended. */
const IDLE_MS = 140;

/**
 * The slide a scroll that started on `from` and is now at `pos` (in slides, fractional) settles
 * on: past a nudge it moves on in the direction it went, to the nearest slide that way.
 */
export function settleIndex(from: number, pos: number, count: number): number {
  const delta = pos - from;
  const target = Math.abs(delta) < NUDGE ? from : delta > 0 ? Math.ceil(pos - NUDGE) : Math.floor(pos + NUDGE);
  return Math.max(0, Math.min(count - 1, target));
}

/**
 * Keeps `track` (its slides each its full width) on slide `index`, and reports through
 * `onSettle` the slide a sideways scroll comes to rest on.
 */
export function useCarouselScroll(track: RefObject<HTMLElement | null>, index: number, count: number, onSettle: (i: number) => void, reducedMotion: boolean): void {
  const onSettleRef = useRef(onSettle);
  onSettleRef.current = onSettle;
  const indexRef = useRef(index);
  indexRef.current = index;
  /** The slide a scroll of ours is heading to; scroll events until it gets there are ours. */
  const heading = useRef<number | undefined>(undefined);
  const scrollTo = useRef<(i: number) => void>(() => {});

  useEffect(() => {
    const el = track.current;
    if (!el || count < 1) return;
    let from: number | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    // Right to left, scrollLeft runs from 0 down.
    const sign = () => (getComputedStyle(el).direction === "rtl" ? -1 : 1);
    const pos = () => Math.abs(el.scrollLeft) / (el.clientWidth || 1);
    scrollTo.current = (i) => {
      const left = sign() * i * el.clientWidth;
      if (Math.abs(el.scrollLeft - left) < 1) {
        heading.current = undefined;
        return;
      }
      heading.current = i;
      el.scrollTo({ left, behavior: reducedMotion ? "instant" : "smooth" });
    };
    const settle = () => {
      const start = from ?? indexRef.current;
      from = undefined;
      const i = settleIndex(start, pos(), count);
      if (i !== indexRef.current) onSettleRef.current(i);
      scrollTo.current(i);
    };
    const onScroll = () => {
      if (heading.current !== undefined) return;
      from ??= indexRef.current;
      clearTimeout(timer);
      timer = setTimeout(settle, IDLE_MS);
    };
    // Fingers on the pad while it is heading somewhere: they take over from there.
    const onWheel = (e: WheelEvent) => {
      if (heading.current !== undefined && Math.abs(e.deltaX) > Math.abs(e.deltaY)) heading.current = undefined;
    };
    const onEnd = () => {
      if (heading.current !== undefined && Math.round(pos()) === heading.current) heading.current = undefined;
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    el.addEventListener("wheel", onWheel, { passive: true });
    el.addEventListener("scrollend", onEnd);
    // A new width (the window resized) keeps the slide in view.
    const ro = new ResizeObserver(() => {
      if (from === undefined) {
        heading.current = undefined;
        el.scrollTo({ left: sign() * indexRef.current * el.clientWidth, behavior: "instant" });
      }
    });
    ro.observe(el);
    return () => {
      clearTimeout(timer);
      el.removeEventListener("scroll", onScroll);
      el.removeEventListener("wheel", onWheel);
      el.removeEventListener("scrollend", onEnd);
      ro.disconnect();
      heading.current = undefined;
      scrollTo.current = () => {};
    };
  }, [track, count, reducedMotion]);

  // The arrows, dots and keys move the index: scroll to it.
  useEffect(() => {
    scrollTo.current(index);
  }, [index, count, reducedMotion]);
}
