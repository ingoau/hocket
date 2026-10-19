import { describe, expect, it } from "vitest";
import { evictPages, materializePages } from "./paged-window";

describe("paged window", () => {
  it("materialises only loaded pages into a holey array", () => {
    const pages = new Map<number, string[]>([
      [0, ["a", "b"]],
      [2, ["e", "f"]],
    ]);
    const rows = materializePages(pages, 6, 2);
    expect(rows.length).toBe(6);
    expect(rows[0]).toBe("a");
    expect(rows[1]).toBe("b");
    expect(rows[2]).toBeUndefined();
    expect(rows[4]).toBe("e");
  });

  it("evicts pages far from the viewport, keeping a window of 12", () => {
    const pages = new Map<number, number[]>();
    for (let p = 0; p < 40; p++) pages.set(p, [p]);
    evictPages(pages, 20);
    expect(pages.size).toBe(12);
    expect(pages.has(20)).toBe(true);
    expect(pages.has(0)).toBe(false);
    expect(pages.has(39)).toBe(false);
  });
});
