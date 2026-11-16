import { describe, expect, it } from "vitest";
import type { QueueItem } from "@core/api";
import * as Q from "./queue";

let n = 0;
const item = (trackId: string, extra: Partial<QueueItem> = {}): QueueItem => ({
  key: `k${++n}`,
  track: { source: "s", id: trackId },
  source: { type: "context", data: { index: 0 } },
  unavailable: false,
  added: false,
  queued: false,
  ordinal: 0,
  ...extra,
});
const fresh = (t: string) => item(t, { source: { type: "inserted" }, added: true });

/** A context of tracks t0..t(count-1), pointer at `ptr`. */
function ctx(count: number, ptr = 0): Q.FlatQueue {
  const q: Q.FlatQueue = { list: Array.from({ length: count }, (_, i) => item(`t${i}`)), ptr, shuffle: false, repeat: "off" };
  Q.renumber(q);
  return q;
}
const tracks = (q: Q.FlatQueue) => q.list.map((i) => i.track.id);
const cur = (q: Q.FlatQueue) => Q.currentItem(q)?.track.id;
const keyOf = (q: Q.FlatQueue, t: string) => q.list.find((i) => i.track.id === t)!.key;

describe("navigation", () => {
  it("next X times then previous X times lands back, and never edits the list", () => {
    const q = ctx(10, 2);
    const before = tracks(q);
    for (let i = 0; i < 5; i++) expect(Q.advance(q).moved).toBe(true);
    expect(cur(q)).toBe("t7");
    for (let i = 0; i < 5; i++) expect(Q.stepBack(q)).toBe(true);
    expect(cur(q)).toBe("t2");
    expect(tracks(q)).toEqual(before);
  });

  it("skips unavailable entries both ways and stops at the ends", () => {
    const q = ctx(4, 0);
    q.list[1]!.unavailable = true;
    Q.advance(q);
    expect(cur(q)).toBe("t2");
    expect(Q.stepBack(q)).toBe(true);
    expect(cur(q)).toBe("t0");
    expect(Q.stepBack(q)).toBe(false);
    q.ptr = 3;
    expect(Q.advance(q).moved).toBe(false);
  });

  it("wraps with repeat all", () => {
    const q = ctx(3, 2);
    q.repeat = "all";
    Q.advance(q);
    expect(cur(q)).toBe("t0");
    Q.stepBack(q);
    expect(cur(q)).toBe("t2");
  });

  it("jump then previous X times returns to where you were", () => {
    const q = ctx(10, 1);
    Q.jump(q, keyOf(q, "t5"));
    expect(cur(q)).toBe("t5");
    for (let i = 0; i < 4; i++) Q.stepBack(q);
    expect(cur(q)).toBe("t1");
  });

  it("trims history to the cap", () => {
    const q = ctx(20, 15);
    Q.trimHistory(q, 10);
    expect(q.ptr).toBe(10);
    expect(cur(q)).toBe("t15");
  });
});

describe("queueing", () => {
  it("play next goes directly after current, add to queue at the group's end, play later at the list's end", () => {
    const q = ctx(5, 1);
    Q.queueItems(q, ["a"], [], "group", true, fresh);
    Q.queueItems(q, ["b"], [], "group", true, fresh);
    Q.queueItems(q, ["n"], [], "next", true, fresh);
    Q.queueItems(q, ["l"], [], "later", true, fresh);
    expect(tracks(q)).toEqual(["t0", "t1", "n", "a", "b", "t2", "t3", "t4", "l"]);
    expect(q.list.map((i) => !!i.queued)).toEqual([false, false, true, true, true, false, false, false, false]);
    expect(q.list.filter((i) => i.added).map((i) => i.track.id)).toEqual(["n", "a", "b", "l"]);
    expect(Q.groupEnd(q) - Q.afterCurrent(q)).toBe(3);
  });

  it("with the queue section off, play next isn't queued unless a group exists", () => {
    const q = ctx(3, 0);
    Q.queueItems(q, ["n"], [], "next", false, fresh);
    expect(q.list[1]!.queued).toBe(false);
    expect(q.list[1]!.added).toBe(true);
    Q.queueItems(q, ["a"], [], "group", true, fresh);
    Q.queueItems(q, ["m"], [], "next", false, fresh);
    // The unqueued "n" isn't in the group, so "a" starts a new one right after the current song.
    expect(tracks(q).slice(0, 4)).toEqual(["t0", "m", "a", "n"]);
    expect(q.list[1]!.queued).toBe(true);
  });

  it("keys ahead of the pointer move; in history or current they are copied", () => {
    const q = ctx(5, 2);
    const k4 = keyOf(q, "t4");
    const k0 = keyOf(q, "t0");
    Q.queueItems(q, [], [k4, k0], "next", true, fresh);
    expect(tracks(q)).toEqual(["t0", "t1", "t2", "t4", "t0", "t3"]);
    expect(q.list[3]!.key).toBe(k4);
    expect(q.list[4]!.key).not.toBe(k0);
    expect(q.list[0]!.key).toBe(k0);
  });

  it("playing a queued song keeps it; previous puts it back at the group's head", () => {
    const q = ctx(3, 0);
    Q.queueItems(q, ["a", "b"], [], "group", true, fresh);
    Q.advance(q);
    expect(cur(q)).toBe("a");
    expect(Q.groupEnd(q) - Q.afterCurrent(q)).toBe(1);
    Q.stepBack(q);
    expect(cur(q)).toBe("t0");
    expect(Q.groupEnd(q) - Q.afterCurrent(q)).toBe(2);
  });
});

describe("move and group", () => {
  it("moves into and out of the group, clamping at its edge", () => {
    const q = ctx(5, 0);
    Q.queueItems(q, ["a", "b"], [], "group", true, fresh);
    // [t0*, a, b, t1, t2, t3, t4]
    expect(Q.moveItem(q, keyOf(q, "t3"), keyOf(q, "t0"), true)).toBe(true);
    expect(tracks(q)).toEqual(["t0", "t3", "a", "b", "t1", "t2", "t4"]);
    expect(q.list[1]!.queued).toBe(true);
    // In-group drop past the group lands at its end.
    expect(Q.moveItem(q, keyOf(q, "t4"), keyOf(q, "t2"), true)).toBe(true);
    expect(tracks(q)).toEqual(["t0", "t3", "a", "b", "t4", "t1", "t2"]);
    // Out-of-group drop inside the group lands just after it.
    expect(Q.moveItem(q, keyOf(q, "a"), keyOf(q, "t0"), false)).toBe(true);
    expect(tracks(q)).toEqual(["t0", "t3", "b", "t4", "a", "t1", "t2"]);
    expect(q.list[4]!.queued).toBe(false);
    expect(Q.groupEnd(q) - Q.afterCurrent(q)).toBe(3);
  });

  it("rejects moves of the current entry, history, or above current", () => {
    const q = ctx(5, 2);
    expect(Q.moveItem(q, keyOf(q, "t2"), keyOf(q, "t3"), false)).toBe(false);
    expect(Q.moveItem(q, keyOf(q, "t0"), keyOf(q, "t3"), false)).toBe(false);
    expect(Q.moveItem(q, keyOf(q, "t4"), keyOf(q, "t1"), false)).toBe(false);
  });

  it("clears the group", () => {
    const q = ctx(3, 0);
    Q.queueItems(q, ["a", "b"], [], "group", true, fresh);
    Q.queueItems(q, ["l"], [], "later", true, fresh);
    expect(Q.clearGroup(q).map((i) => i.track.id)).toEqual(["a", "b"]);
    expect(tracks(q)).toEqual(["t0", "t1", "t2", "l"]);
  });

  it("removing the current entry makes the next playable one current", () => {
    const q = ctx(4, 1);
    q.list[2]!.unavailable = true;
    const r = Q.removeKeys(q, [keyOf(q, "t1"), keyOf(q, "t0")]);
    expect(r.currentRemoved).toBe(true);
    expect(cur(q)).toBe("t3");
  });
});

describe("shuffle", () => {
  it("shuffles only after the group and restores order (rotated from current) when off", () => {
    const q = ctx(20, 5);
    Q.queueItems(q, ["a", "b"], [], "group", true, fresh);
    Q.settle(q);
    Q.setShuffle(q, true, 1234);
    expect(tracks(q).slice(0, 8)).toEqual(["t0", "t1", "t2", "t3", "t4", "t5", "a", "b"]);
    const tail = tracks(q).slice(8);
    expect(tail).not.toEqual(Array.from({ length: 14 }, (_, i) => `t${i + 6}`));
    expect([...tail].sort()).toEqual(Array.from({ length: 14 }, (_, i) => `t${i + 6}`).sort());
    // Added while shuffled: lands after the previous entry's place.
    Q.queueItems(q, ["l"], [], "later", true, fresh);
    Q.setShuffle(q, false, 0);
    Q.settle(q);
    expect(tracks(q)).toEqual(["t0", "t1", "t2", "t3", "t4", "t5", "a", "b", ...Array.from({ length: 14 }, (_, i) => `t${i + 6}`), "l"]);
    expect(q.list.map((i) => i.ordinal)).toEqual(q.list.map((_, i) => i));
  });

  it("unshuffle continues from the current entry's place after jumping", () => {
    const q = ctx(6, 0);
    Q.setShuffle(q, true, 99);
    Q.jump(q, keyOf(q, "t3"));
    Q.setShuffle(q, false, 0);
    // Everything after the pointer, in order from t3's place, wrapping round.
    const after = tracks(q).slice(q.ptr + 1);
    const expected = ["t4", "t5", "t0", "t1", "t2"].filter((t) => after.includes(t));
    expect(after).toEqual(expected);
  });
});

describe("shuffle keeps dormant queued entries in place", () => {
  it("queued entries further down the tail keep their slots through shuffle and unshuffle", () => {
    const q = ctx(12, 0);
    // A group left behind further down (e.g. after going back past it).
    q.list[5]!.queued = true;
    q.list[6]!.queued = true;
    Q.setShuffle(q, true, 4321);
    expect(tracks(q)[5]).toBe("t5");
    expect(tracks(q)[6]).toBe("t6");
    expect(tracks(q)[0]).toBe("t0");
    Q.setShuffle(q, false, 0);
    expect(tracks(q)).toEqual(Array.from({ length: 12 }, (_, i) => `t${i}`));
  });
});

describe("insert at a position", () => {
  /** t0 (history), t1 current, group a b, then t2 t3. */
  function withGroup(): Q.FlatQueue {
    const q = ctx(4, 1);
    Q.queueItems(q, ["a", "b"], [], "group", true, fresh);
    Q.settle(q);
    return q;
  }
  const group = (q: Q.FlatQueue) => q.list.slice(Q.afterCurrent(q), Q.groupEnd(q)).map((i) => i.track.id);

  it("lands directly after the named entry, in or out of the group", () => {
    let q = withGroup();
    expect(Q.insertItems(q, ["n1", "n2"], [], keyOf(q, "a"), true, fresh)).toBe(true);
    expect(tracks(q)).toEqual(["t0", "t1", "a", "n1", "n2", "b", "t2", "t3"]);
    expect(group(q)).toEqual(["a", "n1", "n2", "b"]);
    q = withGroup();
    Q.insertItems(q, ["n"], [], keyOf(q, "t2"), false, fresh);
    expect(tracks(q)).toEqual(["t0", "t1", "a", "b", "t2", "n", "t3"]);
    expect(q.list[5]!.added && !q.list[5]!.queued).toBe(true);
  });

  it("clamps like a drag: in-group past the end lands at its end, out-of-group inside lands after it", () => {
    let q = withGroup();
    Q.insertItems(q, ["n"], [], keyOf(q, "t3"), true, fresh);
    expect(group(q)).toEqual(["a", "b", "n"]);
    q = withGroup();
    Q.insertItems(q, ["n"], [], keyOf(q, "t1"), false, fresh);
    expect(tracks(q)).toEqual(["t0", "t1", "a", "b", "n", "t2", "t3"]);
    expect(group(q)).toEqual(["a", "b"]);
  });

  it("moves keys ahead of the pointer, copies history, and refuses history or unknown anchors", () => {
    const q = withGroup();
    const k3 = keyOf(q, "t3");
    const k0 = keyOf(q, "t0");
    Q.insertItems(q, [], [k3, k0], keyOf(q, "a"), true, fresh);
    expect(tracks(q)).toEqual(["t0", "t1", "a", "t3", "t0", "b", "t2"]);
    expect(keyOf(q, "t3")).toBe(k3);
    expect(q.list[0]!.key).toBe(k0);
    expect(Q.insertItems(q, ["n"], [], k0, false, fresh)).toBe(false);
    expect(Q.insertItems(q, ["n"], [], "nope", false, fresh)).toBe(false);
  });

  it("takes any number of tracks at once", () => {
    const q = withGroup();
    const many = Array.from({ length: 300 }, (_, i) => `m${i}`);
    Q.insertItems(q, many, [], keyOf(q, "t2"), false, fresh);
    expect(q.list.length).toBe(6 + 300);
    expect(tracks(q)[5]).toBe("m0");
    expect(tracks(q)[305]).toBe("t3");
  });
});

describe("limits", () => {
  it("caps the list, history first, then the far end", () => {
    const q = ctx(7000, 4000);
    Q.queueItems(q, Array.from({ length: 4000 }, (_, i) => `x${i}`), [], "later", false, fresh);
    Q.settle(q);
    expect(q.list.length).toBe(Q.MAX_QUEUE_LEN);
    expect(q.ptr).toBe(3000);
    expect(tracks(q)[0]).toBe("t1000");
    const r = ctx(7000, 0);
    Q.queueItems(r, Array.from({ length: 4000 }, (_, i) => `x${i}`), [], "next", false, fresh);
    Q.settle(r);
    expect(r.list.length).toBe(Q.MAX_QUEUE_LEN);
    expect(cur(r)).toBe("t0");
    expect(tracks(r).at(-1)).toBe("t5999");
  });
});

describe("view", () => {
  it("windows around the pointer with exact totals", () => {
    const q = ctx(500, 250);
    Q.queueItems(q, ["a", "b"], [], "group", true, fresh);
    const w = Q.deriveWindow(q, 100, 100);
    expect(w.windowStart).toBe(150);
    expect(w.entries.length).toBe(201);
    expect(w.total).toBe(502);
    expect(w.historyCount).toBe(250);
    expect(w.currentIndex).toBe(250);
    expect(w.groupCount).toBe(2);
    expect(w.totalUpcoming).toBe(251);
    expect(w.entries[100]!.item.track.id).toBe("t250");
    expect(w.entries.filter((e) => e.inGroup).map((e) => e.item.track.id)).toEqual(["a", "b"]);
    expect(w.previous?.track.id).toBe("t249");
  });

  it("queueRange pages rows with the same totals, clamped", () => {
    const q = ctx(50, 10);
    const r = Q.deriveRange(q, 45, 20);
    expect(r.windowStart).toBe(45);
    expect(r.entries.map((e) => e.item.track.id)).toEqual(["t45", "t46", "t47", "t48", "t49"]);
    expect(r.total).toBe(50);
    expect(r.historyCount).toBe(10);
    expect(Q.deriveRange(q, 100, 5).entries).toEqual([]);
  });

  it("wrapNext under repeat all at the end; no current gives no currentIndex", () => {
    const q = ctx(3, 2);
    q.repeat = "all";
    expect(Q.deriveRange(q, 0, 10).wrapNext?.track.id).toBe("t0");
    const empty: Q.FlatQueue = { list: [], ptr: 0, shuffle: false, repeat: "off" };
    expect(Q.deriveWindow(empty, 100, 100).currentIndex).toBeUndefined();
  });
});
