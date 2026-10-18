// Position extrapolation from the last transport sample (design.md "Connect →
// Split authority": position is never broadcast on a timer). The rAF ticker
// only runs while a component that shows position is mounted, and it is
// capped to the frame budget the performance mode allows.
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import type { PositionStamp } from "@core/api";

export function extrapolate(p: PositionStamp, nowMs: number, clockOffsetMs = 0, durationMs?: number): number {
  const base = p.positionMs;
  if (!p.isPlaying) return clamp(base, durationMs);
  const elapsed = nowMs + clockOffsetMs - p.takenAt;
  // Don't extrapolate across an absurd gap (offline hour): snap back to the start.
  if (elapsed > 3_600_000) return 0;
  return clamp(base + Math.max(0, elapsed) * p.rate, durationMs);
}

function clamp(v: number, max?: number): number {
  const lo = Math.max(0, v);
  return max !== undefined && max > 0 ? Math.min(lo, max) : lo;
}

/** Shared ticker: one rAF loop for every subscriber, at most `fps` updates a second. */
class Ticker {
  private subs = new Set<() => void>();
  private raf: number | undefined;
  private last = 0;
  fps = 60;
  paused = false;
  now = typeof performance !== "undefined" ? performance.now() : 0;

  subscribe = (cb: () => void): (() => void) => {
    this.subs.add(cb);
    this.ensure();
    return () => {
      this.subs.delete(cb);
      if (!this.subs.size) this.stop();
    };
  };

  private ensure(): void {
    if (this.raf !== undefined || this.paused || typeof requestAnimationFrame === "undefined") return;
    const loop = (t: number) => {
      this.raf = requestAnimationFrame(loop);
      const interval = 1000 / this.fps;
      if (t - this.last < interval) return;
      this.last = t;
      this.now = t;
      for (const s of this.subs) s();
    };
    this.raf = requestAnimationFrame(loop);
  }

  private stop(): void {
    if (this.raf !== undefined && typeof cancelAnimationFrame !== "undefined") cancelAnimationFrame(this.raf);
    this.raf = undefined;
  }

  setPaused(paused: boolean): void {
    this.paused = paused;
    if (paused) this.stop();
    else if (this.subs.size) this.ensure();
  }

  getSnapshot = (): number => this.now;
}

export const ticker = new Ticker();

/** Re-render every frame (throttled) while mounted. Returns performance.now(). */
export function useFrameClock(): number {
  return useSyncExternalStore(ticker.subscribe, ticker.getSnapshot, () => 0);
}

/**
 * Extrapolated position in ms for the given stamp, updating each frame while
 * playing. Paused stamps do not subscribe to the ticker.
 */
export function usePosition(stamp: PositionStamp, durationMs: number | undefined, clockOffsetMs = 0): number {
  const [pos, setPos] = useState(() => extrapolate(stamp, Date.now(), clockOffsetMs, durationMs));
  useEffect(() => {
    const update = () => setPos(extrapolate(stamp, Date.now(), clockOffsetMs, durationMs));
    update();
    if (!stamp.isPlaying) return;
    return ticker.subscribe(update);
  }, [stamp, durationMs, clockOffsetMs]);
  return pos;
}

/** Same, but only re-renders when the whole-second value changes (labels). */
export function useSecondsPosition(stamp: PositionStamp, durationMs: number | undefined, clockOffsetMs = 0): number {
  const [sec, setSec] = useState(() => Math.floor(extrapolate(stamp, Date.now(), clockOffsetMs, durationMs) / 1000));
  useEffect(() => {
    const update = () => {
      const s = Math.floor(extrapolate(stamp, Date.now(), clockOffsetMs, durationMs) / 1000);
      setSec((prev) => (prev === s ? prev : s));
    };
    update();
    if (!stamp.isPlaying) return;
    return ticker.subscribe(update);
  }, [stamp, durationMs, clockOffsetMs]);
  return sec;
}

/**
 * Drive a paint callback from the shared ticker without a React re-render per
 * frame. Subscribes only while the stamp is playing and `active` holds.
 */
export function usePositionPaint(
  stamp: PositionStamp,
  durationMs: number | undefined,
  clockOffsetMs: number,
  paint: (positionMs: number) => void,
  active = true,
): void {
  const paintRef = useRef(paint);
  paintRef.current = paint;
  useEffect(() => {
    const run = () => paintRef.current(extrapolate(stamp, Date.now(), clockOffsetMs, durationMs));
    run();
    if (!stamp.isPlaying || !active) return;
    return ticker.subscribe(run);
  }, [stamp, durationMs, clockOffsetMs, active]);
}

/**
 * `derive(position)`, re-rendering only when its value changes: the ticker is subscribed only
 * while the stamp plays and `active` holds, and each frame compares the derived value instead of
 * re-rendering (a lyric line index, not every millisecond). `derive` should be stable (useCallback).
 */
export function usePositionValue<T>(stamp: PositionStamp, durationMs: number | undefined, clockOffsetMs: number, derive: (positionMs: number) => T, active = true): T {
  const [value, setValue] = useState(() => derive(extrapolate(stamp, Date.now(), clockOffsetMs, durationMs)));
  useEffect(() => {
    const update = () => {
      const v = derive(extrapolate(stamp, Date.now(), clockOffsetMs, durationMs));
      setValue((prev) => (Object.is(prev, v) ? prev : v));
    };
    update();
    if (!stamp.isPlaying || !active) return;
    return ticker.subscribe(update);
  }, [stamp, durationMs, clockOffsetMs, derive, active]);
  return value;
}
