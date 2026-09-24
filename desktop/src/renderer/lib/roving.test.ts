import { describe, expect, it } from "vitest";
import { nextByGeometry, type Box } from "./roving";

const box = (left: number, top: number, width = 100, height = 100): Box => ({ left, top, width, height });

describe("roving focus by geometry", () => {
  // A 3-column wrapping grid with 7 items: rows [0 1 2] [3 4 5] [6].
  const grid = [0, 1, 2, 3, 4, 5, 6].map((i) => box((i % 3) * 110, Math.floor(i / 3) * 110));

  it("moves left and right within a row and stops at its ends", () => {
    expect(nextByGeometry("ArrowRight", 0, grid)).toBe(1);
    expect(nextByGeometry("ArrowRight", 2, grid)).toBeUndefined();
    expect(nextByGeometry("ArrowLeft", 4, grid)).toBe(3);
    expect(nextByGeometry("ArrowLeft", 3, grid)).toBeUndefined();
  });

  it("moves up and down by column, to the nearest item of a shorter last row", () => {
    expect(nextByGeometry("ArrowDown", 1, grid)).toBe(4);
    expect(nextByGeometry("ArrowUp", 4, grid)).toBe(1);
    expect(nextByGeometry("ArrowDown", 5, grid)).toBe(6);
    expect(nextByGeometry("ArrowDown", 6, grid)).toBeUndefined();
    expect(nextByGeometry("ArrowUp", 0, grid)).toBeUndefined();
  });

  it("jumps to the ends with Home and End", () => {
    expect(nextByGeometry("Home", 5, grid)).toBe(0);
    expect(nextByGeometry("End", 1, grid)).toBe(6);
  });

  it("works for a horizontal shelf and a vertical list", () => {
    const shelf = [0, 1, 2].map((i) => box(i * 160, 0, 150, 200));
    expect(nextByGeometry("ArrowRight", 1, shelf)).toBe(2);
    expect(nextByGeometry("ArrowDown", 1, shelf)).toBeUndefined();
    const list = [0, 1, 2].map((i) => box(0, i * 30, 300, 30));
    expect(nextByGeometry("ArrowDown", 0, list)).toBe(1);
    expect(nextByGeometry("ArrowUp", 2, list)).toBe(1);
    expect(nextByGeometry("ArrowRight", 0, list)).toBeUndefined();
  });

  it("ignores other keys and empty lists", () => {
    expect(nextByGeometry("Enter", 0, grid)).toBeUndefined();
    expect(nextByGeometry("ArrowRight", 0, [])).toBeUndefined();
  });
});
