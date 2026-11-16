import { describe, expect, it } from "vitest";
import { dragLabel, insertIndex, parsePayload, targetIds, toDragTarget } from "./drag";
import { playlistAccepts } from "./playlist-drop";

describe("parsePayload", () => {
  it("reads a payload with its label and origin", () => {
    const raw = JSON.stringify({ target: { type: "tracks", data: { ids: ["a"] } }, label: "Song", origin: "playlist:p1" });
    expect(parsePayload(raw)).toEqual({ target: { type: "tracks", data: { ids: ["a"] } }, label: "Song", origin: "playlist:p1" });
  });

  it("reads queue rows by key", () => {
    expect(parsePayload(JSON.stringify({ target: { type: "queueItems", data: { keys: ["k1", "k2"] } } }))).toEqual({ target: { type: "queueItems", data: { keys: ["k1", "k2"] } } });
  });

  it("reads a bare target (the older wire form)", () => {
    expect(parsePayload(JSON.stringify({ type: "albums", data: { ids: ["al"] } }))).toEqual({ target: { type: "albums", data: { ids: ["al"] } } });
  });

  it("drops junk, empty lists and kinds that don't drag", () => {
    expect(parsePayload("nope")).toBeUndefined();
    expect(parsePayload("null")).toBeUndefined();
    expect(parsePayload(JSON.stringify({ target: { type: "tracks", data: { ids: [] } } }))).toBeUndefined();
    expect(parsePayload(JSON.stringify({ target: { type: "genres", data: { ids: ["g"] } } }))).toBeUndefined();
    expect(parsePayload(JSON.stringify({ target: { type: "queueItems", data: { ids: ["k"] } } }))).toBeUndefined();
    // Non-string ids are dropped, not passed on.
    expect(parsePayload(JSON.stringify({ target: { type: "tracks", data: { ids: ["a", 3, ""] } }, label: 4 }))).toEqual({ target: { type: "tracks", data: { ids: ["a"] } } });
  });
});

describe("toDragTarget / targetIds", () => {
  it("keeps ids or keys", () => {
    const q = toDragTarget({ type: "queueItems", data: { keys: ["x"] } });
    expect(q && targetIds(q)).toEqual(["x"]);
    const p = toDragTarget({ type: "playlists", data: { ids: ["p"] } });
    expect(p && targetIds(p)).toEqual(["p"]);
    expect(toDragTarget({ type: "none" })).toBeUndefined();
  });
});

describe("dragLabel", () => {
  it("names one item by its label, several by a count", () => {
    expect(dragLabel({ target: { type: "tracks", data: { ids: ["a"] } }, label: "Hey Jude" })).toBe("Hey Jude");
    expect(dragLabel({ target: { type: "tracks", data: { ids: ["a", "b", "c"] } }, label: "Hey Jude" })).toBe("3 songs");
    expect(dragLabel({ target: { type: "queueItems", data: { keys: ["a", "b"] } } })).toBe("2 songs");
    expect(dragLabel({ target: { type: "albums", data: { ids: ["a"] } } })).toBe("1 album");
    expect(dragLabel({ target: { type: "albums", data: { ids: ["a", "b"] } } })).toBe("2 albums");
    expect(dragLabel({ target: { type: "playlists", data: { ids: ["a", "b"] } } })).toBe("2 playlists");
  });
});

describe("insertIndex", () => {
  it("is before the row in its top half, after it in its bottom half", () => {
    const r = { top: 100, height: 40 };
    expect(insertIndex(3, 110, r)).toBe(3);
    expect(insertIndex(3, 130, r)).toBe(4);
  });
});

describe("playlistAccepts", () => {
  const none = new Set<string>();
  it("takes songs, albums, queue rows and other playlists", () => {
    expect(playlistAccepts({ target: { type: "tracks", data: { ids: ["t"] } } }, "p1", none)).toBe(true);
    expect(playlistAccepts({ target: { type: "albums", data: { ids: ["a"] } } }, "p1", none)).toBe(true);
    expect(playlistAccepts({ target: { type: "queueItems", data: { keys: ["k"] } } }, "p1", none)).toBe(true);
    expect(playlistAccepts({ target: { type: "playlists", data: { ids: ["p2"] } } }, "p1", none)).toBe(true);
  });

  it("refuses artists, itself, its own rows, and read-only playlists", () => {
    expect(playlistAccepts({ target: { type: "artists", data: { ids: ["ar"] } } }, "p1", none)).toBe(false);
    expect(playlistAccepts({ target: { type: "playlists", data: { ids: ["p1"] } } }, "p1", none)).toBe(false);
    expect(playlistAccepts({ target: { type: "tracks", data: { ids: ["t"] } }, origin: "playlist:p1" }, "p1", none)).toBe(false);
    expect(playlistAccepts({ target: { type: "tracks", data: { ids: ["t"] } }, origin: "playlist:p2" }, "p1", none)).toBe(true);
    expect(playlistAccepts({ target: { type: "tracks", data: { ids: ["t"] } } }, "smart", new Set(["smart"]))).toBe(false);
  });
});
