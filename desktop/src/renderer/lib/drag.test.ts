import { describe, expect, it } from "vitest";
import { dragFromControl, dragLabel, insertIndex, parsePayload, targetCount, toDragTarget } from "./drag";
import { playlistAccepts } from "./playlist-drop";
import { parseLibraryTarget } from "./queue-drag";

const ref = (id: string) => ({ source: "s1", id });

describe("parsePayload", () => {
  it("reads a payload with its label and origin", () => {
    const raw = JSON.stringify({ target: { type: "tracks", data: { tracks: [ref("a")] } }, label: "Song", origin: "playlist:p1" });
    expect(parsePayload(raw)).toEqual({ target: { type: "tracks", data: { tracks: [ref("a")] } }, label: "Song", origin: "playlist:p1" });
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
    expect(parsePayload(JSON.stringify({ target: { type: "tracks", data: { tracks: [] } } }))).toBeUndefined();
    expect(parsePayload(JSON.stringify({ target: { type: "tracks", data: { ids: ["a"] } } }))).toBeUndefined();
    expect(parsePayload(JSON.stringify({ target: { type: "genres", data: { ids: ["g"] } } }))).toBeUndefined();
    expect(parsePayload(JSON.stringify({ target: { type: "queueItems", data: { ids: ["k"] } } }))).toBeUndefined();
    // Malformed refs and non-string ids are dropped, not passed on; extra fields are not carried.
    expect(parsePayload(JSON.stringify({ target: { type: "tracks", data: { tracks: [ref("a"), { id: "b" }, { source: "s", id: "" }, { ...ref("c"), x: 1 }] } }, label: 4 })))
      .toEqual({ target: { type: "tracks", data: { tracks: [ref("a"), ref("c")] } } });
    expect(parsePayload(JSON.stringify({ target: { type: "albums", data: { ids: ["a", 3, ""] } } }))).toEqual({ target: { type: "albums", data: { ids: ["a"] } } });
  });

  it("is what the queue's library parse reads too, without queue rows", () => {
    expect(parseLibraryTarget(JSON.stringify({ target: { type: "albums", data: { ids: ["al"] } }, label: "x" }))).toEqual({ type: "albums", data: { ids: ["al"] } });
    expect(parseLibraryTarget(JSON.stringify({ type: "queueItems", data: { keys: ["k"] } }))).toBeUndefined();
  });
});

describe("toDragTarget / targetCount", () => {
  it("counts refs, ids or keys", () => {
    const q = toDragTarget({ type: "queueItems", data: { keys: ["x", "y"] } });
    expect(q && targetCount(q)).toBe(2);
    const t = toDragTarget({ type: "tracks", data: { tracks: [ref("a")] } });
    expect(t && targetCount(t)).toBe(1);
    expect(toDragTarget({ type: "none" })).toBeUndefined();
  });
});

describe("dragLabel", () => {
  it("names one item by its label, several by a count", () => {
    expect(dragLabel({ target: { type: "tracks", data: { tracks: [ref("a")] } }, label: "Hey Jude" })).toBe("Hey Jude");
    expect(dragLabel({ target: { type: "tracks", data: { tracks: [ref("a"), ref("b"), ref("c")] } }, label: "Hey Jude" })).toBe("3 songs");
    expect(dragLabel({ target: { type: "queueItems", data: { keys: ["a", "b"] } } })).toBe("2 songs");
    expect(dragLabel({ target: { type: "albums", data: { ids: ["a"] } } })).toBe("1 album");
    expect(dragLabel({ target: { type: "albums", data: { ids: ["a", "b"] } } })).toBe("2 albums");
    expect(dragLabel({ target: { type: "playlists", data: { ids: ["a", "b"] } } })).toBe("2 playlists");
  });
});

describe("dragFromControl", () => {
  const el = (sel: string) => ({ closest: (s: string) => (s.split(", ").includes(sel) ? hit : null) });
  const hit = {};
  it("lets a control inside the source keep its pointer", () => {
    expect(dragFromControl({ target: el("button") as unknown as EventTarget, currentTarget: {} as EventTarget })).toBe(true);
    expect(dragFromControl({ target: el("[role='radiogroup']") as unknown as EventTarget, currentTarget: {} as EventTarget }, true)).toBe(true);
  });
  it("lets the source itself be a button or link", () => {
    expect(dragFromControl({ target: el("button") as unknown as EventTarget, currentTarget: hit as EventTarget })).toBe(false);
    expect(dragFromControl({ target: el("a") as unknown as EventTarget, currentTarget: {} as EventTarget }, true)).toBe(false);
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
    expect(playlistAccepts({ target: { type: "tracks", data: { tracks: [ref("t")] } } }, "p1", none)).toBe(true);
    expect(playlistAccepts({ target: { type: "albums", data: { ids: ["a"] } } }, "p1", none)).toBe(true);
    expect(playlistAccepts({ target: { type: "queueItems", data: { keys: ["k"] } } }, "p1", none)).toBe(true);
    expect(playlistAccepts({ target: { type: "playlists", data: { ids: ["p2"] } } }, "p1", none)).toBe(true);
  });

  it("refuses artists, itself, its own rows, and read-only playlists", () => {
    expect(playlistAccepts({ target: { type: "artists", data: { ids: ["ar"] } } }, "p1", none)).toBe(false);
    expect(playlistAccepts({ target: { type: "playlists", data: { ids: ["p1"] } } }, "p1", none)).toBe(false);
    expect(playlistAccepts({ target: { type: "tracks", data: { tracks: [ref("t")] } }, origin: "playlist:p1" }, "p1", none)).toBe(false);
    expect(playlistAccepts({ target: { type: "tracks", data: { tracks: [ref("t")] } }, origin: "playlist:p2" }, "p1", none)).toBe(true);
    expect(playlistAccepts({ target: { type: "tracks", data: { tracks: [ref("t")] } } }, "smart", new Set(["smart"]))).toBe(false);
  });
});
