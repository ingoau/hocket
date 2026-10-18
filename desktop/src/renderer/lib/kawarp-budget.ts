// How the fullscreen fluid background is driven for the performance budget
// (docs/design.md): Kawarp.start() is a rAF loop at the display rate; when the
// window is visible but unfocused we stop that and paint ~24 fps ourselves.
//
// Kawarp.renderFrame(time) treats `time` as accumulated animation seconds (the
// same value its own loop passes to the warp shader). Passing performance.now()
// (wall-clock ms) makes the warp jump every frame — the unfocused fullscreen
// flicker. Always call renderFrame with no argument so Kawarp advances from its
// own lastFrameTime.

export const BACKGROUND_FPS = 24;

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

/** Start Kawarp's frame loop for this performance mode. Returns a disposer. */
export function startKawarpBudget(k: KawarpDriver, perf: PerfMode): (() => void) | undefined {
  if (perf === "stopped") {
    k.stop();
    return;
  }
  if (perf === "full") {
    k.start();
    return () => k.stop();
  }
  k.stop();
  const id = setInterval(() => k.renderFrame(), 1000 / BACKGROUND_FPS);
  return () => clearInterval(id);
}
