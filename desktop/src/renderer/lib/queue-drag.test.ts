import { describe, expect, it } from "vitest";
import { movesToInsertBlock, parseLibraryTarget } from "./queue-drag";

describe("parseLibraryTarget", () => {
  it("reads a tracks, albums, artists or playlists target", () => {
    expect(parseLibraryTarget(JSON.stringify({ type: "tracks", data: { ids: ["a", "b"] } }))).toEqual({ type: "tracks", data: { ids: ["a", "b"] } });
    expect(parseLibraryTarget(JSON.stringify({ type: "albums", data: { ids: ["al"] } }))).toEqual({ type: "albums", data: { ids: ["al"] } });
    expect(parseLibraryTarget(JSON.stringify({ type: "artists", data: { ids: ["ar"] } }))).toEqual({ type: "artists", data: { ids: ["ar"] } });
    expect(parseLibraryTarget(JSON.stringify({ type: "playlists", data: { ids: ["pl"] } }))).toEqual({ type: "playlists", data: { ids: ["pl"] } });
  });

  it("drops queue items, empty lists, and junk", () => {
    expect(parseLibraryTarget(JSON.stringify({ type: "queueItems", data: { keys: ["k"] } }))).toBeUndefined();
    expect(parseLibraryTarget(JSON.stringify({ type: "tracks", data: { ids: [] } }))).toBeUndefined();
    expect(parseLibraryTarget(JSON.stringify({ type: "tracks", data: { ids: [1] } }))).toBeUndefined();
    expect(parseLibraryTarget("not json")).toBeUndefined();
  });
});

describe("movesToInsertBlock", () => {
  it("is a no-op at the front or with nothing added", () => {
    expect(movesToInsertBlock(2, 0)).toEqual([]);
    expect(movesToInsertBlock(0, 3)).toEqual([]);
  });

  it("moves the block last-first so it stays together at toIndex", () => {
    // [X, Y, A, B, C] → want [A, B, X, Y, C] (toIndex 2): Y to 3, then X to 2.
    expect(movesToInsertBlock(2, 2)).toEqual([3, 2]);
    // [X, Y, A, B, C] → want [A, B, C, X, Y] (toIndex 3).
    expect(movesToInsertBlock(2, 3)).toEqual([4, 3]);
    expect(movesToInsertBlock(1, 4)).toEqual([4]);
  });
});
