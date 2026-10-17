import { describe, expect, it } from "vitest";
import { removePin, restorePin, type LibraryPin } from "./library-pins";
import { cycleTap, draftChanged, draftToSave, moveShownBy, nextTap, placeBeside, tileCorners, tileRadius } from "./pin-editor";

const pin = (kind: string, id: string, tap = "open"): LibraryPin => ({ kind, id, tap });
const ids = (pins: LibraryPin[]) => pins.map((p) => p.id);

describe("pin editor draft", () => {
  const a = pin("album", "a"), b = pin("playlist", "b"), c = pin("artist", "c"), d = pin("genre", "d"), gone = pin("album", "gone");

  it("cycles a click through Opens, Plays, Shuffles", () => {
    expect([nextTap("open"), nextTap("play"), nextTap("shuffle")]).toEqual(["play", "shuffle", "open"]);
    const once = cycleTap([a, d], d);
    expect(once[1]).toEqual(pin("genre", "d", "play"));
    expect(cycleTap(cycleTap(once, once[1]!), pin("genre", "d", "shuffle"))[1]!.tap).toBe("open");
  });

  it("moves along and by rows among the shown pins, clamped, leaving hidden ones in place", () => {
    const draft = [a, gone, b, c, d];
    const shown = [a, b, c, d];
    expect(ids(moveShownBy(draft, a, 1, shown))).toEqual(["gone", "b", "a", "c", "d"]);
    expect(ids(moveShownBy(draft, a, 2, shown))).toEqual(["gone", "b", "c", "a", "d"]);
    expect(ids(moveShownBy(draft, d, -2, shown))).toEqual(["a", "gone", "d", "b", "c"]);
    expect(ids(moveShownBy(draft, c, 9, shown))).toEqual(["a", "gone", "b", "d", "c"]);
    expect(moveShownBy(draft, a, -1, shown)).toEqual(draft);
  });

  it("places a dragged pin before or after the tile it lands on", () => {
    expect(ids(placeBeside([a, b, c], a, c, true))).toEqual(["b", "c", "a"]);
    expect(ids(placeBeside([a, b, c], c, a, false))).toEqual(["c", "a", "b"]);
    expect(placeBeside([a, b], a, a, true)).toEqual([a, b]);
  });

  it("unpins and puts back where it was, keeping later edits", () => {
    const { pins, at } = removePin([a, b, c], b);
    expect(ids(pins)).toEqual(["a", "c"]);
    expect(ids(restorePin(moveShownBy(pins, a, 1), b, at))).toEqual(["c", "b", "a"]);
  });

  it("knows an unchanged draft, including one moved back", () => {
    expect(draftChanged([a, b], [a, b])).toBe(false);
    expect(draftChanged([a, b], [b, a])).toBe(true);
    expect(draftChanged([a, b], [a])).toBe(true);
    expect(draftChanged([a, b], [a, pin("playlist", "b", "play")])).toBe(true);
    expect(draftChanged([a, b], moveShownBy(moveShownBy([a, b], a, 1), a, -1))).toBe(false);
  });

  it("keeps on Done what was pinned elsewhere meanwhile, not what the draft unpinned", () => {
    const added = pin("track", "t", "play");
    expect(ids(draftToSave([a, b, c], [c, a], [a, b, c, added]))).toEqual(["c", "a", "t"]);
  });
});

describe("tile corners", () => {
  const shape = (n: number) => Array.from({ length: n }, (_, i) => {
    const c = tileCorners(i, n);
    return [c.tl, c.tr, c.br, c.bl].map((x) => (x ? "L" : "s")).join("");
  });

  it("rounds the group's outer corners only", () => {
    expect(shape(1)).toEqual(["LLLL"]);
    expect(shape(2)).toEqual(["LssL", "sLLs"]);
    expect(shape(4)).toEqual(["Lsss", "sLss", "sssL", "ssLs"]);
  });

  it("gives a short last row's lone tile and the tile above its gap their outer corners", () => {
    expect(shape(3)).toEqual(["Lsss", "sLLs", "ssLL"]);
    expect(shape(5)).toEqual(["Lsss", "sLss", "ssss", "ssLs", "ssLL"]);
  });

  it("writes them as a border-radius", () => {
    expect(tileRadius(0, 2)).toBe("24px 6px 6px 24px");
    expect(tileRadius(2, 3, 20, 4)).toBe("4px 4px 20px 20px");
  });
});
