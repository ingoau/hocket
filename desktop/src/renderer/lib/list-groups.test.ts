import { describe, expect, it } from "vitest";
import type { PlayHistoryEntry, Playlist, TrackSummary } from "@core/api";
import { GROUP_SIZE, recentFromHistory, recentGenres, recentPlaylists } from "./list-groups";

const played = (id: string, albumId?: string, artistId?: string): PlayHistoryEntry => ({
  track: { id, serverId: "s", title: id, albumId, artistId, durationMs: 1000, rating: 0, loved: false, offline: "none" } as TrackSummary,
  playedAt: 0, playedMs: 1000, scrobbled: true, deviceId: "d",
});
const playlist = (id: string, changed?: number): Playlist => ({ id, serverId: "s", name: id, public: false, songCount: 1, durationMs: 1, changed, isSmart: false, isMine: true, offline: "none" });

describe("Recent groups", () => {
  it("derives albums and artists from the recently played songs, newest first, once each", () => {
    const history = [played("t1", "a1", "r1"), played("t2", "a1", "r2"), played("t3", "a2", "r1"), played("t4", undefined, "r3"), played("t5", "a3")];
    expect(recentFromHistory(history, (t) => t.albumId)).toEqual(["a1", "a2", "a3"]);
    expect(recentFromHistory(history, (t) => t.artistId)).toEqual(["r1", "r2", "r3"]);
  });

  it("leaves out what's pinned and stops at the group size", () => {
    const history = Array.from({ length: 20 }, (_, i) => played(`t${i}`, `a${i}`));
    expect(recentFromHistory(history, (t) => t.albumId, new Set(["a0", "a2"]))).toEqual(["a1", "a3", "a4", "a5", "a6", "a7"]);
    expect(recentFromHistory(history, (t) => t.albumId)).toHaveLength(GROUP_SIZE);
    expect(recentFromHistory(history, (t) => t.albumId, new Set(), 3)).toEqual(["a0", "a1", "a2"]);
  });

  it("lists the most recently changed playlists that aren't pinned", () => {
    const list = [playlist("p1", 5), playlist("p2", 50), playlist("p3"), playlist("p4", 40), playlist("p5", 30)];
    expect(recentPlaylists(list, new Set(["p2"]), 2).map((p) => p.id)).toEqual(["p4", "p5"]);
    expect(recentPlaylists(list, new Set()).map((p) => p.id)).toEqual(["p2", "p4", "p5", "p1"]);
  });

  it("finds the genres of the recently played songs", () => {
    const genreOf = new Map<string, string | undefined>([["t1", "Jazz"], ["t2", "Jazz"], ["t3", " Folk "], ["t4", undefined], ["t5", "Gone"], ["t6", "Pop"]]);
    const known = new Set(["Jazz", "Folk", "Pop"]);
    expect(recentGenres(["t1", "t2", "t3", "t4", "t5", "t6", "t7"], genreOf, known, new Set())).toEqual(["Jazz", "Folk", "Pop"]);
    expect(recentGenres(["t1", "t3", "t6"], genreOf, known, new Set(["Folk"]), 1)).toEqual(["Jazz"]);
  });
});
