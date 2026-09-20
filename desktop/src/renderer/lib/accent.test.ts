import { describe, expect, it } from "vitest";
import { pickAccent } from "./accent";

describe("pickAccent", () => {
  it("picks the vivid colour over grey and ignores near-black/white", () => {
    const px: number[] = [];
    for (let i = 0; i < 100; i++) px.push(128, 128, 128, 255); // grey
    for (let i = 0; i < 30; i++) px.push(220, 40, 60, 255); // red
    for (let i = 0; i < 50; i++) px.push(2, 2, 2, 255); // black
    const c = pickAccent(new Uint8ClampedArray(px))!;
    const r = parseInt(c.slice(1, 3), 16);
    const g = parseInt(c.slice(3, 5), 16);
    expect(r).toBeGreaterThan(180);
    expect(g).toBeLessThan(80);
  });
  it("returns undefined for an all-black image", () => {
    expect(pickAccent(new Uint8ClampedArray([0, 0, 0, 255, 5, 5, 5, 255]))).toBeUndefined();
  });
});
