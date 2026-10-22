import { describe, expect, it } from "vitest";
import { moveBlock, movesToInsertBlock, parseLibraryTarget } from "./queue-drag";

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

/** Apply MoveQueueItem semantics (remove, then insert at the final index). */
function apply(order: string[], moves: { key: string; to_index: number }[]): string[] {
  const out = [...order];
  for (const m of moves) {
    out.splice(out.indexOf(m.key), 1);
    out.splice(m.to_index, 0, m.key);
  }
  return out;
}

describe("moveBlock", () => {
  const order = ["a", "b", "c", "d", "e", "f"];

  it("moves a scattered selection down, together and in queue order", () => {
    expect(apply(order, moveBlock(order, ["e", "a", "c"], "f"))).toEqual(["b", "d", "a", "c", "e", "f"]);
  });

  it("moves a selection up in front of an item", () => {
    expect(apply(order, moveBlock(order, ["d", "f"], "b"))).toEqual(["a", "d", "f", "b", "c", "e"]);
  });

  it("moves a selection to the end", () => {
    expect(apply(order, moveBlock(order, ["b", "c"], undefined))).toEqual(["a", "d", "e", "f", "b", "c"]);
  });

  it("leaves items already in place alone", () => {
    expect(moveBlock(order, ["c", "d"], "e")).toEqual([]);
    expect(moveBlock(order, ["a", "c"], "d")).toEqual([{ key: "a", to_index: 1 }]);
  });

  it("does nothing for a target inside the block or not in the queue", () => {
    expect(moveBlock(order, ["b", "c"], "c")).toEqual([]);
    expect(moveBlock(order, ["b"], "zz")).toEqual([]);
  });
});
