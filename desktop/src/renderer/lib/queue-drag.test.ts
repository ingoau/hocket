import { describe, expect, it } from "vitest";
import { GROUP_END, applyMove, dropSlot, parseLibraryTarget, placeBlock, sortableOrder, stepMove, type QueueMove } from "./queue-drag";

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

const END = GROUP_END;
/** Apply moves the way the core does (applyMove mirrors MoveQueueItem). */
function apply(order: string[], moves: QueueMove[]): string[] {
  return moves.reduce<string[]>((o, m) => applyMove(o, m), order);
}

describe("sortableOrder", () => {
  it("puts the Next from heading between the group and the rest, only with the section on", () => {
    expect(sortableOrder(["g1"], ["r1", "r2"], true)).toEqual(["g1", END, "r1", "r2"]);
    expect(sortableOrder([], ["r1"], false)).toEqual(["r1"]);
  });
});

describe("dropSlot", () => {
  const order = ["g1", "g2", END, "r1", "r2"];

  it("joins the group above the heading and not below it", () => {
    expect(dropSlot(order, 0, "cur")).toEqual({ after: "cur", inGroup: true });
    expect(dropSlot(order, 1, "cur")).toEqual({ after: "g1", inGroup: true });
    expect(dropSlot(order, 2, "cur")).toEqual({ after: "g2", inGroup: true });
    expect(dropSlot(order, 3, "cur")).toEqual({ after: "g2", inGroup: false });
    expect(dropSlot(order, 5, "cur")).toEqual({ after: "r2", inGroup: false });
  });

  it("with an empty group, above the heading starts one right after the current song", () => {
    expect(dropSlot([END, "r1"], 0, "cur")).toEqual({ after: "cur", inGroup: true });
    expect(dropSlot([END, "r1"], 1, "cur")).toEqual({ after: "cur", inGroup: false });
  });

  it("without a heading, membership is the given default", () => {
    expect(dropSlot(["r1", "r2"], 1, "cur")).toEqual({ after: "r1", inGroup: false });
    expect(dropSlot(["g1"], 1, "cur", new Set(), true)).toEqual({ after: "g1", inGroup: true });
  });

  it("clamps to the top: nothing lands above the current song", () => {
    expect(dropSlot(order, -3, "cur")).toEqual({ after: "cur", inGroup: true });
  });
});

describe("applyMove", () => {
  it("moves after the named entry, keeping membership by the heading", () => {
    const order = ["g1", "g2", END, "r1", "r2"];
    expect(applyMove(order, { key: "r2", after: "g1", in_group: true })).toEqual(["g1", "r2", "g2", END, "r1"]);
    expect(applyMove(order, { key: "g1", after: "g2", in_group: false })).toEqual(["g2", END, "g1", "r1", "r2"]);
    // An in-group drop past the group's end lands at its end; out-of-group inside it, just after.
    expect(applyMove(order, { key: "r2", after: "r1", in_group: true })).toEqual(["g1", "g2", "r2", END, "r1"]);
    expect(applyMove(order, { key: "r2", after: "cur", in_group: false })).toEqual(["g1", "g2", END, "r2", "r1"]);
  });
});

describe("placeBlock", () => {
  const order = ["g1", "g2", END, "r1", "r2", "r3"];
  const drag = (active: string, overIndex: number) => {
    const from = order.indexOf(active);
    const out = [...order];
    out.splice(from, 1);
    out.splice(overIndex, 0, active);
    return out;
  };

  it("dragging a row above the Next from heading puts it in the group", () => {
    const moves = placeBlock(order, drag("r2", 2), "r2", ["r2"], "cur");
    expect(moves).toEqual([{ key: "r2", after: "g2", in_group: true }]);
    expect(apply(order, moves)).toEqual(["g1", "g2", "r2", END, "r1", "r3"]);
  });

  it("dragging a group row below the heading takes it out", () => {
    const moves = placeBlock(order, drag("g1", 2), "g1", ["g1"], "cur");
    expect(moves).toEqual([{ key: "g1", after: "g2", in_group: false }]);
    expect(apply(order, moves)).toEqual(["g2", END, "g1", "r1", "r2", "r3"]);
  });

  it("the top of the list is right after the current song", () => {
    expect(placeBlock(order, drag("r3", 0), "r3", ["r3"], "cur")).toEqual([{ key: "r3", after: "cur", in_group: true }]);
  });

  it("with the section off every drop is out of the group", () => {
    const flat = ["a", "b", "c"];
    expect(placeBlock(flat, ["c", "a", "b"], "c", ["c"], "cur")).toEqual([{ key: "c", after: "cur", in_group: false }]);
  });

  it("moves a scattered selection together, in queue order", () => {
    // Drag r3 (with g1 and r1 selected) to just below the heading.
    const moves = placeBlock(order, drag("r3", 3), "r3", ["g1", "r1", "r3"], "cur");
    expect(apply(order, moves)).toEqual(["g2", END, "g1", "r1", "r3", "r2"]);
    expect(moves.every((m) => !m.in_group)).toBe(true);
  });

  it("leaves out moves that change nothing", () => {
    expect(placeBlock(order, order, "r1", ["r1"], "cur")).toEqual([]);
    expect(placeBlock(order, drag("r2", 4), "r2", ["r1", "r2"], "cur")).toEqual([]);
  });
});

describe("stepMove", () => {
  const order = ["g1", END, "r1", "r2"];
  it("steps across the heading into and out of the group", () => {
    expect(stepMove(order, "r1", -1, "cur")).toEqual([{ key: "r1", after: "g1", in_group: true }]);
    expect(stepMove(order, "g1", 1, "cur")).toEqual([{ key: "g1", after: "cur", in_group: false }]);
    expect(stepMove(order, "r1", 1, "cur")).toEqual([{ key: "r1", after: "r2", in_group: false }]);
    expect(stepMove(order, "g1", -1, "cur")).toEqual([]);
    expect(stepMove(order, "r2", 1, "cur")).toEqual([]);
  });
});
