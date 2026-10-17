import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { Genre, Playlist } from "@core/api";
import { ARTIST_ONLY_SORTS, defaultSort, directionLabels, loadListSort, parseListSort, saveListSort, sortGenres, sortOptions, sortPlaylists } from "./list-sort";

const playlist = (id: string, name: string, extra: Partial<Playlist> = {}): Playlist => ({ id, serverId: "s", name, public: false, songCount: 0, durationMs: 0, isSmart: false, isMine: true, offline: "none", ...extra });
const genre = (name: string, songCount: number, albumCount: number): Genre => ({ name, songCount, albumCount });

describe("list sorts", () => {
  it("offers the artist sorts, including the artist-only counts", () => {
    expect(sortOptions("artists").map((o) => o.value)).toEqual(["title", "albumCount", "songCount", "playCount", "dateAdded", "random"]);
    for (const s of ARTIST_ONLY_SORTS) expect(sortOptions("albums").some((o) => o.value === s)).toBe(false);
  });

  it("opens Albums newest first and the other lists A–Z", () => {
    expect(defaultSort("albums")).toEqual({ sort: "dateAdded", descending: true });
    for (const list of ["artists", "playlists", "songs", "genres"] as const) expect(defaultSort(list).descending).toBe(false);
  });

  it("words the directions for the order", () => {
    expect(directionLabels("alpha")).toEqual(["sort.dir.alpha.asc", "sort.dir.alpha.desc"]);
    expect(directionLabels("plain")).toEqual(["sort.dir.asc", "sort.dir.desc"]);
  });

  it("reads a stored sort, falling back per field", () => {
    expect(parseListSort("playlists", { sort: "changed", descending: true })).toEqual({ sort: "changed", descending: true });
    // A sort the list doesn't offer (or junk) keeps the stored direction but the default order.
    expect(parseListSort("playlists", { sort: "albumCount", descending: true })).toEqual({ sort: "name", descending: true });
    expect(parseListSort("albums", { sort: "year", descending: "yes" })).toEqual({ sort: "year", descending: true });
    expect(parseListSort("genres", null)).toEqual(defaultSort("genres"));
    expect(parseListSort("genres", "songCount")).toEqual(defaultSort("genres"));
  });

  it("sorts playlists by name, dates, size and length", () => {
    const list = [
      playlist("1", "beta", { changed: 30, created: 1, songCount: 5, durationMs: 900 }),
      playlist("2", "Alpha", { changed: 10, created: 3, songCount: 9, durationMs: 100 }),
      playlist("3", "gamma", { songCount: 5, durationMs: 500 }),
    ];
    const ids = (s: string, d: boolean) => sortPlaylists(list, s, d).map((p) => p.id);
    expect(ids("name", false)).toEqual(["2", "1", "3"]);
    expect(ids("name", true)).toEqual(["3", "1", "2"]);
    // Missing dates go last either way.
    expect(ids("changed", true)).toEqual(["1", "2", "3"]);
    expect(ids("changed", false)).toEqual(["2", "1", "3"]);
    expect(ids("created", true)).toEqual(["2", "1", "3"]);
    // Ties read A–Z.
    expect(ids("songCount", false)).toEqual(["1", "3", "2"]);
    expect(ids("songCount", true)).toEqual(["2", "1", "3"]);
    expect(ids("duration", true)).toEqual(["1", "3", "2"]);
  });

  it("sorts genres by name and counts", () => {
    const list = [genre("Rock", 10, 2), genre("ambient", 30, 2), genre("Jazz", 20, 5)];
    expect(sortGenres(list, "name", false).map((g) => g.name)).toEqual(["ambient", "Jazz", "Rock"]);
    expect(sortGenres(list, "songCount", true).map((g) => g.name)).toEqual(["ambient", "Jazz", "Rock"]);
    expect(sortGenres(list, "albumCount", false).map((g) => g.name)).toEqual(["ambient", "Rock", "Jazz"]);
  });
});

describe("remembered sorts", () => {
  let store: Map<string, string>;
  beforeEach(() => {
    store = new Map();
    (globalThis as { localStorage?: unknown }).localStorage = {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
    };
  });
  afterEach(() => {
    delete (globalThis as { localStorage?: unknown }).localStorage;
  });

  it("keeps one entry per list", () => {
    saveListSort("artists", { sort: "albumCount", descending: true });
    saveListSort("genres", { sort: "songCount", descending: false });
    expect(loadListSort("artists")).toEqual({ sort: "albumCount", descending: true });
    expect(loadListSort("genres")).toEqual({ sort: "songCount", descending: false });
    expect(loadListSort("playlists")).toEqual(defaultSort("playlists"));
    expect([...store.keys()].sort()).toEqual(["hocket.listSort.artists", "hocket.listSort.genres"]);
  });

  it("reads the old separate Albums and Songs keys until the list saves its own", () => {
    store.set("hocket.albums.sort", JSON.stringify("year"));
    store.set("hocket.albums.desc", JSON.stringify(false));
    expect(loadListSort("albums")).toEqual({ sort: "year", descending: false });
    saveListSort("albums", { sort: "title", descending: true });
    expect(loadListSort("albums")).toEqual({ sort: "title", descending: true });
  });

  it("falls back to the default without storage", () => {
    delete (globalThis as { localStorage?: unknown }).localStorage;
    expect(loadListSort("songs")).toEqual(defaultSort("songs"));
  });
});
