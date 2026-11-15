import { describe, expect, it } from "vitest";
import { DEFAULT_SECTIONS, HOME_SECTIONS, moveBefore, moveShown, parseSections, sectionDef, sectionsValue, setShelves } from "./home-sections";
import { LIBRARY_PINS_MAX, choosesTap, movePin, parsePins, pinTap, restorePin, shownNeighbour, togglePin, type LibraryPin } from "./library-pins";

describe("home sections", () => {
  // The phone's HomeLibraryLogicTest checks the same list, so the two platforms stay in step.
  it("has the phone's catalogue, defaults and order", () => {
    expect(HOME_SECTIONS.map((s) => `${s.id}:${s.defaultOn ? "on" : "off"}:${s.defaultStyle}`)).toEqual([
      "spotlight:on:hero", "rediscover:on:row", "filters:on:row", "favouriteArtists:on:row", "topGenre:on:grid",
      "moreFromArtist:off:list", "onRepeat:off:row", "recentQueues:off:list", "stats:off:tile",
    ]);
  });

  it("parses keeping unknown ids and appending new sections off; empty is the default", () => {
    expect(parseSections([])).toEqual(DEFAULT_SECTIONS);
    expect(parseSections("junk")).toEqual(DEFAULT_SECTIONS);
    const parsed = parseSections([{ id: "stats", on: true }, { id: "fromTheFuture", on: true, style: "wall" }, { id: "stats", on: false }]);
    // Spotlight (pinned) arrives first and on; every other new section arrives off.
    expect(parsed[0]).toEqual({ id: "spotlight", on: true, style: "hero" });
    expect(parsed[1]).toEqual({ id: "stats", on: true });
    expect(parsed[2]).toEqual({ id: "fromTheFuture", on: true, style: "wall" });
    expect(parsed).toHaveLength(HOME_SECTIONS.length + 1);
    expect(new Set(parsed.slice(3).map((p) => p.on))).toEqual(new Set([false]));
  });

  it("keeps Spotlight first and never moves it", () => {
    expect(parseSections([{ id: "stats", on: true }, { id: "spotlight", on: false }])[0]).toEqual({ id: "spotlight", on: false });
    const prefs = [{ id: "spotlight", on: true }, { id: "rediscover", on: true }, { id: "filters", on: true }];
    expect(moveShown(prefs, "spotlight", 1)).toEqual(prefs);
    expect(moveShown(prefs, "rediscover", -1)).toEqual(prefs);
    expect(moveBefore(prefs, "filters", "spotlight")).toEqual(prefs);
    expect(moveShown(prefs, "rediscover", 1).map((p) => p.id)).toEqual(["spotlight", "filters", "rediscover"]);
  });

  it("lists the core's shelves: Continue listening after Spotlight, the accounts' after the sections that are on", () => {
    setShelves([
      { id: "hocket/continueListening", title: "Continue listening", defaultOn: true },
      { id: "a/recentlyAdded", title: "Recently added", defaultOn: true },
      { id: "a/mostPlayed", title: "Most played", defaultOn: false },
    ]);
    try {
      const parsed = parseSections([]);
      expect(parsed.map((p) => `${p.id}:${p.on ? "on" : "off"}`)).toEqual([
        "spotlight:on", "shelf:hocket/continueListening:on", "rediscover:on", "filters:on", "favouriteArtists:on", "topGenre:on",
        "shelf:a/recentlyAdded:on", "shelf:a/mostPlayed:off", "moreFromArtist:off", "onRepeat:off", "recentQueues:off", "stats:off",
      ]);
      expect(sectionDef("shelf:a/mostPlayed")?.styles).toEqual(["row", "grid", "list"]);
      expect(sectionsValue(parsed)).toEqual([]);
      // A stored list gets a new account's shelves at the end, as they default.
      const stored = parseSections([{ id: "rediscover", on: true }]);
      expect(stored.find((p) => p.id === "shelf:a/recentlyAdded")?.on).toBe(true);
      expect(stored.find((p) => p.id === "shelf:a/mostPlayed")?.on).toBe(false);
      // Hidden and moved like any section.
      const moved = moveShown(parsed, "shelf:a/recentlyAdded", -1).map((p) => p.id);
      expect(moved.indexOf("shelf:a/recentlyAdded")).toBeLessThan(moved.indexOf("topGenre"));
    } finally {
      setShelves([]);
    }
  });

  it("stores the default as [] so later defaults reach it", () => {
    expect(sectionsValue(parseSections([]))).toEqual([]);
    expect(sectionsValue(moveShown(parseSections([]), "rediscover", 1))).not.toEqual([]);
  });

  it("moves among shown sections, skipping hidden and unknown ones' slots", () => {
    const prefs = [{ id: "rediscover", on: true }, { id: "stats", on: false }, { id: "unknown", on: true }, { id: "filters", on: true }];
    expect(moveShown(prefs, "rediscover", 1).map((p) => p.id)).toEqual(["stats", "unknown", "filters", "rediscover"]);
    expect(moveShown(prefs, "rediscover", -1)).toEqual(prefs);
    // Dragging onto a row's lower half places after it, so a section can go last.
    expect(moveBefore(prefs, "rediscover", "filters", true).map((p) => p.id)).toEqual(["stats", "unknown", "filters", "rediscover"]);
  });
});

describe("library pins", () => {
  const pin = (kind: string, id: string, tap = "open"): LibraryPin => ({ kind, id, tap });

  it("keeps kinds and fields this build doesn't know, drops duplicates", () => {
    const pins = parsePins([{ kind: "station", id: "s1", tap: "playNext", serverId: "srv" }, { kind: "album", id: "a1" }, { kind: "album", id: "a1" }]);
    expect(pins).toEqual([{ kind: "station", id: "s1", tap: "playNext", serverId: "srv" }, { kind: "album", id: "a1", tap: "open" }]);
  });

  it("plays or shuffles on click for albums, artists, playlists, genres and filters", () => {
    expect(pinTap(pin("album", "a", "shuffle"))).toBe("shuffle");
    expect(pinTap(pin("genre", "g", "shuffle"))).toBe("shuffle");
    expect(pinTap(pin("filter", "f", "play"))).toBe("play");
    expect(pinTap(pin("playlist", "p", "dance"))).toBe("open");
    expect(pinTap(pin("station", "s", "play"))).toBe("open");
    expect(choosesTap("track")).toBe(false);
  });

  it("refuses to pin past the limit, still unpins", () => {
    const full = Array.from({ length: LIBRARY_PINS_MAX }, (_, i) => pin("album", `a${i}`));
    expect(togglePin(full, "genre", "Folk")).toEqual(full);
    expect(togglePin(full, "album", "a3")).toHaveLength(LIBRARY_PINS_MAX - 1);
    expect(togglePin([], "track", "t1")).toEqual([pin("track", "t1", "play")]);
  });

  it("moves past the next shown pin, leaving hidden ones in place", () => {
    const a = pin("album", "a"), gone = pin("album", "gone"), b = pin("playlist", "b");
    const shown = [a, b];
    expect(movePin([a, gone, b], a, 1, shown)).toEqual([gone, b, a]);
    expect(movePin([a, gone, b], b, -1, shown)).toEqual([b, a, gone]);
    expect(shownNeighbour(shown, a, -1)).toBeUndefined();
  });

  it("undo puts a pin back without undoing later edits", () => {
    const a = pin("album", "a"), b = pin("album", "b"), c = pin("album", "c");
    expect(restorePin([a, c], b, 1)).toEqual([a, b, c]);
  });
});
