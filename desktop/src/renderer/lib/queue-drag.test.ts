import { describe, expect, it } from "vitest";
import type { QueueEntry, QueueView } from "@core/api";
import { EMPTY_QUEUE } from "../store/reducer";
import { GROUP_END, applyMove, dropSlot, insertAnchor, insertTracksCommand, libraryDropPlan, parseLibraryTarget, placeBlock, previewMoves, queueDragLayout, sortableOrder, stepMove, type QueueMove } from "./queue-drag";

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

const entry = (key: string, queued = false): QueueEntry => ({
  item: { key, trackId: `t-${key}`, source: { type: "context", data: { index: 0 } }, queued },
  track: { id: `t-${key}`, serverId: "s", title: key, durationMs: 1, rating: 0, loved: false, offline: "none" },
});

/** h0 | c | `group` queued rows | `rest` rows, the event's window holding `shown` rows after the current one. */
function view(group: number, rest: number, shown = group + rest): QueueView {
  const all = [entry("h0"), entry("c"), ...Array.from({ length: group }, (_, i) => entry(`g${i}`, true)), ...Array.from({ length: rest }, (_, i) => entry(`r${i}`))];
  const entries = all.slice(0, 2 + shown);
  return { ...EMPTY_QUEUE, entries, windowStart: 0, total: all.length, historyCount: 1, currentIndex: 1, groupCount: group, totalUpcoming: group + rest, current: all[1] };
}

describe("queueDragLayout", () => {
  it("with the section off but a group still there, drags within it stay in the group", () => {
    const { order, noHeading } = queueDragLayout(["g0", "g1", "g2"], ["r0", "r1"], false, 3);
    expect(order).toEqual(["g0", "g1", "g2", END, "r0", "r1"]);
    // g2 dragged to the top of the group, and Alt+Up on g1.
    expect(placeBlock(order, ["g2", "g0", "g1", END, "r0", "r1"], "g2", ["g2"], "c", noHeading)).toEqual([{ key: "g2", after: "c", in_group: true }]);
    expect(stepMove(order, "g1", -1, "c", noHeading)).toEqual([{ key: "g1", after: "c", in_group: true }]);
    // Below the heading it leaves the group.
    expect(stepMove(order, "g2", 1, "c", noHeading)).toEqual([{ key: "g2", after: "g1", in_group: false }]);
  });

  it("with the section off and a group that is everything upcoming, every drop is in it", () => {
    const { order, noHeading } = queueDragLayout(["g0", "g1"], [], false, 2);
    expect(order).toEqual(["g0", "g1"]);
    expect(placeBlock(order, ["g1", "g0"], "g1", ["g1"], "c", noHeading)).toEqual([{ key: "g1", after: "c", in_group: true }]);
  });

  it("with the section off and no group, there is no heading and nothing joins one", () => {
    const { order, noHeading } = queueDragLayout([], ["r0", "r1"], false, 0);
    expect(order).toEqual(["r0", "r1"]);
    expect(stepMove(order, "r1", -1, "c", noHeading)).toEqual([{ key: "r1", after: "c", in_group: false }]);
  });
});

describe("libraryDropPlan", () => {
  it("uses Play next, Add to queue or Play later only where they land exactly", () => {
    const q = view(2, 3);
    expect(libraryDropPlan(q, true)).toEqual({ action: "addToQueue" });
    expect(libraryDropPlan(q, false)).toEqual({ action: "playNext" });
    expect(libraryDropPlan(q, true, { after: "c", inGroup: true })).toEqual({ action: "playNext" });
    expect(libraryDropPlan(q, true, { after: "g1", inGroup: true })).toEqual({ action: "addToQueue" });
    expect(libraryDropPlan(q, true, { after: "r2", inGroup: false })).toEqual({ action: "playLater" });
    // With the section off Play next joins an existing group, so the top out of it is not Play next.
    expect(libraryDropPlan(q, false, { after: "c", inGroup: true })).toEqual({ action: "playNext" });
    expect(libraryDropPlan(q, false, { after: "c", inGroup: false })).toEqual({ insert: { after: "c", in_group: false } });
    expect(libraryDropPlan(view(0, 3), false, { after: "c", inGroup: false })).toEqual({ action: "playNext" });
    expect(libraryDropPlan(view(0, 3), true, { after: "c", inGroup: false })).toEqual({ insert: { after: "c", in_group: false } });
  });

  it("drops anywhere else, past the event's window too, with one InsertTracks", () => {
    // 150 upcoming rows, only 100 of them in the event's window.
    const q = view(2, 148, 100);
    expect(libraryDropPlan(q, true, { after: "r40", inGroup: false })).toEqual({ insert: { after: "r40", in_group: false } });
    expect(libraryDropPlan(q, true, { after: "g0", inGroup: true })).toEqual({ insert: { after: "g0", in_group: true } });
    // The end of the window is not the end of the list.
    expect(libraryDropPlan(q, true, { after: "r97", inGroup: false })).toEqual({ insert: { after: "r97", in_group: false } });
  });
});

describe("insertTracksCommand", () => {
  it("inserts every resolved song at once at a mid-list slot", () => {
    const ids = Array.from({ length: 150 }, (_, i) => `lib${i}`);
    const cmd = insertTracksCommand(view(2, 148, 100), "s1", ids, { after: "r40", in_group: false });
    expect(cmd).toEqual({ type: "insertTracks", data: { server_id: "s1", track_ids: ids, after: "r40", in_group: false } });
  });

  it("falls back to the top when the slot's entry has gone into history, and does nothing without songs", () => {
    // The track changed after the drop: c and g0 are history now.
    const moved: QueueView = { ...view(2, 3), historyCount: 3, currentIndex: 3, groupCount: 1, current: entry("g0", true) };
    expect(insertAnchor(moved, "c")).toBe("g0");
    expect(insertAnchor(moved, "r1")).toBe("r1");
    expect(insertAnchor(moved, "unloaded")).toBe("unloaded");
    expect(insertTracksCommand(moved, "s1", ["x"], { after: "h0", in_group: true })).toEqual({ type: "insertTracks", data: { server_id: "s1", track_ids: ["x"], after: "g0", in_group: true } });
    expect(insertTracksCommand(moved, "s1", [], { after: "r1", in_group: false })).toBeUndefined();
  });
});

describe("previewMoves", () => {
  const entry = (key: string, inGroup = false): QueueEntry => ({
    item: { key, trackId: `t-${key}`, source: { type: "context", data: { index: 0 } } },
    track: { id: `t-${key}`, serverId: "s", title: key, durationMs: 1, rating: 0, loved: false, offline: "none" },
    ...(inGroup ? { inGroup: true } : {}),
  });
  /** h0 | c | g0 g1 (group) | r0 r1 r2, windowed from `start`. */
  const view = (start = 0): QueueView => {
    const all = [entry("h0"), entry("c"), entry("g0", true), entry("g1", true), entry("r0"), entry("r1"), entry("r2")];
    return { ...EMPTY_QUEUE, entries: all.slice(start), windowStart: start, total: all.length, historyCount: 1, currentIndex: 1, groupCount: 2, totalUpcoming: 5, current: all[1] };
  };
  const keys = (q: QueueView) => q.entries.map((e) => e.item.key);

  it("reorders the rest", () => {
    const q = previewMoves(view(), [{ key: "r0", after: "r2", in_group: false }], true);
    expect(keys(q)).toEqual(["h0", "c", "g0", "g1", "r1", "r2", "r0"]);
    expect(q.groupCount).toBe(2);
  });

  it("moves the group's border when a row joins or leaves it", () => {
    const joined = previewMoves(view(), [{ key: "r1", after: "g0", in_group: true }], true);
    expect(keys(joined)).toEqual(["h0", "c", "g0", "r1", "g1", "r0", "r2"]);
    expect(joined.groupCount).toBe(3);
    expect(joined.entries[3]!.inGroup).toBe(true);
    const left = previewMoves(view(), [{ key: "g0", after: "r0", in_group: false }], true);
    expect(keys(left)).toEqual(["h0", "c", "g1", "r0", "g0", "r1", "r2"]);
    expect(left.groupCount).toBe(1);
    expect(left.entries[4]!.inGroup).toBe(false);
  });

  it("applies a block of moves in order, matching placeBlock", () => {
    const order = ["g0", "g1", GROUP_END, "r0", "r1", "r2"];
    const moves = placeBlock(order, ["g0", "g1", GROUP_END, "r1", "r2", "r0"], "r0", ["r0"], "c");
    expect(keys(previewMoves(view(), moves, true))).toEqual(["h0", "c", "g0", "g1", ...apply(["r0", "r1", "r2"], moves)]);
  });

  it("works on a window that starts at the current entry or later in history", () => {
    expect(keys(previewMoves(view(1), [{ key: "r2", after: "c", in_group: true }], true))).toEqual(["c", "r2", "g0", "g1", "r0", "r1"]);
  });

  it("leaves the queue alone with nothing to apply or a window past the current entry", () => {
    const q = view();
    expect(previewMoves(q, [], true)).toBe(q);
    const late = view(3);
    expect(previewMoves(late, [{ key: "r0", after: "r2", in_group: false }], true)).toBe(late);
  });
});
