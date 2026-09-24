import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ALBUM_DWELL_MS, INTENT_DWELL_MS, IntentPrimer, SCROLL_QUIET_MS, VisitDwell } from "./prime";

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(1_000_000);
});
afterEach(() => {
  vi.useRealTimers();
});

describe("VisitDwell (album page dwell)", () => {
  it("fires once after 2 s of continuous visibility and focus, never before", () => {
    const fire = vi.fn();
    const d = new VisitDwell(fire);
    d.setActive(true);
    vi.advanceTimersByTime(ALBUM_DWELL_MS - 1);
    expect(fire).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(fire).toHaveBeenCalledTimes(1);
    // Staying, blurring and refocusing: still once per visit.
    vi.advanceTimersByTime(10_000);
    d.setActive(false);
    d.setActive(true);
    vi.advanceTimersByTime(10_000);
    expect(fire).toHaveBeenCalledTimes(1);
    expect(d.hasFired).toBe(true);
  });

  it("restarts the count when the window loses focus or the page is hidden", () => {
    const fire = vi.fn();
    const d = new VisitDwell(fire);
    d.setActive(true);
    vi.advanceTimersByTime(1500);
    d.setActive(false);
    vi.advanceTimersByTime(5000);
    expect(fire).not.toHaveBeenCalled();
    d.setActive(true);
    vi.advanceTimersByTime(1500);
    expect(fire).not.toHaveBeenCalled();
    vi.advanceTimersByTime(500);
    expect(fire).toHaveBeenCalledTimes(1);
  });

  it("never fires while inactive, and a new visit may fire again", () => {
    const fire = vi.fn();
    const d = new VisitDwell(fire);
    vi.advanceTimersByTime(10_000);
    expect(fire).not.toHaveBeenCalled();
    d.setActive(true);
    vi.advanceTimersByTime(ALBUM_DWELL_MS);
    expect(fire).toHaveBeenCalledTimes(1);
    d.reset();
    vi.advanceTimersByTime(ALBUM_DWELL_MS - 1);
    expect(fire).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(1);
    expect(fire).toHaveBeenCalledTimes(2);
  });

  it("does nothing after dispose (the page was left)", () => {
    const fire = vi.fn();
    const d = new VisitDwell(fire);
    d.setActive(true);
    vi.advanceTimersByTime(1000);
    d.dispose();
    vi.advanceTimersByTime(10_000);
    expect(fire).not.toHaveBeenCalled();
  });
});

describe("IntentPrimer (hover / focus on a play control)", () => {
  it("primes after resting ~300 ms, not on a pass-over", () => {
    const prime = vi.fn();
    const p = new IntentPrimer(prime);
    p.enter("a");
    vi.advanceTimersByTime(INTENT_DWELL_MS - 50);
    p.leave("a");
    vi.advanceTimersByTime(1000);
    expect(prime).not.toHaveBeenCalled();
    p.enter("a");
    vi.advanceTimersByTime(INTENT_DWELL_MS);
    expect(prime).toHaveBeenCalledWith("a");
  });

  it("primes each id at most once per visit, and again after reset", () => {
    const prime = vi.fn();
    const p = new IntentPrimer(prime);
    for (let i = 0; i < 3; i++) {
      p.enter("a");
      vi.advanceTimersByTime(INTENT_DWELL_MS + 10);
      p.leave("a");
    }
    p.enter("b");
    vi.advanceTimersByTime(INTENT_DWELL_MS);
    expect(prime.mock.calls).toEqual([["a"], ["b"]]);
    expect(p.hasPrimed("a")).toBe(true);
    p.reset();
    p.enter("a");
    vi.advanceTimersByTime(INTENT_DWELL_MS);
    expect(prime.mock.calls).toEqual([["a"], ["b"], ["a"]]);
  });

  it("debounces a burst of enters on the same control into one timer", () => {
    const prime = vi.fn();
    const p = new IntentPrimer(prime);
    p.enter("a");
    vi.advanceTimersByTime(100);
    p.enter("a");
    vi.advanceTimersByTime(INTENT_DWELL_MS - 100);
    expect(prime).toHaveBeenCalledTimes(1);
  });

  it("a scroll cancels pending intents and ignores arrivals right after it (scroll-by)", () => {
    const prime = vi.fn();
    const p = new IntentPrimer(prime);
    p.enter("a");
    vi.advanceTimersByTime(200);
    p.scrolled();
    vi.advanceTimersByTime(1000);
    expect(prime).not.toHaveBeenCalled();
    // Content keeps scrolling under the pointer: each tile it crosses "enters".
    for (const id of ["b", "c", "d"]) {
      p.scrolled();
      vi.advanceTimersByTime(40);
      p.enter(id);
      vi.advanceTimersByTime(40);
    }
    vi.advanceTimersByTime(2000);
    expect(prime).not.toHaveBeenCalled();
    // Once the scroll has settled, a deliberate rest primes.
    vi.advanceTimersByTime(SCROLL_QUIET_MS);
    p.enter("d");
    vi.advanceTimersByTime(INTENT_DWELL_MS);
    expect(prime.mock.calls).toEqual([["d"]]);
  });

  it("an arrival where the pointer was parked when the content scrolled never primes, however late", () => {
    const prime = vi.fn();
    const p = new IntentPrimer(prime);
    const parked = { x: 100, y: 200 };
    p.scrolled(parked);
    // The browser's hover update after the scroll settles: same spot, much later.
    vi.advanceTimersByTime(2000);
    p.enter("a", { ...parked });
    vi.advanceTimersByTime(2000);
    expect(prime).not.toHaveBeenCalled();
    // The user moves the pointer on the control: that's intent.
    p.enter("a", { x: 104, y: 201 });
    vi.advanceTimersByTime(INTENT_DWELL_MS);
    expect(prime).toHaveBeenCalledWith("a");
    // Keyboard focus (no position) isn't affected by where the pointer is.
    p.scrolled(parked);
    vi.advanceTimersByTime(SCROLL_QUIET_MS);
    p.enter("b");
    vi.advanceTimersByTime(INTENT_DWELL_MS);
    expect(prime.mock.calls).toEqual([["a"], ["b"]]);
  });

  it("nothing fires after dispose", () => {
    const prime = vi.fn();
    const p = new IntentPrimer(prime);
    p.enter("a");
    p.dispose();
    vi.advanceTimersByTime(5000);
    expect(prime).not.toHaveBeenCalled();
  });
});
