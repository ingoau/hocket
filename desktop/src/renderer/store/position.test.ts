import { describe, expect, it } from "vitest";
import { extrapolate, frameDue } from "./position";

describe("frameDue", () => {
  /** Frames painted in one second of vsync at `hz` (with ±0.3 ms of jitter) under an `fps` cap. */
  const painted = (hz: number, fps: number) => {
    let last = 0;
    let n = 0;
    for (let i = 1; i <= hz; i++) {
      const t = (i * 1000) / hz + (i % 2 ? 0.3 : -0.3);
      if (frameDue(t - last, fps)) {
        last = t;
        n++;
      }
    }
    return n;
  };
  it("keeps a 30 fps cap at 30 on a 60 Hz display, not every third frame", () => {
    expect(painted(60, 30)).toBe(30);
  });
  it("never goes over the cap, and paints every frame when the cap is the refresh rate", () => {
    expect(painted(60, 60)).toBe(60);
    expect(painted(120, 30)).toBe(30);
    expect(painted(144, 30)).toBeLessThanOrEqual(30);
    expect(painted(144, 30)).toBeGreaterThanOrEqual(28);
    expect(painted(60, 24)).toBeLessThanOrEqual(24);
  });
});

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
