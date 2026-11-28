import { describe, expect, it } from "vitest";
import { previewPosition, previewProgress } from "./preview-seek";

describe("preview progress", () => {
  it("extrapolates while playing and holds while paused", () => {
    const base = { positionMs: 3000, takenAt: 1000, durationMs: 10000 };
    expect(previewPosition({ ...base, playing: true }, 1500)).toBe(3500);
    expect(previewPosition({ ...base, playing: false }, 9000)).toBe(3000);
    expect(previewPosition({ positionMs: 9500, takenAt: 0, playing: true, durationMs: 10000 }, 2000)).toBe(10000);
  });

  it("runs through the clip, else the whole song", () => {
    const clip = { positionMs: 60_000, takenAt: 0, playing: true, durationMs: 200_000, clip: { startMs: 60_000, endMs: 90_000 } };
    expect(previewProgress(clip, 0)).toBe(0);
    expect(previewProgress(clip, 15_000)).toBeCloseTo(0.5);
    expect(previewProgress(clip, 60_000)).toBe(1);
    expect(previewProgress({ ...clip, clip: undefined }, 40_000)).toBeCloseTo(0.5);
    expect(previewProgress({ positionMs: 0, takenAt: 0, playing: false, durationMs: 0 }, 0)).toBe(0);
  });
});
