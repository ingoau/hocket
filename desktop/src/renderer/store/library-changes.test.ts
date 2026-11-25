import { describe, expect, it } from "vitest";
import type { Query } from "@core/api";
import { ALBUM_DEPS, LIBRARY_LOG_CAP, changesSince, emptyLibraryLog, queryLibraryDeps, recordLibraryChange, settledIds, shownIds, showsLibrary, touches, touchesAny, type LibraryChange } from "./library-changes";

const change = (tables: string[], ids: string[] = []): LibraryChange => ({ tables, ids });
const deps = (q: Query) => queryLibraryDeps(q);
const topSongs = deps({ type: "artistTopSongs", data: { id: "ar1", count: 10 } });
const home = deps({ type: "homeShelves" });

describe("library change log", () => {
  it("hands each view the changes since it last looked, or all when they fell out", () => {
    let log = emptyLibraryLog;
    for (let i = 0; i < 3; i++) log = recordLibraryChange(log, change(["tracks"], [`t${i}`]));
    expect(changesSince(log, 3)).toEqual([]);
    expect(changesSince(log, 1)).toEqual([{ seq: 2, tables: ["tracks"], ids: ["t1"] }, { seq: 3, tables: ["tracks"], ids: ["t2"] }]);
    for (let i = 0; i < LIBRARY_LOG_CAP; i++) log = recordLibraryChange(log, change(["tracks"]));
    expect(log.recent).toHaveLength(LIBRARY_LOG_CAP);
    expect(changesSince(log, 1)).toBe("all");
    expect(changesSince(log, log.seq - 1)).toHaveLength(1);
  });
});

describe("touches", () => {
  it("leaves server-backed queries alone on stream-cache and play changes for other tracks (#312)", () => {
    const cached = change(["tracks"], ["t9"]);
    const played = change(["tracks", "play_history"], ["t9"]);
    const shown = () => ["t1", "t2"];
    expect(touches(topSongs, cached, shown)).toBe(false);
    expect(touches(topSongs, played, shown)).toBe(false);
    expect(touches(home, cached, shown)).toBe(false);
    // Home's own shelves follow plays.
    expect(touches(home, played, shown)).toBe(true);
    // A shown row changed (cached, rated): refetch.
    expect(touches(topSongs, change(["tracks"], ["t2"]), shown)).toBe(true);
    // Nothing on screen yet (or it failed): a track's change can't touch it.
    expect(touches(topSongs, cached, () => [])).toBe(false);
  });

  it("matches id-scoped changes to the view's own item", () => {
    expect(touches(topSongs, change(["artistTopSongs"], ["ar1"]))).toBe(true);
    expect(touches(topSongs, change(["artistTopSongs"], ["ar2"]))).toBe(false);
    const playlist = deps({ type: "playlistTracks", data: { id: "p1", page: { offset: 0, limit: 10 } } });
    expect(touches(playlist, change(["playlists", "playlist_tracks"], ["p1"]), () => [])).toBe(true);
    expect(touches(playlist, change(["playlists", "playlist_tracks"], ["p2"]), () => [])).toBe(false);
    const album = deps({ type: "album", data: { id: "al1" } });
    expect(touches(album, change(["tracks", "albums"], ["al1"]))).toBe(true);
    expect(touches(album, change(["tracks", "albums"], ["t1"]))).toBe(false);
    // Downloads finished: tracks with no ids (offline badges).
    expect(touches(album, change(["tracks"]))).toBe(true);
  });

  it("refetches filtered lists on any change to their tables, and everything on a full change", () => {
    const songs = deps({ type: "tracks", data: { server_id: "s", filter: undefined, sort: "title", descending: false, page: { offset: 0, limit: 10 } } } as Query);
    expect(touches(songs, change(["tracks"], ["t9"]))).toBe(true);
    expect(touches(songs, change(["albums"]))).toBe(false);
    expect(touches(songs, change([]))).toBe(true);
    expect(touches(deps({ type: "artistInfo", data: { id: "ar1" } }), change(["artists"], ["ar1"]))).toBe(false);
    expect(touches(deps({ type: "artistInfo", data: { id: "ar1" } }), change([]))).toBe(true);
    // Unknown queries refetch on everything, as before.
    expect(touches(deps({ type: "jobs" }), change(["home"]))).toBe(true);
    expect(touchesAny(songs, "all")).toBe(true);
    expect(touchesAny(songs, [change(["albums"]), change(["genres"])])).toBe(false);
  });

  it("album grids skip track-id changes but follow downloads", () => {
    const albums = () => ["al1", "al2"];
    expect(touches(ALBUM_DEPS, change(["tracks"], ["t1"]), albums)).toBe(false);
    expect(touches(ALBUM_DEPS, change(["tracks"]), albums)).toBe(true);
    expect(touches(ALBUM_DEPS, change(["tracks", "albums"], ["al9"]), albums)).toBe(true);
  });
});

describe("shown ids", () => {
  it("lists the rows of track lists, pages and home shelves", () => {
    expect(shownIds([{ id: "t1", durationMs: 1 }, { id: "t2", durationMs: 1 }])).toEqual(["t1", "t2"]);
    expect(shownIds([{ id: "s1", title: "x", tracks: [{ id: "t1" }], albums: [{ id: "al1" }], playlists: [] }])).toEqual(["t1", "al1"]);
    expect(shownIds({ items: [{ id: "al1" }], total: 1 })).toEqual(["al1"]);
    expect(shownIds({ requestId: "r", tracks: [{ id: "t1" }], albums: [], artists: [], playlists: [] })).toEqual(["t1"]);
    expect(shownIds({ total: 3 })).toBeUndefined();
    expect(shownIds([{ track: { id: "t1" } }])).toBeUndefined();
  });
});

describe("rows still loading", () => {
  const rowDeps = deps({ type: "albumTracks", data: { id: "al1" } });

  it("match any row change, so one that lands during a first load isn't dropped", () => {
    const loaded: string[] = [];
    expect(touchesAny(rowDeps, [change(["tracks"], ["t9"])], () => settledIds(true, () => loaded))).toBe(true);
    expect(touchesAny(rowDeps, [change(["tracks"], ["t9"])], () => settledIds(false, () => loaded))).toBe(false);
    expect(touchesAny(rowDeps, [change(["tracks"], ["t9"])], () => settledIds(false, () => ["t9"]))).toBe(true);
  });

  it("don't ask for the rows on screen until they are settled", () => {
    let asked = 0;
    const ids = () => {
      asked++;
      return ["t1"];
    };
    expect(settledIds(true, ids)).toBeUndefined();
    expect(asked).toBe(0);
    expect(settledIds(false, ids)).toEqual(["t1"]);
  });
});

describe("showing the library", () => {
  it("counts an unfocused fullscreen window Chromium calls hidden (#45) as shown", () => {
    expect(showsLibrary({ pageVisible: false, windowState: { visible: true } })).toBe(true);
    expect(showsLibrary({ pageVisible: true, windowState: { visible: true } })).toBe(true);
    expect(showsLibrary({ pageVisible: false, windowState: { visible: false } })).toBe(false);
  });
});
