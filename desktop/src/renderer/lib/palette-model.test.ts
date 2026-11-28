import { describe, expect, it } from "vitest";
import { parseChord, type Chord } from "../store/shortcuts";
import { buildMenu, layoutActions, type MenuFacts } from "./menu-model";
import { songLayout } from "./menu-model.fixture";
import {
  actionForChord, ariaShortcut, entryId, filterActions, flatRows, isPaletteChord, keycaps, paletteActions, paletteGroups,
  pushRecent, rawId, sanitizeRecents, targetOf, type PaletteAction, type PaletteEntry, type PaletteSources,
} from "./palette-model";

// The core's layout for one song (as in menu-model.test), with the registry's labels.
const songItems = songLayout({ labels: true });
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

  it("lists lyric hits under Songs, unranked, leaving out songs already listed", () => {
    const hit = (id: string, title: string) => ({ ...e("track", id, title), id: `ly:${id}`, lyric: { line: "the sun is up", start: 4, end: 7 } });
    const groups = paletteGroups("sun", src({ lyrics: [hit("t1", "Sunrise"), hit("t8", "Morning")] }));
    expect(groups.map((g) => g.id).slice(0, 2)).toEqual(["track", "lyrics"]);
    expect(groups[1]?.rows.map((r) => r.item.label)).toEqual(["Morning"]);
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

  it("remembers a lyric hit as its song", () => {
    const hit = { ...e("track", "t1", "Sunrise", { sub: "the sun", ref: { source: "s", id: "t1" } }), id: "ly:t1", lyric: { line: "the sun", start: 4, end: 7 } };
    expect(pushRecent([], hit)).toEqual([{ id: "t:t1", label: "Sunrise", kind: "track", sub: undefined, coverArt: undefined, icon: undefined, ref: { source: "s", id: "t1" } }]);
  });

  it("reads back only well-formed entries", () => {
    expect(sanitizeRecents("nope")).toEqual([]);
    const ok = { id: "t:1", label: "A", kind: "track", ref: { source: "s", id: "1" } };
    expect(sanitizeRecents([ok, { id: "t:2", label: "No ref", kind: "track" }, { id: 3 }, { id: "x", label: "B", kind: "weird" }, null])).toEqual([ok]);
  });
});

describe("palette targets", () => {
  it("maps rows to the registry's targets", () => {
    expect(targetOf(e("track", "t1", "x", { ref: { source: "nd1", id: "t1" } }))).toEqual({ type: "tracks", data: { tracks: [{ source: "nd1", id: "t1" }] } });
    expect(targetOf(e("track", "t1", "x"))).toBeUndefined();
    expect(targetOf(e("album", "a:1", "x"))).toEqual({ type: "albums", data: { ids: ["a:1"] } });
    expect(targetOf(e("playlist", "p1", "x"))).toEqual({ type: "playlists", data: { ids: ["p1"] } });
    expect(targetOf(e("action", "shuffle", "x"))).toBeUndefined();
    expect(rawId(e("album", "a:1", "x"))).toBe("a:1");
  });
});

describe("palette actions menu", () => {
  const song = e("track", "t1", "Sunrise", { ref: { source: "nd1", id: "t1" } });
  const menuOf = (layout: ReturnType<typeof songLayout>, facts: MenuFacts, entry = song) => paletteActions(entry, buildMenu(layout, [], facts), layoutActions(layout));
  const songMenu = () => menuOf(songItems, songFacts);

  it("follows the context menu's sections: play group, Love, organising, navigation, keeping, about; no rating, no inapplicable pin", () => {
    expect(keysOf(songMenu())).toEqual([
      ["play", "playNext", "playLater"],
      ["love"],
      ["addToPlaylist", "stash"],
      ["goToAlbum", "goToArtist"],
      ["download"],
      ["preview", "ui.info"],
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
    const flat = menuOf(songItems, { ...songFacts, loved: 1, downloaded: 1 }).flat();
    expect(flat.find((a) => a.key === "unlove")).toMatchObject({ label: "Unlove", shortcut: "Mod+L", run: { type: "action", id: "unlove" } });
    expect(flat.find((a) => a.key === "unpin")).toMatchObject({ label: "Remove download", destructive: true });
  });

  it("opens an album on Enter; Play moves to Shift+Enter", () => {
    const album = e("album", "a1", "Sunny Days");
    const groups = menuOf(songItems.filter((i) => i.id !== "goToAlbum"), { kind: "album", count: 1 }, album);
    expect(groups[0]?.[0]).toMatchObject({ key: "open", label: "Open album", shortcut: "Enter", run: { type: "open" } });
    expect(groups[0]?.find((a) => a.key === "play")?.shortcut).toBe("Shift+Enter");
  });

  it("gives a command one action, Run", () => {
    expect(paletteActions(e("action", "shuffle", "Shuffle"), [], [])).toEqual([[{ key: "run", label: "Run command", icon: undefined, shortcut: "Enter", run: { type: "command", id: "shuffle" } }]]);
  });

  it("with the core's own Add to queue, Play later keeps its name and gives up the chord", () => {
    const flat = menuOf(songLayout({ labels: true, addToQueue: true }), songFacts).flat();
    expect(flat.find((a) => a.key === "addToQueue")).toMatchObject({ shortcut: "Mod+Shift+Enter" });
    expect(flat.find((a) => a.key === "playLater")).toMatchObject({ label: "Play later", shortcut: undefined });
  });

  it("leaves disabled actions out and keeps one row per chord", () => {
    const layout = songItems.map((i) => (i.id === "playNext" ? { ...i, actions: i.actions.map((a) => ({ ...a, enabled: false })) } : i));
    const flat = menuOf(layout, songFacts).flat();
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
