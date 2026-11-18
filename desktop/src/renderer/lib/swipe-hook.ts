// Drives a SwipeTracker (swipe.ts) from an element's wheel events and AppKit's scroll phases, and
// plays its haptics.
//
// One hook can serve many rows: `begin` names the row under the pointer when a stream starts (and
// which of its sides have an action), and the view says which row is moving. Surfaces nest (a song
// row inside the page): the innermost one that takes a stream claims its events, and the ones
// around it leave them alone.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import type { ScrollPhase } from "@shared/bridge-types";
import { spring } from "./spring";
import { coarseWheel, SwipeTracker, WheelSequence, type HapticSource, type Side, type SwipeEffect, type SwipeStage, type SwipeTuning } from "./swipe";

export interface SwipeView<K = string> {
  offset: number;
  armed: Side | 0;
  stage: SwipeStage;
  /** The row (`begin`'s key) the swipe is on: kept after it ends, so that row settles back. */
  key?: K;
  /** The side it last moved towards, kept while it settles. */
  side: Side | 0;
  /** It ended and is springing back (or out): keep showing what it uncovered. */
  settling: boolean;
}

/** One per window: every surface shares what the trackpad is doing. */
const sequence = new WheelSequence();
/** Wheel events an inner surface has taken (it is swiping, or deciding whether to). */
const claimed = new WeakSet<Event>();

/** Play a haptic: an AppKit pattern, or the actuator (a private API; main falls back to AppKit's "generic"). */
export function playHaptic(pattern: HapticSource, t: SwipeTuning): void {
  try {
    if (pattern !== "actuator") window.hocket?.gestures?.haptic(pattern);
    else window.hocket?.gestures?.actuate(t.kickId, t.kickArg1, t.kickArg2);
  } catch {
    /* no bridge (tests) */
  }
}

/** A subscription to AppKit's scroll phases shared by every surface (one IPC listener). */
const phaseListeners = new Set<(p: ScrollPhase) => void>();
let unsubscribePhases: (() => void) | undefined;
function onScrollPhase(listener: (p: ScrollPhase) => void): () => void {
  phaseListeners.add(listener);
  if (!unsubscribePhases) {
    try {
      unsubscribePhases = window.hocket?.gestures?.onScrollPhase((p) => {
        sequence.phase(p);
        for (const l of phaseListeners) l(p);
      });
    } catch {
      /* no bridge */
    }
  }
  return () => {
    phaseListeners.delete(listener);
    if (phaseListeners.size === 0) {
      unsubscribePhases?.();
      unsubscribePhases = undefined;
    }
  };
}

/**
 * Whether `el`, or anything between it and `root`, scrolls sideways (a carousel, a wide table):
 * there a sideways stream is a scroll, not a swipe.
 */
export function inHorizontalScroller(el: Element | null, root: Element): boolean {
  for (let n = el; n && n !== root; n = n.parentElement) {
    if (n.scrollWidth > n.clientWidth + 1) {
      const o = getComputedStyle(n).overflowX;
      if (o === "auto" || o === "scroll") return true;
    }
  }
  return false;
}

export interface Sides {
  start: boolean;
  end: boolean;
}

export interface UseSwipeOptions<K> {
  /** Null: no swipes here (the gesture is off). */
  tuning: SwipeTuning | null;
  /** Which sides have an action, when it doesn't depend on where the stream starts. */
  sides?: Sides;
  /**
   * Where a stream starts (the event's target): the row it is on and which of its sides have an
   * action, or null for none here. Called only when no swipe is under way.
   */
  begin?(e: WheelEvent): { key: K; sides: Sides } | null;
  onCommit?(side: Side, how: "threshold" | "release" | "fling", key: K | undefined): void;
  disabled?: boolean;
}

const NO_SIDES = { 1: false, [-1]: false } as const;

/**
 * Two-finger horizontal swipes on `ref`'s element. Returns where the surface should be drawn:
 * `offset` px (positive towards the end of the line) while `stage` is "tracking", 0 after (animate
 * back with the tuning's spring, `useSettle`).
 */
export function useSwipe<K = string>(ref: RefObject<HTMLElement | null>, opts: UseSwipeOptions<K>): SwipeView<K> {
  const optsRef = useRef(opts);
  optsRef.current = opts;
  const trackerRef = useRef<SwipeTracker | null>(null);
  const fallback = opts.tuning ?? trackerRef.current?.tuning;
  trackerRef.current ??= fallback ? new SwipeTracker(fallback, sequence) : null;
  const tracker = trackerRef.current;
  if (tracker && opts.tuning) tracker.tuning = opts.tuning;
  const [view, setView] = useState<SwipeView<K>>({ offset: 0, armed: 0, stage: "idle", side: 0, settling: false });
  const on = !!opts.tuning;
  // The element can come and go (a list that is empty at first): follow it. Every render, but it
  // only sets state when the element changed.
  const [el, setEl] = useState<HTMLElement | null>(null);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useLayoutEffect(() => {
    if (ref.current !== el) setEl(ref.current);
  });

  useEffect(() => {
    if (!el || !tracker || !on) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    /** The row `begin` named for the stream starting, and the one moving once it is a swipe. */
    let pending: K | undefined;
    let active: K | undefined;
    let side: Side | 0 = 0;
    let settling = false;
    let settleTimer: ReturnType<typeof setTimeout> | undefined;
    let wasTracking = false;

    const publish = () => {
      const tracking = tracker.stage === "tracking";
      if (tracking && tracker.offset !== 0) side = tracker.offset > 0 ? 1 : -1;
      if (tracking) {
        settling = false;
        clearTimeout(settleTimer);
      } else if (wasTracking) {
        settling = true;
        clearTimeout(settleTimer);
        settleTimer = setTimeout(() => {
          settling = false;
          publish();
        }, spring(tracker.tuning.springStiffness, tracker.tuning.springDamping).duration + 40);
      }
      wasTracking = tracking;
      setView((v) =>
        v.offset === tracker.offset && v.armed === tracker.armed && v.stage === tracker.stage && v.key === active && v.side === side && v.settling === settling
          ? v
          : { offset: tracker.offset, armed: tracker.armed, stage: tracker.stage, key: active, side, settling },
      );
    };
    const apply = (effects: SwipeEffect[]) => {
      for (const e of effects) {
        if (e.kind === "haptic") playHaptic(e.pattern, tracker.tuning);
        else if (e.kind === "decided" && e.axis === "x") {
          active = pending;
          side = 0;
        }
        else if (e.kind === "commit") optsRef.current.onCommit?.(e.side, e.how, active);
      }
    };
    const scheduleTick = () => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        apply(tracker.tick(performance.now()));
        publish();
        if (tracker.stage !== "idle") scheduleTick();
      }, tracker.tuning.idleMs + 10);
    };

    const onWheel = (e: WheelEvent) => {
      if (e.ctrlKey) return; // a pinch
      if (claimed.has(e)) return; // an inner surface's
      const o = optsRef.current;
      if (o.disabled && tracker.stage !== "tracking") return;
      if (tracker.stage === "idle") {
        if (o.begin) {
          const b = inHorizontalScroller(e.target as Element | null, el) ? null : o.begin(e);
          pending = b?.key;
          tracker.sides = b ? { 1: b.sides.start, [-1]: b.sides.end } : { ...NO_SIDES };
        } else {
          const s = o.sides ?? { start: true, end: true };
          tracker.sides = inHorizontalScroller(e.target as Element | null, el) ? { ...NO_SIDES } : { 1: s.start, [-1]: s.end };
        }
      }
      tracker.width = el.clientWidth || 1;
      // Lines (a notched mouse wheel) are scaled to pixels; `coarseWheel` keeps them from starting a swipe.
      const scale = e.deltaMode === WheelEvent.DOM_DELTA_PIXEL ? 1 : 16;
      const { consumed, effects } = tracker.wheel(e.deltaX * scale, e.deltaY * scale, e.timeStamp, coarseWheel(e));
      if (consumed) e.preventDefault();
      if (tracker.stage === "deciding" || tracker.stage === "tracking" || tracker.stage === "coasting") claimed.add(e);
      apply(effects);
      publish();
      scheduleTick();
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    const off = onScrollPhase((p) => {
      apply(tracker.phase(p, performance.now()));
      publish();
    });
    return () => {
      clearTimeout(timer);
      clearTimeout(settleTimer);
      el.removeEventListener("wheel", onWheel);
      off();
    };
  }, [el, tracker, on]);

  return view;
}

/** The settle back (or out) after a swipe, as a CSS transition of `translate`; none with reduced motion. */
export function useSettle(t: SwipeTuning | null, reducedMotion: boolean): string {
  const stiffness = t?.springStiffness ?? 500;
  const damping = t?.springDamping ?? 0.82;
  return useMemo(() => {
    if (reducedMotion) return "none";
    const s = spring(stiffness, damping);
    return `translate ${s.duration}ms ${s.easing}`;
  }, [stiffness, damping, reducedMotion]);
}
