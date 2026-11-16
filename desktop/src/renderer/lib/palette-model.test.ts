import { describe, expect, it } from "vitest";
import type { ActionDescriptor } from "@core/api";
import { parseChord, type Chord } from "../store/shortcuts";
import { buildMenu, type MenuFacts } from "./menu-model";
import {
  actionForChord, ariaShortcut, entryId, filterActions, flatRows, isPaletteChord, keycaps, menuActions, paletteActions, paletteGroups,
  pushRecent, rawId, sanitizeRecents, targetOf, type PaletteAction, type PaletteEntry, type PaletteSources,
} from "./palette-model";

const d = (id: string, category: string, extra: Partial<ActionDescriptor> = {}): ActionDescriptor => ({ id, label: id, icon: "", category, enabled: true, undoable: false, destructive: false, ...extra });
// The registry's context menu for one song, in its order (as in menu-model.test).
const songActions = [
  d("play", "playback"), d("playShuffled", "playback"), d("playNext", "queue"), d("playLater", "queue"), d("addToPlaylist", "library", { label: "Add to playlist…" }),
  d("love", "library", { label: "Love" }), d("unlove", "library", { label: "Unlove" }), d("rate5", "library"), d("rate4", "library"), d("rate3", "library"), d("rate2", "library"), d("rate1", "library"), d("rate0", "library"),
  d("download", "library", { label: "Download" }), d("unpin", "library", { label: "Remove download", destructive: true }), d("goToAlbum", "navigate", { label: "Go to album" }), d("goToArtist", "navigate", { label: "Go to artist" }),
  d("pinToLibrary", "library", { label: "Pin to Library", enabled: false }), d("unpinFromLibrary", "library", { label: "Unpin from Library", enabled: false }),
  d("removeFromPlaylist", "library"), d("ui.newPlaylist", "library"),
];
const songFacts: MenuFacts = { kind: "song", count: 1, loved: 0, downloaded: 0, ratings: [0] };
const e = (kind: PaletteEntry["kind"], id: string, label: string, extra: Partial<PaletteEntry> = {}): PaletteEntry => ({ id: entryId(kind, id), label, kind, ...extra });
const chord = (s: string): Chord => parseChord(s) as Chord;
const keysOf = (groups: PaletteAction[][]) => groups.map((g) => g.map((a) => a.key));

const src = (over: Partial<PaletteSources> = {}): PaletteSources => ({
  recents: [],
  tracks: [e("track", "t1", "Sunrise", { sub: "Band · Album" }), e("track", "t2", "Sundown")],
  albums: [e("album", "a1", "Sunny Days")],
  artists: [e("artist", "r1", "The Suns")],
  playlists: [e("playlist", "p1", "Sunday mix")],
  commands: [e("action", "togglePlay", "Play / pause"), e("action", "navigateSettings", "Settings"), e("action", "shuffle", "Shuffle")],
  ...over,
});

describe("palette groups", () => {
  it("with a query: Songs, Albums, Artists, Playlists, Commands, in that order, empty groups dropped", () => {
    expect(paletteGroups("sun", src()).map((g) => g.id)).toEqual(["track", "album", "artist", "playlist"]);
    const all = paletteGroups("s", src());
    expect(all.map((g) => g.id)).toEqual(["track", "album", "artist", "playlist", "action"]);
    expect(all.find((g) => g.id === "action")?.rows.map((r) => r.item.label)).toEqual(["Shuffle", "Settings", "Play / pause"]);
  });

  it("without a query: recent picks as Suggestions, then every other command", () => {
    const recents = [e("album", "a9", "Old album"), e("action", "shuffle", "Shuffle")];
    const groups = paletteGroups("", src({ recents }));
    expect(groups.map((g) => g.id)).toEqual(["suggestions", "action"]);
    expect(groups[0]?.rows.map((r) => r.item.label)).toEqual(["Old album", "Shuffle"]);
    expect(groups[1]?.rows.map((r) => r.item.label)).toEqual(["Play / pause", "Settings"]);
    expect(flatRows(groups)).toHaveLength(4);
  });

  it("drops a recent command the registry no longer offers", () => {
    const groups = paletteGroups("", src({ recents: [e("action", "gone", "Gone")] }));
    expect(groups.map((g) => g.id)).toEqual(["action"]);
  });

  it("limits each kind while searching", () => {
    const tracks = Array.from({ length: 20 }, (_, i) => e("track", `t${i}`, `Song ${i}`));
    expect(paletteGroups("song", src({ tracks })).find((g) => g.id === "track")?.rows).toHaveLength(6);
  });
});

describe("palette recents", () => {
  it("puts a pick first, once, and keeps at most six", () => {
    let r: PaletteEntry[] = [];
    for (let i = 0; i < 8; i++) r = pushRecent(r, e("track", `t${i}`, `T${i}`));
    r = pushRecent(r, e("track", "t5", "T5"));
    expect(r.map((x) => x.label)).toEqual(["T5", "T7", "T6", "T4", "T3", "T2"]);
  });

  it("reads back only well-formed entries", () => {
    expect(sanitizeRecents("nope")).toEqual([]);
    expect(sanitizeRecents([{ id: "t:1", label: "A", kind: "track" }, { id: 3 }, { id: "x", label: "B", kind: "weird" }, null])).toEqual([{ id: "t:1", label: "A", kind: "track" }]);
  });
});

describe("palette targets", () => {
  it("maps rows to the registry's targets", () => {
    expect(targetOf(e("track", "t1", "x"))).toEqual({ type: "tracks", data: { ids: ["t1"] } });
    expect(targetOf(e("album", "a:1", "x"))).toEqual({ type: "albums", data: { ids: ["a:1"] } });
    expect(targetOf(e("playlist", "p1", "x"))).toEqual({ type: "playlists", data: { ids: ["p1"] } });
    expect(targetOf(e("action", "shuffle", "x"))).toBeUndefined();
    expect(rawId(e("album", "a:1", "x"))).toBe("a:1");
  });
});

describe("palette actions menu", () => {
  const song = e("track", "t1", "Sunrise");
  const songMenu = () => {
    const registry = menuActions(songActions);
    return paletteActions(song, buildMenu(registry, [], songFacts), registry);
  };

  it("follows the context menu: play group, then the toggles and Add to playlist, then navigation; no rating, no inapplicable pin", () => {
    expect(keysOf(songMenu())).toEqual([
      ["play", "playShuffled", "playNext", "playLater"],
      ["love", "download", "addToPlaylist"],
      ["goToAlbum", "goToArtist", "ui.info"],
    ]);
  });

  it("names rows as actions and gives each a chord: Enter plays a song", () => {
    const flat = songMenu().flat();
    const by = (k: string) => flat.find((a) => a.key === k);
    expect(by("play")).toMatchObject({ label: "Play", shortcut: "Enter", run: { type: "action", id: "play" } });
    expect(by("playNext")).toMatchObject({ label: "Play next", shortcut: "Mod+Enter" });
    expect(by("playLater")).toMatchObject({ label: "Add to queue", shortcut: "Mod+Shift+Enter" });
    expect(by("addToPlaylist")).toMatchObject({ label: "Add to playlist…", shortcut: "Mod+P" });
    expect(by("love")).toMatchObject({ label: "Love", shortcut: "Mod+L" });
    expect(by("goToAlbum")).toMatchObject({ shortcut: "Mod+Shift+A" });
  });

  it("runs the half of a toggle that applies: a loved, downloaded song offers Unlove and Remove download", () => {
    const registry = menuActions(songActions);
    const flat = paletteActions(song, buildMenu(registry, [], { ...songFacts, loved: 1, downloaded: 1 }), registry).flat();
    expect(flat.find((a) => a.key === "unlove")).toMatchObject({ label: "Unlove", shortcut: "Mod+L", run: { type: "action", id: "unlove" } });
    expect(flat.find((a) => a.key === "unpin")).toMatchObject({ label: "Remove download", destructive: true });
  });

  it("opens an album on Enter; Play moves to Shift+Enter", () => {
    const album = e("album", "a1", "Sunny Days");
    const registry = menuActions(songActions.filter((a) => a.id !== "goToAlbum"));
    const groups = paletteActions(album, buildMenu(registry, [], { kind: "album", count: 1 }), registry);
    expect(groups[0]?.[0]).toMatchObject({ key: "open", label: "Open album", shortcut: "Enter", run: { type: "open" } });
    expect(groups[0]?.find((a) => a.key === "play")?.shortcut).toBe("Shift+Enter");
  });

  it("gives a command one action, Run", () => {
    expect(paletteActions(e("action", "shuffle", "Shuffle"), [], [])).toEqual([[{ key: "run", label: "Run command", icon: undefined, shortcut: "Enter", run: { type: "command", id: "shuffle" } }]]);
  });

  it("leaves disabled actions out and keeps one row per chord", () => {
    const registry = menuActions(songActions.map((a) => (a.id === "playNext" ? { ...a, enabled: false } : a)));
    const flat = paletteActions(song, buildMenu(registry, [], songFacts), registry).flat();
    expect(flat.some((a) => a.key === "playNext")).toBe(false);
    const chords = flat.flatMap((a) => (a.shortcut ? [a.shortcut] : []));
    expect(new Set(chords).size).toBe(chords.length);
  });

  it("finds the action a chord runs", () => {
    const groups = songMenu();
    expect(actionForChord(groups, chord("Mod+Enter"))?.key).toBe("playNext");
    expect(actionForChord(groups, chord("Mod+L"))?.key).toBe("love");
    expect(actionForChord(groups, chord("Mod+Q"))).toBeUndefined();
  });

  it("knows the palette's chords before a row's actions load, and never claims typing", () => {
    expect(isPaletteChord(chord("Mod+Enter"))).toBe(true);
    expect(isPaletteChord(chord("Shift+Enter"))).toBe(true);
    expect(isPaletteChord(chord("Mod+P"))).toBe(true);
    expect(isPaletteChord(chord("Shift+A"))).toBe(false);
    expect(isPaletteChord(chord("Mod+Z"))).toBe(false);
  });

  it("filters by label, keeping the groups", () => {
    expect(keysOf(filterActions(songMenu(), "play"))).toEqual([["play", "playNext"], ["addToPlaylist"]]);
    expect(filterActions(songMenu(), "zzz")).toEqual([]);
  });
});

describe("keycaps", () => {
  it("draws macOS chords as glyphs in Apple's order", () => {
    expect(keycaps("Mod+Shift+Enter", "macOs")).toEqual(["⇧", "⌘", "↵"]);
    expect(keycaps("Alt+Mod+I", "macOs")).toEqual(["⌥", "⌘", "I"]);
    expect(keycaps("Enter", "macOs")).toEqual(["↵"]);
  });

  it("spells modifiers out elsewhere", () => {
    expect(keycaps("Mod+Shift+Enter", "linux")).toEqual(["Ctrl", "Shift", "↵"]);
    expect(keycaps("Ctrl+K", "windows")).toEqual(["Ctrl", "K"]);
    expect(keycaps("", "linux")).toEqual([]);
  });

  it("names the chord for aria-keyshortcuts", () => {
    expect(ariaShortcut("Mod+Enter", "macOs")).toBe("Meta+Enter");
    expect(ariaShortcut("Mod+Shift+P", "linux")).toBe("Control+Shift+P");
    expect(ariaShortcut(undefined, "linux")).toBeUndefined();
  });
});
