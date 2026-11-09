import { describe, expect, it } from "vitest";
import type { PlayHistoryEntry, Playlist, SavedQueue } from "@core/api";
import { RECENT_ALBUMS, collectLibrary, entryKey, parseLibraryView, viewLibrary, type LibraryEntry } from "./sidebar-library";

const playlist = (id: string, name: string, created?: number, owner = "me"): Playlist =>
  ({ id, serverId: "s", name, owner, public: false, songCount: 3, durationMs: 0, created, changed: created, isSmart: false, isMine: true, offline: "none" }) as unknown as Playlist;
const queue = (kind: "album" | "artist" | "playlist" | "genre", id: string, label: string, at: number): SavedQueue =>
  ({ id: `q-${id}`, context: { serverId: "s", kind: { type: kind, data: { id } }, label, sort: "default" }, label, history: [], upcoming: [], shuffle: false, repeat: "off", positionMs: 0, pinned: false, createdAt: at, lastInteractedAt: at, updatedAt: at, trackCount: 1, coverArt: `c-${id}` }) as unknown as SavedQueue;
const played = (albumId: string, album: string, artistId: string, artist: string, at: number): PlayHistoryEntry =>
  ({ track: { id: `t-${albumId}-${at}`, serverId: "s", title: "t", artist, album, albumId, artistId, durationMs: 1, coverArt: `c-${albumId}`, rating: 0, loved: false, offline: "none" }, playedAt: at, playedMs: 1, scrobbled: false, deviceId: "d" }) as unknown as PlayHistoryEntry;

const keys = (list: LibraryEntry[]) => list.map(entryKey);

describe("collecting the sidebar library", () => {
  it("lists every playlist, and the albums and artists played lately, once each with their latest play", () => {
    const all = collectLibrary(
      [playlist("p1", "Late drive", 100), playlist("p2", "Sunday slow", 50)],
      [queue("album", "a1", "Tidewater", 900), queue("playlist", "p2", "Sunday slow", 700), queue("genre", "Folk", "Folk", 950)],
      [played("a1", "Tidewater", "ar1", "Marlowe Reed", 1000), played("a2", "Copper Hours", "ar2", "The Lindens", 800), played("a1", "Tidewater", "ar1", "Marlowe Reed", 400)],
    );
    expect(keys(all).sort()).toEqual(["album:a1", "album:a2", "artist:ar1", "artist:ar2", "playlist:p1", "playlist:p2"]);
    const tide = all.find((e) => e.id === "a1");
    expect(tide).toMatchObject({ title: "Tidewater", playedAt: 1000, coverArt: "c-a1" });
    expect(all.find((e) => e.id === "a2")?.owner).toBe("The Lindens");
    expect(all.find((e) => e.id === "p2")?.playedAt).toBe(700);
    // A genre queue isn't a library item.
    expect(all.some((e) => e.id === "Folk")).toBe(false);
  });

  it("counts the queue playing now, and names an album from its songs before a queue's label", () => {
    const all = collectLibrary([], [queue("album", "a1", "Road trip", 100)], [played("a1", "Tidewater", "ar1", "Marlowe Reed", 50)], { kind: { type: "album", data: { id: "a2" } }, label: "Copper Hours", coverArt: "c-a2", at: 2000 });
    expect(all.find((e) => e.id === "a1")).toMatchObject({ title: "Tidewater", playedAt: 100 });
    expect(all.find((e) => e.id === "a2")).toMatchObject({ title: "Copper Hours", playedAt: 2000, coverArt: "c-a2" });
  });

  it("skips queues from playlists it doesn't have and songs without an album name", () => {
    const all = collectLibrary([], [queue("playlist", "gone", "Old", 5)], [played("a9", "", "ar9", "Someone", 5)]);
    expect(keys(all)).toEqual(["artist:ar9"]);
  });

  it("caps what the history adds", () => {
    const history = Array.from({ length: RECENT_ALBUMS + 10 }, (_, i) => played(`a${i}`, `Album ${i}`, "ar", "A", 10_000 - i));
    expect(collectLibrary([], [], history).filter((e) => e.kind === "album")).toHaveLength(RECENT_ALBUMS);
  });
});

describe("viewing the sidebar library", () => {
  const entries: LibraryEntry[] = [
    { kind: "playlist", id: "p1", title: "Late drive", owner: "me", addedAt: 100 },
    { kind: "playlist", id: "p2", title: "Sunday slow", owner: "me", addedAt: 300, playedAt: 700 },
    { kind: "album", id: "a1", title: "Tidewater", owner: "Marlowe Reed", playedAt: 1000 },
    { kind: "album", id: "a2", title: "copper Hours", owner: "The Lindens", playedAt: 800 },
    { kind: "artist", id: "ar1", title: "Marlowe Reed", playedAt: 1000 },
  ];

  it("puts the latest played first (ties by name), a never played playlist by when it was added", () => {
    expect(keys(viewLibrary(entries, new Set(), { sort: "recents" }))).toEqual(["artist:ar1", "album:a1", "album:a2", "playlist:p2", "playlist:p1"]);
  });

  it("sorts by when it was added (dated first, the rest by their last play), and by name ignoring case", () => {
    expect(keys(viewLibrary(entries, new Set(), { sort: "added" }))).toEqual(["playlist:p2", "playlist:p1", "artist:ar1", "album:a1", "album:a2"]);
    expect(viewLibrary(entries, new Set(), { sort: "alpha" }).map((e) => e.title)).toEqual(["copper Hours", "Late drive", "Marlowe Reed", "Sunday slow", "Tidewater"]);
  });

  it("leaves out what is pinned", () => {
    expect(keys(viewLibrary(entries, new Set(["album:a1", "playlist:p1"]), { sort: "recents" }))).toEqual(["artist:ar1", "album:a2", "playlist:p2"]);
  });

  it("narrows to one kind and filters by name or owner", () => {
    expect(keys(viewLibrary(entries, new Set(), { kind: "album", sort: "recents" }))).toEqual(["album:a1", "album:a2"]);
    expect(keys(viewLibrary(entries, new Set(), { sort: "recents", query: "marlowe" }))).toEqual(["artist:ar1", "album:a1"]);
    expect(keys(viewLibrary(entries, new Set(), { kind: "playlist", sort: "alpha", query: "SUN" }))).toEqual(["playlist:p2"]);
  });

  it("reads a stored view, dropping what it doesn't know", () => {
    expect(parseLibraryView({ kind: "album", sort: "alpha" })).toEqual({ kind: "album", sort: "alpha" });
    expect(parseLibraryView({ kind: "genre", sort: "loud" })).toEqual({ kind: undefined, sort: "recents" });
    expect(parseLibraryView(null)).toEqual({ kind: undefined, sort: "recents" });
  });
});
