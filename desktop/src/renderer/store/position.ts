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
 * mounted. `durationMs` clamps. Uses Date.now() at each frame so it is
 * comparable with `takenAt`.
 */
export function usePosition(stamp: PositionStamp, durationMs: number | undefined, clockOffsetMs = 0): number {
  useFrameClock();
  return extrapolate(stamp, Date.now(), clockOffsetMs, durationMs);
}

/** Same, but only re-renders when the whole-second value changes (labels). */
export function useSecondsPosition(stamp: PositionStamp, durationMs: number | undefined, clockOffsetMs = 0): number {
  const frame = useFrameClock();
  const [sec, setSec] = useState(() => Math.floor(extrapolate(stamp, Date.now(), clockOffsetMs, durationMs) / 1000));
  const ref = useRef(sec);
  useEffect(() => {
    const s = Math.floor(extrapolate(stamp, Date.now(), clockOffsetMs, durationMs) / 1000);
    if (s !== ref.current) {
      ref.current = s;
      setSec(s);
    }
  }, [frame, stamp, durationMs, clockOffsetMs]);
  return sec;
}
