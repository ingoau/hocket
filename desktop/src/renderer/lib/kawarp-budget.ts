// How the fullscreen fluid background is driven for the performance budget
// (docs/design.md): Kawarp.start() is a rAF loop at the display rate; when the
// window is visible but unfocused we stop that and paint ~24 fps ourselves.
//
// Kawarp.renderFrame(time) treats `time` as accumulated animation seconds (the
// same value its own loop passes to the warp shader). Passing performance.now()
// (wall-clock ms) makes the warp jump every frame — the unfocused fullscreen
// flicker. Always call renderFrame with no argument so Kawarp advances from its
// own lastFrameTime.

import { frameDue, ticker } from "../store/position";

export const BACKGROUND_FPS = 24;
/** Focused fullscreen never runs at the display rate — Kawarp plus lyrics at 120 Hz is the battery cliff. */
export const FOCUSED_FPS = 30;

export type KawarpDriver = {
  start: () => void;
  stop: () => void;
  renderFrame: (time?: number) => void;
};

export type PerfMode = "full" | "background" | "stopped";

/** Stop only when Electron has hidden or minimised the window. Chromium's
 *  document.hidden also fires for an unfocused fullscreen window (occlusion),
 *  which would stop and restart the background — the flicker in #45. */
export function perfForWindow(visible: boolean, focused: boolean): PerfMode {
  if (!visible) return "stopped";
  return focused ? "full" : "background";
}

/** A shared frame source: the position ticker (store/position.ts). */
export interface FrameClock {
  subscribe(cb: () => void): () => void;
  /** The current frame's time (performance.now() ms). */
  readonly now: number;
}

/**
 * Start Kawarp's frame loop for this performance mode. Returns a disposer.
 * Frames come from `clock` (the position ticker), at most `fps` a second: a timer of its own
 * painted out of step with the lyrics and the seek bar, so the window produced the frames of
 * both (two 30 fps loops a frame apart are 60 frames a second).
 */
export function startKawarpBudget(k: KawarpDriver, perf: PerfMode, clock: FrameClock = ticker): (() => void) | undefined {
  if (perf === "stopped") {
    k.stop();
    return;
  }
  // Drive every frame ourselves: Kawarp.start() is unbounded display-rate rAF.
  k.stop();
  const fps = perf === "full" ? FOCUSED_FPS : BACKGROUND_FPS;
  let last = -Infinity;
  const unsubscribe = clock.subscribe(() => {
    if (!frameDue(clock.now - last, fps)) return;
    last = clock.now;
    k.renderFrame();
  });
  return () => {
    unsubscribe();
    k.stop();
  };
}
