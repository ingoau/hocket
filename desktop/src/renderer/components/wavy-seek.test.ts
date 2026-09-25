import { describe, expect, it } from "vitest";
import { WAVE, wavePath } from "./WavySeek";

const points = (d: string) => d.replace(/^M /, "").split(" L ").map((p) => p.split(" ").map(Number) as [number, number]);

describe("wavy seek path", () => {
  it("is empty when nothing has played", () => {
    expect(wavePath(3, 3, 20, WAVE.amplitude)).toBe("");
  });

  it("tapers into the start and the thumb, and swings by the amplitude in between", () => {
    const pts = points(wavePath(3, 400, 20, WAVE.amplitude));
    expect(pts[0]).toEqual([3, 20]);
    expect(pts[pts.length - 1]).toEqual([400, 20]);
    const ys = pts.map(([, y]) => y);
    expect(Math.max(...ys)).toBeCloseTo(20 + WAVE.amplitude, 0);
    expect(Math.min(...ys)).toBeCloseTo(20 - WAVE.amplitude, 0);
  });

  it("is a flat line at zero amplitude", () => {
    expect(points(wavePath(3, 200, 20, 0)).every(([, y]) => y === 20)).toBe(true);
  });

  it("moves its crests forward with the phase", () => {
    const a = points(wavePath(3, 400, 20, 4, 0));
    const b = points(wavePath(3, 400, 20, 4, WAVE.wavelength / 4));
    expect(a[100]![1]).not.toBeCloseTo(b[100]![1], 1);
  });
});
