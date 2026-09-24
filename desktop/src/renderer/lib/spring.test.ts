import { describe, expect, it } from "vitest";
import { SPRING_EFFECTS, SPRING_SPATIAL, spring, springAt } from "./spring";

const values = (easing: string) => easing.replace(/^linear\(|\)$/g, "").split(", ").map(Number);

describe("spring easing", () => {
  it("starts at rest and ends exactly at 1", () => {
    const v = values(spring(300, 0.7).easing);
    expect(v[0]).toBe(0);
    expect(v[v.length - 1]).toBe(1);
  });

  it("overshoots when underdamped and never when critically damped", () => {
    expect(Math.max(...values(SPRING_SPATIAL.easing))).toBeGreaterThan(1);
    expect(Math.max(...values(SPRING_EFFECTS.easing))).toBeLessThanOrEqual(1);
  });

  it("has settled by the reported duration", () => {
    for (const [k, r] of [[260, 0.72], [700, 0.6], [400, 1]] as const) {
      const { duration } = spring(k, r);
      expect(Math.abs(1 - springAt(duration / 1000, k, r))).toBeLessThan(0.002);
      expect(duration).toBeGreaterThan(100);
      expect(duration).toBeLessThan(2000);
    }
  });
});
