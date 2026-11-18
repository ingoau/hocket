import { describe, expect, it } from "vitest";
import type { ScrollPhase } from "@shared/bridge-types";
import { coarseWheel, DEFAULT_TUNING as TUNED, STALE_MOMENTUM_MS, defaultRelease, detentIndex, resolveTuning, shapeTravel, SURFACE_TUNING, SwipeTracker, systemDirection, WheelSequence, type SwipeEffect, type SwipeTuning, type TuningPrefs } from "./swipe";

const W = 400;
/** Fixed numbers for these tests, whatever the tuned defaults become: a 120 px threshold. */
const DEFAULT_TUNING: SwipeTuning = { ...TUNED, threshold: 0.3, flingVelocity: 0, detentPx: 0, hapticCommit: "none" };

function setup(patch: Partial<SwipeTuning> = {}, seq = new WheelSequence()) {
  const tracker = new SwipeTracker({ ...DEFAULT_TUNING, ...patch }, seq);
  tracker.width = W;
  return { tracker, seq };
}

const phase = (p: Partial<ScrollPhase>): ScrollPhase => ({ phase: "none", momentum: "none", precise: true, inverted: true, t: 0, ...p });
const kinds = (effects: SwipeEffect[]) => effects.map((e) => (e.kind === "haptic" ? `haptic:${e.reason}` : e.kind));

/** Fingers moving `total` px sideways (natural scrolling: deltas are the opposite sign) in `steps` events. */
function swipe(tracker: SwipeTracker, total: number, steps: number, t0: number, dt = 8) {
  const effects: SwipeEffect[] = [];
  let consumed = true;
  for (let i = 0; i < steps; i++) {
    const r = tracker.wheel(-total / steps, 0, t0 + i * dt);
    consumed &&= r.consumed;
    effects.push(...r.effects);
  }
  return { effects, consumed, end: t0 + steps * dt };
}

describe("shapeTravel", () => {
  it("follows the fingers up to the threshold, then resists, and stretches past the end", () => {
    const t = { ...DEFAULT_TUNING, threshold: 0.25, resistance: 0.5, maxTravel: 0.5 };
    expect(shapeTravel(80, W, t)).toBe(80);
    expect(shapeTravel(-100, W, t)).toBe(-100);
    expect(shapeTravel(140, W, t)).toBe(120);
    const far = shapeTravel(10_000, W, t);
    expect(far).toBeGreaterThan(200);
    expect(far).toBeLessThan(248);
  });
});

describe("SwipeTracker", () => {
  it("runs the action when the fingers lift past the threshold (AppKit phases)", () => {
    const { tracker } = setup();
    tracker.phase(phase({ phase: "began" }), 0);
    const { effects, consumed } = swipe(tracker, 160, 10, 1);
    expect(consumed).toBe(true);
    expect(tracker.armed).toBe(1);
    expect(kinds(effects)).toEqual(["decided", "haptic:arm"]);
    const out = tracker.phase(phase({ phase: "ended" }), 100);
    expect(out).toContainEqual({ kind: "commit", side: 1, how: "release" });
    expect(tracker.stage).toBe("coasting");
    expect(tracker.offset).toBe(0);
  });

  it("springs back when let go short of the threshold", () => {
    const { tracker } = setup();
    tracker.phase(phase({ phase: "began" }), 0);
    swipe(tracker, 60, 6, 1);
    const out = tracker.phase(phase({ phase: "ended" }), 60);
    expect(kinds(out)).toEqual(["release"]);
    expect(out[0]).toMatchObject({ committed: false });
  });

  it("leaves a vertical scroll alone", () => {
    const { tracker } = setup();
    const r1 = tracker.wheel(0, 4, 1);
    const r2 = tracker.wheel(1, 8, 9);
    expect(r1.consumed).toBe(false);
    expect(r2.consumed).toBe(false);
    expect(tracker.stage).toBe("passthrough");
    expect(tracker.wheel(-30, 0, 17).consumed).toBe(false);
  });

  it("swallows the momentum after a commit, and momentum never starts a swipe elsewhere", () => {
    const seq = new WheelSequence();
    const { tracker } = setup({}, seq);
    const other = setup({}, seq).tracker;
    tracker.phase(phase({ phase: "began" }), 0);
    const { end } = swipe(tracker, 200, 10, 1);
    tracker.phase(phase({ phase: "ended" }), end);
    tracker.phase(phase({ momentum: "began" }), end + 2);
    expect(tracker.wheel(-12, 0, end + 8).consumed).toBe(true);
    // The row went away; the coast lands on the next one.
    const r = other.wheel(-10, 0, end + 16);
    expect(r.consumed).toBe(false);
    expect(other.stage).toBe("passthrough");
    tracker.phase(phase({ momentum: "ended" }), end + 300);
    expect(tracker.stage).toBe("idle");
  });

  it("commits at the threshold in threshold mode, once", () => {
    const { tracker } = setup({ commit: "threshold", hapticCommit: "levelChange" });
    tracker.phase(phase({ phase: "began" }), 0);
    const { effects } = swipe(tracker, 300, 15, 1);
    expect(kinds(effects)).toEqual(["decided", "haptic:arm", "commit", "haptic:commit", "release"]);
    expect(tracker.phase(phase({ phase: "ended" }), 200)).toEqual([]);
  });

  it("disarms with a tick when pulled back past the hysteresis", () => {
    const { tracker } = setup({ hysteresis: 0.05, resistance: 0 });
    tracker.phase(phase({ phase: "began" }), 0);
    const { end } = swipe(tracker, 130, 10, 1);
    expect(tracker.armed).toBe(1);
    // 130 → 115: within the 20 px hysteresis below 120, still armed.
    tracker.wheel(15, 0, end);
    expect(tracker.armed).toBe(1);
    const r = tracker.wheel(20, 0, end + 8);
    expect(tracker.armed).toBe(0);
    expect(kinds(r.effects)).toEqual(["haptic:disarm"]);
  });

  it("ends on an idle gap without AppKit phases", () => {
    const { tracker } = setup({ release: "idle", idleMs: 80 });
    const { end } = swipe(tracker, 160, 10, 1);
    expect(tracker.tick(end + 40)).toEqual([]);
    expect(kinds(tracker.tick(end + 100))).toEqual(["commit", "release"]);
  });

  it("does not take a resting finger for a lift when the phases say it is down", () => {
    const { tracker } = setup({ idleMs: 80 });
    tracker.phase(phase({ phase: "began" }), 0);
    const { end } = swipe(tracker, 60, 6, 1);
    expect(tracker.tick(end + 300)).toEqual([]);
    expect(tracker.wheel(-10, 0, end + 400).consumed).toBe(true);
    expect(tracker.stage).toBe("tracking");
  });

  it("reads shrinking deltas as a lift in momentum mode", () => {
    const { tracker } = setup({ release: "momentum", decayEvents: 3 });
    swipe(tracker, 140, 7, 1);
    const out: SwipeEffect[] = [];
    let t = 100;
    for (const d of [18, 14, 10, 7]) out.push(...tracker.wheel(-d, 0, (t += 8)).effects);
    expect(kinds(out)).toContain("commit");
  });

  it("flings a short, fast swipe through", () => {
    const { tracker } = setup({ flingVelocity: 1 });
    tracker.phase(phase({ phase: "began" }), 0);
    // 80 px in 32 ms: 2.5 px/ms, short of the 120 px threshold.
    const { end } = swipe(tracker, 80, 4, 1, 8);
    expect(tracker.armed).toBe(0);
    expect(tracker.phase(phase({ phase: "ended" }), end)).toContainEqual({ kind: "commit", side: 1, how: "fling" });
  });

  it("follows the direction setting", () => {
    const natural = setup().tracker;
    natural.wheel(-20, 0, 1);
    expect(natural.offset).toBeGreaterThan(0);
    const reversed = setup({ direction: "reversed" }).tracker;
    reversed.wheel(-20, 0, 1);
    expect(reversed.offset).toBeLessThan(0);
    const seq = new WheelSequence();
    seq.phase(phase({ phase: "began", inverted: false }));
    const auto = setup({}, seq).tracker;
    auto.wheel(20, 0, 1);
    expect(auto.offset).toBeGreaterThan(0);
  });

  it("crowds the detents towards the threshold with the ramp", () => {
    const t = { ...DEFAULT_TUNING, detentPx: 12, detentRamp: 2 };
    // 120 px threshold: 10 detents; squared, the first sits at ~38 px and the last two ~6 px apart.
    const crossings: number[] = [];
    let last = 0;
    for (let a = 0; a < 120; a++) {
      const d = detentIndex(a, 120, t);
      if (d !== last) crossings.push(a);
      last = d;
    }
    expect(crossings).toHaveLength(9);
    const gaps = crossings.slice(1).map((c, i) => c - crossings[i]!);
    expect(gaps[0]).toBeGreaterThan(gaps[gaps.length - 1]! * 2);
    expect(detentIndex(60, 120, { ...t, detentRamp: 1 })).toBe(5);
  });

  it("ticks a detent every detentPx short of the threshold, and none past it", () => {
    const { tracker } = setup({ detentPx: 20, detentRamp: 1, resistance: 0 });
    tracker.phase(phase({ phase: "began" }), 0);
    const { effects } = swipe(tracker, 200, 20, 1);
    const detents = effects.filter((e) => e.kind === "haptic" && e.reason === "detent").length;
    // 20, 40, … 100 px: five ticks before arming at 120, then nothing.
    expect(detents).toBe(5);
    expect(kinds(effects).indexOf("haptic:arm")).toBeGreaterThan(kinds(effects).lastIndexOf("haptic:detent"));
  });

  it("leaves a stream alone when neither side has an action", () => {
    const { tracker } = setup();
    tracker.sides = { 1: false, [-1]: false };
    const { consumed } = swipe(tracker, 200, 10, 1);
    expect(consumed).toBe(false);
    expect(tracker.stage).toBe("passthrough");
  });

  it("never starts halfway through a touch, even with the fingers down (AppKit)", () => {
    const seq = new WheelSequence();
    const page = setup({}, seq).tracker;
    const row = setup({}, seq).tracker;
    page.phase(phase({ phase: "began" }), 0);
    // The page takes the touch's first events; a row the lean brings under the pointer joins late.
    swipe(page, 40, 4, 1);
    expect(page.stage).toBe("tracking");
    expect(row.wouldStart(40)).toBe(false);
    expect(row.wheel(-10, 0, 40).consumed).toBe(false);
    expect(row.stage).toBe("passthrough");
    // The next touch starts afresh.
    seq.phase(phase({ phase: "ended" }));
    page.phase(phase({ phase: "ended" }), 60);
    seq.phase(phase({ phase: "began" }));
    expect(row.wouldStart(500)).toBe(true);
  });

  it("asks again where a stream starts after a pause with the fingers resting", () => {
    const { tracker, seq } = setup();
    tracker.phase(phase({ phase: "began" }), 0);
    for (let i = 0; i < 10; i++) tracker.wheel(0, 20, 1 + i * 8);
    expect(tracker.stage).toBe("passthrough");
    expect(tracker.wouldStart(90)).toBe(false);
    // A pause past the idle gap: what comes next is a new stream, on whatever is under the pointer now.
    expect(tracker.wouldStart(300)).toBe(true);
    expect(seq.fingersDown).toBe(true);
  });

  it("lets go of momentum whose end never arrived", () => {
    const { tracker, seq } = setup();
    tracker.phase(phase({ phase: "began" }), 0);
    swipe(tracker, 160, 10, 1);
    tracker.phase(phase({ phase: "ended" }), 90);
    tracker.phase(phase({ momentum: "began" }), 95);
    tracker.wheel(-30, 0, 100);
    expect(tracker.stage).toBe("coasting");
    // The pointer left the window: no momentum "ended". After a quiet spell it is over.
    tracker.tick(100 + STALE_MOMENTUM_MS + 1);
    expect(tracker.stage).toBe("idle");
    expect(seq.canStart(100 + STALE_MOMENTUM_MS + 1, DEFAULT_TUNING.idleMs)).toBe(true);
    expect(tracker.wheel(0, 30, 100 + STALE_MOMENTUM_MS + 2).consumed).toBe(false);
  });

  it("never swipes on a mouse wheel's stream", () => {
    const { tracker } = setup();
    // A tilt wheel or Shift+wheel: 100 px notches sideways.
    const r = tracker.wheel(-100, 0, 1, true);
    expect(r.consumed).toBe(false);
    expect(tracker.stage).toBe("passthrough");
    // The notches after it are the same stream, coarse or not.
    for (let i = 1; i < 6; i++) expect(tracker.wheel(-100, 0, 1 + i * 30, false).consumed).toBe(false);
    expect(tracker.offset).toBe(0);
    // A trackpad stream after a pause swipes again.
    const { consumed } = swipe(tracker, 160, 10, 1000);
    expect(consumed).toBe(true);
    expect(tracker.armed).toBe(1);
  });

  it("lets nested surfaces read the same event as the start of a stream", () => {
    const seq = new WheelSequence();
    const inner = setup({}, seq).tracker;
    const outer = setup({}, seq).tracker;
    inner.sides = { 1: false, [-1]: false };
    // The inner surface sees each event first and passes it on.
    for (let i = 0; i < 10; i++) {
      const t = 1 + i * 8;
      inner.wheel(-16, 0, t);
      outer.wheel(-16, 0, t);
    }
    expect(inner.stage).toBe("passthrough");
    expect(outer.stage).toBe("tracking");
    expect(outer.armed).toBe(1);
  });

  it("does not move towards a side without an action", () => {
    const { tracker } = setup();
    tracker.sides = { 1: false, [-1]: true };
    swipe(tracker, 200, 10, 1);
    expect(tracker.offset).toBe(0);
    expect(tracker.armed).toBe(0);
  });
});

describe("coarseWheel", () => {
  const ev = (deltaX: number, deltaY: number, more: { deltaMode?: number; shiftKey?: boolean } = {}) => ({ deltaX, deltaY, deltaMode: 0, shiftKey: false, ...more });
  it("takes lines, Shift and notch-sized jumps for a mouse wheel", () => {
    expect(coarseWheel(ev(0, 3, { deltaMode: 1 }))).toBe(true);
    expect(coarseWheel(ev(0, 4, { shiftKey: true }))).toBe(true);
    // Windows: 100 px a notch (120 on some mice); Linux: ~53.
    expect(coarseWheel(ev(0, 100))).toBe(true);
    expect(coarseWheel(ev(-120, 0))).toBe(true);
    expect(coarseWheel(ev(0, 53.33))).toBe(true);
  });
  it("takes the small pixel deltas fingers start with for a trackpad", () => {
    expect(coarseWheel(ev(-4, 0.5))).toBe(false);
    expect(coarseWheel(ev(12.5, -3))).toBe(false);
  });
});

describe("resolveTuning", () => {
  const prefs: TuningPrefs = { sensitivity: "medium", direction: "system", haptics: true };
  it("ends swipes by AppKit's phases on macOS, decaying deltas on Windows and an idle gap on Linux", () => {
    expect(defaultRelease("macOs")).toBe("native");
    expect(defaultRelease("windows")).toBe("momentum");
    expect(defaultRelease("linux")).toBe("idle");
    expect(resolveTuning("rows", "windows", prefs).release).toBe("momentum");
    expect(resolveTuning("page", "linux", prefs).release).toBe("idle");
    expect(resolveTuning("cover", "macOs", prefs).release).toBe("native");
  });

  it("keeps the shipped tuning at medium on a Mac", () => {
    expect(resolveTuning("rows", "macOs", prefs)).toEqual({ ...SURFACE_TUNING.rows, release: "native", direction: "auto" });
    expect(resolveTuning("queue", "macOs", prefs)).toEqual(resolveTuning("rows", "macOs", prefs));
    expect(SURFACE_TUNING.rows).toMatchObject({ threshold: 0.2, hapticDetent: "alignment", hapticArm: "actuator", kickId: 15, kickArg1: 3, kickArg2: 3, hapticDisarm: "alignment", hapticCommit: "levelChange", hapticStart: "none" });
    expect(SURFACE_TUNING.cover).toMatchObject({ threshold: 0.4, maxTravel: 0.35, resistance: 0.25, flingVelocity: 1.2, hapticCommit: "none" });
    expect(SURFACE_TUNING.page).toMatchObject({ threshold: 0.16, maxTravel: 0.3, resistance: 0.6, flingVelocity: 1, hapticDisarm: "none", hapticCommit: "none" });
  });

  it("scales the threshold (and the detents and fling with it) by sensitivity", () => {
    const low = resolveTuning("rows", "macOs", { ...prefs, sensitivity: "low" });
    const high = resolveTuning("rows", "macOs", { ...prefs, sensitivity: "high" });
    expect(low.threshold).toBeGreaterThan(0.2);
    expect(high.threshold).toBeLessThan(0.2);
    expect(high.detentPx / high.threshold).toBeCloseTo(TUNED.detentPx / 0.2);
    expect(high.flingVelocity).toBeLessThan(low.flingVelocity);
    // Still inside the travel, even on the cover's short leash.
    const cover = resolveTuning("cover", "macOs", { ...prefs, sensitivity: "low" });
    expect(cover.maxTravel).toBeGreaterThan(cover.threshold);
  });

  it("swipes a shorter way at high sensitivity", () => {
    const run = (sensitivity: TuningPrefs["sensitivity"]) => {
      const tracker = new SwipeTracker(resolveTuning("rows", "linux", { ...prefs, sensitivity }), new WheelSequence());
      tracker.width = W;
      swipe(tracker, 70, 7, 1);
      return tracker.armed;
    };
    // 70 of 400 px: short of medium's 80 px threshold, past high's 56.
    expect(run("medium")).toBe(0);
    expect(run("high")).toBe(1);
  });

  it("follows AppKit on macOS and natural scrolling elsewhere, unless told", () => {
    expect(systemDirection("macOs")).toBe("auto");
    expect(systemDirection("windows")).toBe("natural");
    expect(resolveTuning("rows", "linux", prefs).direction).toBe("natural");
    expect(resolveTuning("rows", "macOs", { ...prefs, direction: "reversed" }).direction).toBe("reversed");
  });

  it("silences every haptic when they are off, and off macOS", () => {
    for (const t of [resolveTuning("rows", "macOs", { ...prefs, haptics: false }), resolveTuning("rows", "windows", prefs)]) {
      expect([t.hapticStart, t.hapticDetent, t.hapticArm, t.hapticDisarm, t.hapticCommit]).toEqual(["none", "none", "none", "none", "none"]);
    }
  });
});
