import { describe, expect, it } from "vitest";
import { previewPosition, seekFraction } from "./preview-seek";

describe("preview seek", () => {
  it("stays inside the row and clear of the edges", () => {
    expect(seekFraction(0, 200, 24)).toBe(0);
    expect(seekFraction(24, 200, 24)).toBe(0);
    expect(seekFraction(200, 200, 24)).toBe(1);
    expect(seekFraction(100, 200, 24)).toBeCloseTo(0.5);
  });

  it("extrapolates while playing and holds while paused", () => {
    const base = { positionMs: 3000, takenAt: 1000, durationMs: 10000 };
    expect(previewPosition({ ...base, playing: true }, 1500)).toBe(3500);
    expect(previewPosition({ ...base, playing: false }, 9000)).toBe(3000);
    expect(previewPosition({ positionMs: 9500, takenAt: 0, playing: true, durationMs: 10000 }, 2000)).toBe(10000);
  });
});