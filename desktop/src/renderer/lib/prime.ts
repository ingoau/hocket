// The album primer's intent signals (core Command::PrimeAlbum / PrimeTrack):
// the core fetches a track's first seconds into the stream cache so pressing
// play starts from disk. The core decides whether to act (network, battery,
// rate limits); the UI only says when the user looks likely to press play:
//
// - VisitDwell: the album page has been on screen, in a focused window, for
//   2 s in a row. Fires once per visit (a new visit is a new instance or a
//   `reset()`).
// - IntentPrimer: the pointer rests on (or keyboard focus sits on) a play
//   control for ~300 ms. Fires once per id per visit. A scroll cancels every
//   pending intent and ignores new ones for a moment, so content moving under
//   a stationary pointer (scroll-by in a grid or list) never primes.
//
// Pure timers, no React: see use-prime.ts for the hooks and prime.test.ts.

export const ALBUM_DWELL_MS = 2000;
export const INTENT_DWELL_MS = 300;
/** After a scroll, pointer "arrivals" are content moving under the pointer. */
export const SCROLL_QUIET_MS = 250;

export interface TimerApi {
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
  now(): number;
}

const defaultTimers: TimerApi = {
  setTimeout: (fn, ms) => globalThis.setTimeout(fn, ms),
  clearTimeout: (h) => globalThis.clearTimeout(h as ReturnType<typeof globalThis.setTimeout>),
  now: () => Date.now(),
};

/** Fires `onFire` once after the page has been active for `delayMs` without interruption. */
export class VisitDwell {
  private handle: unknown;
  private fired = false;
  private active = false;
  private readonly timers: TimerApi;

  constructor(private readonly onFire: () => void, private readonly delayMs = ALBUM_DWELL_MS, timers: TimerApi = defaultTimers) {
    this.timers = timers;
  }

  /** Visible and focused (true) or not (false). Losing it restarts the count. */
  setActive(active: boolean): void {
    if (active === this.active) return;
    this.active = active;
    if (!active) {
      this.cancel();
      return;
    }
    if (this.fired || this.handle !== undefined) return;
    this.handle = this.timers.setTimeout(() => {
      this.handle = undefined;
      if (!this.active || this.fired) return;
      this.fired = true;
      this.onFire();
    }, this.delayMs);
  }

  /** A new visit: may fire again. Keeps the current activity. */
  reset(): void {
    this.cancel();
    this.fired = false;
    const was = this.active;
    this.active = false;
    this.setActive(was);
  }

  get hasFired(): boolean {
    return this.fired;
  }

  dispose(): void {
    this.cancel();
    this.active = false;
  }

  private cancel(): void {
    if (this.handle !== undefined) this.timers.clearTimeout(this.handle);
    this.handle = undefined;
  }
}

/** Debounced "resting on a play control" intent, once per id per visit. */
export class IntentPrimer {
  private pending = new Map<string, unknown>();
  private primed = new Set<string>();
  private lastScroll = Number.NEGATIVE_INFINITY;
  private readonly timers: TimerApi;

  constructor(private readonly onPrime: (id: string) => void, private readonly delayMs = INTENT_DWELL_MS, timers: TimerApi = defaultTimers, private readonly quietMs = SCROLL_QUIET_MS) {
    this.timers = timers;
  }

  /** Pointer entered (or focus arrived on) the control for `id`. */
  enter(id: string): void {
    if (!id || this.primed.has(id) || this.pending.has(id)) return;
    if (this.timers.now() - this.lastScroll < this.quietMs) return;
    const h = this.timers.setTimeout(() => {
      this.pending.delete(id);
      if (this.primed.has(id)) return;
      this.primed.add(id);
      this.onPrime(id);
    }, this.delayMs);
    this.pending.set(id, h);
  }

  /** Pointer left (or focus left) before the dwell elapsed: no intent. */
  leave(id: string): void {
    const h = this.pending.get(id);
    if (h === undefined) return;
    this.timers.clearTimeout(h);
    this.pending.delete(id);
  }

  /** Something scrolled: whatever is under the pointer got there by moving. */
  scrolled(): void {
    this.lastScroll = this.timers.now();
    for (const h of this.pending.values()) this.timers.clearTimeout(h);
    this.pending.clear();
  }

  /** A new visit: every id may prime again. */
  reset(): void {
    for (const h of this.pending.values()) this.timers.clearTimeout(h);
    this.pending.clear();
    this.primed.clear();
    this.lastScroll = Number.NEGATIVE_INFINITY;
  }

  hasPrimed(id: string): boolean {
    return this.primed.has(id);
  }

  dispose(): void {
    for (const h of this.pending.values()) this.timers.clearTimeout(h);
    this.pending.clear();
  }
}
