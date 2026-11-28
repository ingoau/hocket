import { describe, expect, it } from "vitest";
import { gridColumns, resolvedColumns } from "./grid-columns";

describe("gridColumns", () => {
  it("fits as many columns as the minimum width allows, then fills the row", () => {
    // 4 × 160 + 3 × 14 = 682 fits in 734; 5 × 160 + 4 × 14 = 856 doesn't.
    const g = gridColumns(734, 160, 14);
    expect(g.cols).toBe(4);
    expect(g.cols * g.colW + (g.cols - 1) * 14).toBeCloseTo(734);
  });

  it("counts the gap only between columns", () => {
    // Exactly 3 × 160 + 2 × 14 = 508: three columns, not two.
    expect(gridColumns(508, 160, 14)).toEqual({ cols: 3, colW: 160 });
    expect(gridColumns(507, 160, 14).cols).toBe(2);
  });

  it("keeps one column however narrow", () => {
    expect(gridColumns(90, 160, 14)).toEqual({ cols: 1, colW: 90 });
    expect(gridColumns(-5, 160, 14)).toEqual({ cols: 1, colW: 0 });
  });
});

describe("resolvedColumns", () => {
  it("reads a laid-out grid's tracks", () => {
    expect(resolvedColumns("237.333px 237.333px 237.333px")).toEqual({ cols: 3, colW: 237.333 });
  });

  it("is undefined before layout", () => {
    expect(resolvedColumns("none")).toBeUndefined();
    expect(resolvedColumns("")).toBeUndefined();
  });
});
