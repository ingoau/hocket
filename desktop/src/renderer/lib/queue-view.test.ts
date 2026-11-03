import { describe, expect, it } from "vitest";
import type { QueueEntry, QueueView } from "@core/api";
import { EMPTY_QUEUE } from "../store/reducer";
import { mergePages, pagedFits, queueSections, rangeChunks, restLabel, rowMarker, upcomingEntries, wantedRange } from "./queue-view";

const entry = (key: string, extra: Partial<QueueEntry["item"]> = {}, more: Partial<QueueEntry> = {}): QueueEntry => ({
  item: { key, trackId: `t-${key}`, source: { type: "context", data: { index: 0 } }, ...extra },
  track: { id: `t-${key}`, serverId: "s", title: key, durationMs: 1, rating: 0, loved: false, offline: "none" },
  ...more,
});

/** h0 h1 | c | g0 g1 (group) | r0 r1 r2, windowed from `start`. */
function view(start = 0, end = 8): QueueView {
  const all = [entry("h0"), entry("h1", { added: true }), entry("c"), entry("g0", { added: true, queued: true }, { inGroup: true }), entry("g1", { added: true, queued: true }, { inGroup: true }), entry("r0"), entry("r1", { added: true }), entry("r2", { source: { type: "autoplay", data: { provider: "similarSongs", reason: "x" } } }, { autoplay: true })];
  return { ...EMPTY_QUEUE, contextLabel: "Abbey Road", entries: all.slice(start, end), windowStart: start, total: all.length, historyCount: 2, currentIndex: 2, groupCount: 2, totalUpcoming: 5, current: all[2] };
}

describe("queueSections", () => {
  it("splits the window into history, now playing, the group and the rest", () => {
    const s = queueSections(view());
    expect(s.history.map((e) => e.item.key)).toEqual(["h0", "h1"]);
    expect(s.current?.item.key).toBe("c");
    expect(s.group.map((e) => e.item.key)).toEqual(["g0", "g1"]);
    expect(s.rest.map((e) => e.item.key)).toEqual(["r0", "r1", "r2"]);
  });

  it("works on a window that starts after the history", () => {
    const s = queueSections(view(3, 8));
    expect(s.history).toEqual([]);
    expect(s.current?.item.key).toBe("c");
    expect(s.group.map((e) => e.item.key)).toEqual(["g0", "g1"]);
    expect(upcomingEntries(view(3, 8)).map((e) => e.item.key)).toEqual(["g0", "g1", "r0", "r1", "r2"]);
  });
});

describe("rowMarker", () => {
  it("marks added rows outside the group with the queue icon, none inside it, autoplay its own", () => {
    const q = view();
    const s = queueSections(q);
    expect(s.group.map((e) => rowMarker(e, "group"))).toEqual([undefined, undefined]);
    expect(s.rest.map((e) => rowMarker(e, "rest"))).toEqual([undefined, "added", "autoplay"]);
    expect(rowMarker(s.history[1]!, "history")).toBe("added");
  });
});

describe("restLabel", () => {
  it("names the context, or says Later", () => {
    expect(restLabel(view())).toBe("Next from: Abbey Road");
    expect(restLabel({ ...view(), contextLabel: undefined })).toBe("Later");
  });
});

describe("paging", () => {
  it("asks for ranges in chunks the core accepts", () => {
    expect(rangeChunks(0, 450)).toEqual([{ start: 0, count: 200 }, { start: 200, count: 200 }, { start: 400, count: 50 }]);
    expect(rangeChunks(5, 5)).toEqual([]);
    expect(wantedRange(view(3, 6), 200, 1)).toEqual({ start: 0, end: 7 });
  });

  it("merges pages into one window, and refuses pages from a different list", () => {
    const base = view(3, 6);
    const merged = mergePages(base, [view(4, 8), view(0, 4)]);
    expect(merged?.windowStart).toBe(0);
    expect(merged?.entries.map((e) => e.item.key)).toEqual(["h0", "h1", "c", "g0", "g1", "r0", "r1", "r2"]);
    expect(mergePages(base, [{ ...view(0, 8), total: 9 }])).toBeUndefined();
    expect(mergePages(base, [view(0, 2), view(4, 8)])).toBeUndefined();
  });
});

describe("pagedFits", () => {
  const wide = (): QueueView => view(0, 8);
  const base = (): QueueView => view(2, 6);

  it("keeps a wider window for the same list around the same current entry", () => {
    expect(pagedFits(wide(), base())).toBe(true);
  });

  it("drops it after a track change, so drops don't target rows now in history", () => {
    const next = base();
    const advanced: QueueView = { ...next, historyCount: 3, currentIndex: 3, groupCount: 1, current: next.entries[1] };
    expect(pagedFits(wide(), advanced)).toBe(false);
  });

  it("drops it after the context is replaced, even at the same size and position", () => {
    const other = entry("x");
    const replaced: QueueView = { ...base(), current: other, entries: [other, ...base().entries.slice(1)] };
    expect(pagedFits(wide(), replaced)).toBe(false);
    expect(pagedFits(wide(), { ...base(), total: 9 })).toBe(false);
  });
});
