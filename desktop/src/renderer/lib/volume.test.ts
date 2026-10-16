import { describe, expect, it } from "vitest";
import { volumeStep } from "./volume";

describe("relative volume steps", () => {
  it("steps the level, clamped to 0–1 and rounded to 1 %", () => {
    expect(volumeStep(0.5, false, 0.05)).toBe(0.55);
    expect(volumeStep(0.98, false, 0.05)).toBe(1);
    expect(volumeStep(0.02, false, -0.05)).toBe(0);
  });

  it("unmutes from silence going up, and stays muted going down", () => {
    expect(volumeStep(0.6, true, 0.05)).toBe(0.05);
    expect(volumeStep(0.6, true, -0.05)).toBeUndefined();
    expect(volumeStep(0.6, true, -0.2)).toBeUndefined();
  });
});
