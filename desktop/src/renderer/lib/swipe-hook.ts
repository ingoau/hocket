// Drives a SwipeTracker (swipe.ts) from an element's wheel events and AppKit's scroll phases, and
// plays its haptics.
import { useEffect, useRef, useState, type RefObject } from "react";
import type { ScrollPhase } from "@shared/bridge-types";
import { detentActuation, SwipeTracker, WheelSequence, type HapticSource, type Side, type SwipeEffect, type SwipeStage, type SwipeTuning } from "./swipe";

export interface SwipeView {
  offset: number;
  armed: Side | 0;
  stage: SwipeStage;
}

export interface SwipeTrace {
  wheel?(dx: number, dy: number, consumed: boolean, t: number): void;
  phase?(p: ScrollPhase, t: number): void;
  effect?(e: SwipeEffect, t: number): void;
  frame?(offset: number, width: number, armed: boolean, t: number): void;
}

/** One per window: every surface shares what the trackpad is doing. */
const sequence = new WheelSequence();

/**
 * Play a haptic. "actuator" detents ramp their actuation ID with `progress`; any other
 * "actuator" haptic is the kick.
 */
export function playHaptic(pattern: HapticSource, t: SwipeTuning, detentProgress?: number): void {
  try {
    if (pattern !== "actuator") window.hocket?.gestures?.haptic(pattern);
    else if (detentProgress !== undefined) window.hocket?.gestures?.actuate(detentActuation(t, detentProgress), t.actuatorArg1, t.actuatorArg2);
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

/** Observe phases without driving a surface (the playground's readout). */
export function useScrollPhases(listener: (p: ScrollPhase) => void): void {
  const ref = useRef(listener);
  ref.current = listener;
  useEffect(() => onScrollPhase((p) => ref.current(p)), []);
}

export interface UseSwipeOptions {
  tuning: SwipeTuning;
  /** Which sides have an action. */
  sides?: { start: boolean; end: boolean };
  onCommit?(side: Side, how: "threshold" | "release" | "fling"): void;
  trace?: SwipeTrace;
  disabled?: boolean;
}

/**
 * Two-finger horizontal swipes on `ref`'s element. Returns where the surface should be drawn:
 * `offset` px (positive towards the end of the line) while `stage` is "tracking", 0 after (animate
 * back with the tuning's spring).
 */
export function useSwipe(ref: RefObject<HTMLElement | null>, opts: UseSwipeOptions): SwipeView {
  const optsRef = useRef(opts);
  optsRef.current = opts;
  const trackerRef = useRef<SwipeTracker | null>(null);
  trackerRef.current ??= new SwipeTracker(opts.tuning, sequence);
  const tracker = trackerRef.current;
  tracker.tuning = opts.tuning;
  tracker.sides = { 1: opts.sides?.start ?? true, [-1]: opts.sides?.end ?? true };
  const [view, setView] = useState<SwipeView>({ offset: 0, armed: 0, stage: "idle" });

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let timer: ReturnType<typeof setTimeout> | undefined;

    const publish = (t: number) => {
      setView((v) => (v.offset === tracker.offset && v.armed === tracker.armed && v.stage === tracker.stage ? v : { offset: tracker.offset, armed: tracker.armed, stage: tracker.stage }));
      optsRef.current.trace?.frame?.(tracker.offset, tracker.width, tracker.armed !== 0, t);
    };
    const apply = (effects: SwipeEffect[], t: number) => {
      for (const e of effects) {
        optsRef.current.trace?.effect?.(e, t);
        if (e.kind === "haptic") playHaptic(e.pattern, tracker.tuning, e.reason === "detent" ? (e.progress ?? 0) : undefined);
        else if (e.kind === "commit") optsRef.current.onCommit?.(e.side, e.how);
      }
    };
    const scheduleTick = () => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        const t = performance.now();
        apply(tracker.tick(t), t);
        publish(t);
        if (tracker.stage !== "idle") scheduleTick();
      }, tracker.tuning.idleMs + 10);
    };

    const onWheel = (e: WheelEvent) => {
      if (optsRef.current.disabled || e.ctrlKey) return; // ctrl: a pinch
      tracker.width = el.clientWidth || 1;
      // Lines (a notched mouse wheel) are no swipe; pixels are a trackpad or a Magic Mouse.
      const scale = e.deltaMode === WheelEvent.DOM_DELTA_PIXEL ? 1 : 16;
      const { consumed, effects } = tracker.wheel(e.deltaX * scale, e.deltaY * scale, e.timeStamp);
      if (consumed) e.preventDefault();
      optsRef.current.trace?.wheel?.(e.deltaX, e.deltaY, consumed, e.timeStamp);
      apply(effects, e.timeStamp);
      publish(e.timeStamp);
      scheduleTick();
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    const off = onScrollPhase((p) => {
      const t = performance.now();
      const effects = tracker.phase(p, t);
      apply(effects, t);
      publish(t);
    });
    return () => {
      clearTimeout(timer);
      el.removeEventListener("wheel", onWheel);
      off();
    };
  }, [ref, tracker]);

  return view;
}
