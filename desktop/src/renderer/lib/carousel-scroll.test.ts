import { describe, expect, it } from "vitest";
import { settleIndex } from "./carousel-scroll";

describe("settleIndex", () => {
  it("springs back from a nudge", () => {
    expect(settleIndex(2, 2.1, 5)).toBe(2);
    expect(settleIndex(2, 1.9, 5)).toBe(2);
  });
  it("moves on past a nudge, the way the scroll went", () => {
    expect(settleIndex(0, 0.2, 5)).toBe(1);
    expect(settleIndex(2, 1.8, 5)).toBe(1);
    expect(settleIndex(0, 1.3, 5)).toBe(2);
    expect(settleIndex(3, 1.7, 5)).toBe(1);
  });
  it("settles exactly on a slide it stopped on", () => {
    expect(settleIndex(0, 2, 5)).toBe(2);
    expect(settleIndex(4, 1, 5)).toBe(1);
  });
  it("stays within the slides", () => {
    expect(settleIndex(4, 4.5, 5)).toBe(4);
    expect(settleIndex(0, -0.5, 5)).toBe(0);
  });
});
