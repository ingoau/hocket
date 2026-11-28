import { describe, expect, it } from "vitest";
import { pageTarget, tileWidth } from "./Shelf";

describe("shelf tile width", () => {
  it("fits a whole number of tiles, as many as stay at least the minimum", () => {
    // 4 × 160 + 3 × 6 = 658: four fit in 700, each a little wider.
    const w = tileWidth(700, 6, 156);
    expect(w).toBeGreaterThanOrEqual(156);
    expect(4 * w + 3 * 6).toBeLessThanOrEqual(700);
    expect(4 * w + 3 * 6).toBeGreaterThan(699);
  });
  it("never fewer than two", () => {
    expect(tileWidth(200, 6, 156)).toBe(97);
  });
});

describe("shelf paging", () => {
  // Tiles 100 wide with a 10 gap, starting after a 20 inset: 20, 130, 240, …
  const starts = Array.from({ length: 12 }, (_, i) => 20 + i * 110);
  const widths = starts.map(() => 100);
  it("forward: the first tile not wholly in view leads the next page", () => {
    // Viewport 380 with 20 insets: tiles at 20, 130, 240 show whole (to 340 ≤ 360); 350 is cut.
    expect(pageTarget(starts, widths, 0, 380, 20, 10, 1)).toBe(330);
  });
  it("back: a page back, on a whole tile, or the start", () => {
    expect(pageTarget(starts, widths, 330, 380, 20, 10, -1)).toBe(0);
    expect(pageTarget(starts, widths, 660, 380, 20, 10, -1)).toBe(330);
    expect(pageTarget(starts, widths, 100, 380, 20, 10, -1)).toBe(0);
  });
  it("forward at the end stays put", () => {
    expect(pageTarget(starts, widths, 1000, 380, 20, 10, 1)).toBe(1000);
  });
});
