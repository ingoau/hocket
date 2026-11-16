import { describe, expect, it } from "vitest";
import { movesToInsertBlock, parseLibraryTarget, queueDropPlan, reorderMoves } from "./queue-drag";

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

/** Applies moveQueueItem steps the way the core does: take the key out, put it at `to`. */
function apply(list: string[], moves: { key: string; to: number }[]): string[] {
  const out = [...list];
  for (const m of moves) {
    out.splice(out.indexOf(m.key), 1);
    out.splice(m.to, 0, m.key);
  }
  return out;
}

describe("reorderMoves", () => {
  const L = ["A", "B", "C", "D", "E"];
  it("moves one row down or up in one step", () => {
    const down = reorderMoves(L, ["A"], 3);
    expect(apply(L, down)).toEqual(["B", "C", "A", "D", "E"]);
    expect(down).toHaveLength(1);
    const up = reorderMoves(L, ["D"], 1);
    expect(apply(L, up)).toEqual(["A", "D", "B", "C", "E"]);
    expect(up).toHaveLength(1);
  });

  it("moves several rows together, in list order, to the gap", () => {
    expect(apply(L, reorderMoves(L, ["D", "A"], 5))).toEqual(["B", "C", "E", "A", "D"]);
    expect(apply(L, reorderMoves(L, ["E", "B"], 0))).toEqual(["B", "E", "A", "C", "D"]);
    expect(apply(L, reorderMoves(L, ["B", "D"], 3))).toEqual(["A", "C", "B", "D", "E"]);
    // Fewest steps: here moving C up past B is the same result as moving B and D.
    expect(reorderMoves(L, ["B", "D"], 3)).toHaveLength(1);
  });

  it("does nothing for a drop where the rows already are, or unknown keys", () => {
    expect(reorderMoves(L, ["B"], 1)).toEqual([]);
    expect(reorderMoves(L, ["B"], 2)).toEqual([]);
    expect(reorderMoves(L, ["Z"], 2)).toEqual([]);
  });

  it("matches the wanted order for every single-row move", () => {
    for (let from = 0; from < L.length; from++) {
      for (let gap = 0; gap <= L.length; gap++) {
        const key = L[from] as string;
        const want = [...L];
        want.splice(from, 1);
        want.splice(gap > from ? gap - 1 : gap, 0, key);
        expect(apply(L, reorderMoves(L, [key], gap))).toEqual(want);
      }
    }
  });
});

describe("queueDropPlan", () => {
  const reorderable = ["n1", "n2", "u1"];
  const trackOf = (k: string) => ({ h1: "th1", cur: "tc", n1: "t1", n2: "t2", u1: "t3" })[k];
  it("adds library items", () => {
    expect(queueDropPlan({ type: "albums", data: { ids: ["al"] } }, reorderable, 1, trackOf)).toEqual({ kind: "add", target: { type: "albums", data: { ids: ["al"] } } });
  });

  it("moves rows that can move", () => {
    expect(queueDropPlan({ type: "queueItems", data: { keys: ["u1"] } }, reorderable, 0, trackOf)).toEqual({ kind: "move", moves: [{ key: "u1", to: 0 }] });
  });

  it("adds history and the current song again (they can't move)", () => {
    expect(queueDropPlan({ type: "queueItems", data: { keys: ["h1", "cur"] } }, reorderable, 2, trackOf)).toEqual({ kind: "add", target: { type: "tracks", data: { ids: ["th1", "tc"] } } });
    expect(queueDropPlan({ type: "queueItems", data: { keys: ["gone"] } }, reorderable, 2, trackOf)).toBeUndefined();
  });
});
