import { describe, expect, it } from "vitest";
import { TOP_SONGS_COUNT, TOP_SONGS_PREVIEW, topSongsSection } from "./top-songs";

describe("topSongsSection", () => {
  it("holds as many rows while loading as the first answer shows", () => {
    const loading = topSongsSection(undefined, false);
    const full = topSongsSection(TOP_SONGS_PREVIEW, false);
    expect(loading.kind).toBe("loading");
    expect(full.kind).toBe("list");
    expect(loading.kind !== "none" && full.kind !== "none" && loading.rows === full.rows).toBe(true);
  });

  it("hides the section when there are no songs or the query failed", () => {
    expect(topSongsSection(0, false)).toEqual({ kind: "none" });
    expect(topSongsSection(undefined, true)).toEqual({ kind: "none" });
  });

  it("sizes a short list to its rows", () => {
    const three = topSongsSection(2, false);
    const full = topSongsSection(TOP_SONGS_COUNT, false);
    expect(three.kind === "list" && full.kind === "list" && three.rows < full.rows).toBe(true);
  });
});
