import { describe, expect, it } from "vitest";
import type { Playlist, PlaylistMembership } from "@core/api";
import { canAddTo, clickIntent, keepLayout, layoutPlaylists, pickerGroups, rowState } from "./playlist-picker";

const DAY = 86_400_000;
const pl = (id: string, name: string, changedDaysAgo?: number, extra: Partial<Playlist> = {}): Playlist => ({
  id, serverId: "s", name, public: false, songCount: 10, durationMs: 0, isSmart: false, isMine: true, offline: "none",
  changed: changedDaysAgo === undefined ? undefined : 1_000 * DAY - changedDaysAgo * DAY, ...extra,
});
// The design's example playlists: Late night and Songs for long drives are pinned; Run club and Sunday slow changed last.
const lists = [
  pl("late", "Late night", 9), pl("drives", "Songs for long drives through the night", 8), pl("run", "Run club", 1), pl("sunday", "Sunday slow", 2),
  pl("focus", "Focus", 30), pl("morning", "Morning coffee", 20), pl("road", "Road trip 2024 (the extended summer edition)", 40),
  pl("smart", "Top rated", 0, { isSmart: true }), pl("theirs", "Guest list", 0, { isMine: false }),
];
const pins = [{ kind: "album", id: "al-1", tap: "open" }, { kind: "playlist", id: "drives", tap: "open" }, { kind: "playlist", id: "late", tap: "play" }, { kind: "playlist", id: "smart", tap: "open" }];
const byId = new Map(lists.map((p) => [p.id, p]));

describe("add to playlist groups", () => {
  it("offers only the user's own, non-smart playlists", () => {
    expect(lists.filter(canAddTo).map((p) => p.id)).not.toContain("smart");
    expect(lists.filter(canAddTo).map((p) => p.id)).not.toContain("theirs");
  });

  it("groups Pinned (pin order) / Recent (three most recently changed) / All playlists (by name)", () => {
    const layout = layoutPlaylists(lists, pins, 2);
    expect(layout).toEqual({ pinned: ["drives", "late"], recent: ["run", "sunday"], all: ["focus", "morning", "road"] });
    expect(layoutPlaylists(lists, []).recent).toEqual(["run", "sunday", "drives"]);
    expect(pickerGroups(layout, byId, "").map((g) => `${g.id}:${g.playlists.length}`)).toEqual(["pinned:2", "recent:2", "all:3"]);
  });

  it("searches into one Matches group in the same order, and leaves empty groups out", () => {
    const layout = layoutPlaylists(lists, pins, 2);
    expect(pickerGroups(layout, byId, " NIGHT ")).toEqual([{ id: "matches", playlists: [byId.get("drives"), byId.get("late")] }]);
    expect(pickerGroups(layout, byId, "zzz")).toEqual([]);
    expect(pickerGroups(layoutPlaylists(lists, []), byId, "").map((g) => g.id)).toEqual(["recent", "all"]);
  });

  it("keeps rows still while open: a changed playlist stays put, a new one leads Recent, a gone one leaves", () => {
    const first = layoutPlaylists(lists, pins, 2);
    const touched = lists.map((p) => (p.id === "focus" ? { ...p, changed: 2_000 * DAY } : p)).filter((p) => p.id !== "morning");
    const next = layoutPlaylists([...touched, pl("new", "Rainy days", 0)], pins, 2);
    expect(keepLayout(first, next)).toEqual({ pinned: ["drives", "late"], recent: ["new", "run", "sunday"], all: ["focus", "road"] });
  });
});

describe("add to playlist rows", () => {
  const m = (entries: [string, number][]): PlaylistMembership => ({ playlistId: "p", entries: entries.map(([trackId, index]) => ({ trackId, index })), synced: true });

  it("ticks a playlist holding the song, with a ×2 badge and its copies by index", () => {
    expect(rowState(["a"], m([]))).toMatchObject({ checked: false, copies: 0, missing: ["a"] });
    expect(rowState(["a"], undefined)).toMatchObject({ checked: false });
    expect(rowState(["a"], m([["a", 3]]))).toEqual({ checked: true, copies: 0, indices: [3], lastCopies: [3], missing: [] });
    expect(rowState(["a"], m([["a", 7], ["a", 2]]))).toEqual({ checked: true, copies: 2, indices: [2, 7], lastCopies: [7], missing: [] });
  });

  it("is mixed when it holds some of several songs, and adds only the missing ones", () => {
    const s = rowState(["a", "b", "c"], m([["a", 0], ["b", 4], ["x", 1]]));
    expect(s).toMatchObject({ checked: "mixed", copies: 0, indices: [0, 4], missing: ["c"] });
    expect(clickIntent(s)).toBe("addMissing");
  });

  it("badges several songs only when each is in it more than once; Remove one takes each song's last copy", () => {
    const all = rowState(["a", "b"], m([["a", 0], ["b", 1], ["a", 5], ["b", 6], ["b", 9]]));
    expect(all).toMatchObject({ checked: true, copies: 2, indices: [0, 1, 5, 6, 9], lastCopies: [5, 9] });
    expect(rowState(["a", "b"], m([["a", 0], ["b", 1], ["a", 5]]))).toMatchObject({ checked: true, copies: 0 });
  });

  it("clicks add, remove, or ask about several copies", () => {
    expect(clickIntent(rowState(["a"], m([])))).toBe("add");
    expect(clickIntent(rowState(["a"], m([["a", 1]])))).toBe("remove");
    expect(clickIntent(rowState(["a"], m([["a", 1], ["a", 2]])))).toBe("ask");
  });
});
