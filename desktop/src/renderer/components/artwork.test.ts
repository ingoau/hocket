import { describe, expect, it } from "vitest";
import { artworkWaits } from "./Artwork";

describe("artwork dwell", () => {
  it("waits only in a list that asked to, and only for a cover not asked for yet", () => {
    // The player, the mini player, the now-playing sidebar: at once, no placeholder flash on a track change.
    expect(artworkWaits(false, "al1", false)).toBe(false);
    // A virtualised row or grid cell: a fast scroll doesn't send a query per row it passes.
    expect(artworkWaits(true, "al1", false)).toBe(true);
    expect(artworkWaits(true, "al1", true)).toBe(false);
    expect(artworkWaits(true, undefined, false)).toBe(false);
  });
});
