import { describe, expect, it } from "vitest";
import { TOP_SONGS_COUNT, topSongsSection } from "./top-songs";

describe("topSongsSection", () => {
  it("reserves a full list's height while loading", () => {
    const loading = topSongsSection(undefined, false);
    const full = topSongsSection(TOP_SONGS_COUNT, false);
    expect(loading.kind).toBe("loading");
    expect(full.kind).toBe("list");
    expect(loading.kind !== "none" && full.kind !== "none" && loading.height === full.height).toBe(true);
  });

  it("hides the section when there are no songs or the query failed", () => {
    expect(topSongsSection(0, false)).toEqual({ kind: "none" });
    expect(topSongsSection(undefined, true)).toEqual({ kind: "none" });
  });

  it("sizes a short list to its rows", () => {
    const three = topSongsSection(3, false);
    const full = topSongsSection(TOP_SONGS_COUNT, false);
    expect(three.kind === "list" && full.kind === "list" && three.height < full.height).toBe(true);
  });
});
