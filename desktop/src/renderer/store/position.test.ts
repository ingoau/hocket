import { describe, expect, it } from "vitest";
import { extrapolate } from "./position";

describe("extrapolate", () => {
  it("holds still when paused", () => {
    expect(extrapolate({ positionMs: 1000, takenAt: 0, rate: 1, isPlaying: false }, 5000)).toBe(1000);
  });
  it("advances with rate while playing and clamps to duration", () => {
    expect(extrapolate({ positionMs: 1000, takenAt: 1000, rate: 1, isPlaying: true }, 3500)).toBe(3500);
    expect(extrapolate({ positionMs: 1000, takenAt: 1000, rate: 1, isPlaying: true }, 9000, 0, 5000)).toBe(5000);
  });
  it("applies the clock offset and snaps after an hour gap", () => {
    expect(extrapolate({ positionMs: 0, takenAt: 10_000, rate: 1, isPlaying: true }, 10_000, 500)).toBe(500);
    expect(extrapolate({ positionMs: 0, takenAt: 0, rate: 1, isPlaying: true }, 3_700_000)).toBe(0);
  });
});
