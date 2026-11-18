import { describe, expect, it } from "vitest";
import type { ScrollPhase } from "@shared/bridge-types";
import { DEFAULT_TUNING as TUNED, detentActuation, detentIndex, shapeTravel, SwipeTracker, WheelSequence, type SwipeEffect, type SwipeTuning } from "./swipe";

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

  it("ramps the detents' actuation from light to strong, slowly at first", () => {
    const t = { ...DEFAULT_TUNING, detentIdFrom: 1, detentIdTo: 4, detentStrengthCurve: 2.5 };
    expect(detentActuation(t, 0)).toBe(1);
    expect(detentActuation(t, 0.3)).toBe(1);
    expect(detentActuation(t, 0.5)).toBe(2);
    expect(detentActuation(t, 0.8)).toBe(3);
    expect(detentActuation(t, 1)).toBe(4);
  });

  it("tells each detent how far along it is", () => {
    const { tracker } = setup({ detentPx: 20, detentRamp: 1, resistance: 0 });
    const { effects } = swipe(tracker, 110, 11, 1);
    const progress = effects.flatMap((e) => (e.kind === "haptic" && e.reason === "detent" ? [e.progress ?? -1] : []));
    expect(progress).toEqual([20 / 120, 40 / 120, 60 / 120, 80 / 120, 100 / 120]);
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

  it("does not move towards a side without an action", () => {
    const { tracker } = setup();
    tracker.sides = { 1: false, [-1]: true };
    swipe(tracker, 200, 10, 1);
    expect(tracker.offset).toBe(0);
    expect(tracker.armed).toBe(0);
  });
});
