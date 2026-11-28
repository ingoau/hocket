// The fake core's item menus follow the core's (crates/hocket-core/src/actions/menu.rs tests).
import { describe, expect, it } from "vitest";
import type { ActionDescriptor, ActionTarget, MenuScreen } from "@core/api";
import { menuItemsInfo, menuLayout, overridesValid, parseOverrides, withOverride } from "./menu";

const d = (id: string): ActionDescriptor => ({ id, label: id, icon: "", category: "", enabled: true, undoable: false, destructive: false });
const SONG = ["play", "playNext", "addToQueue", "playLater", "rate5", "rate4", "rate3", "rate2", "rate1", "rate0", "love", "addToPlaylist", "stash", "pinToLibrary", "goToAlbum", "goToArtist", "download", "preview", "snooze", "removeFromPlaylist"].map(d);
const ALBUM = ["play", "playShuffled", "addToPlaylist", "stash", "pinToLibrary", "goToArtist", "download"].map(d);
const tracks = (n: number): ActionTarget => ({ type: "tracks", data: { tracks: Array.from({ length: n }, (_, i) => ({ source: "s", id: `t${i}` })) } });
const album: ActionTarget = { type: "albums", data: { ids: ["al"] } };
const SONG_APPS = ["share", "info", "genius"];

function layout(target: ActionTarget, screen: MenuScreen, appItems: string[], extras: number, overrides = "{}", descriptors = target.type === "albums" ? ALBUM : SONG) {
  const m = menuLayout({ target, screen, appItems, extras, overrides: parseOverrides(overrides), descriptors });
  const list = m.items.filter((i) => !["play", "rate", "danger"].includes(i.section));
  return { shown: list.filter((i) => !i.folded).map((i) => i.id), more: list.filter((i) => i.folded).map((i) => i.id), ids: m.items.map((i) => i.id) };
}

describe("fake core item menus", () => {
  it("keeps a song's Go to rows in the short list and folds the rest", () => {
    const m = layout(tracks(1), "list", SONG_APPS, 0);
    expect(m.shown).toEqual(["addToPlaylist", "goToAlbum", "goToArtist"]);
    expect(m.more).toEqual(["stash", "pinToLibrary", "download", "share", "info"]);
    expect(m.ids).not.toContain("removeFromPlaylist");
    // The stars, Genius, lyrics timing and snooze wait until the user adds them; Preview is the artwork's.
    expect(m.ids.slice(0, 5)).toEqual(["play", "playNext", "addToQueue", "playLater", "love"]);
    for (const hidden of ["rating", "genius", "lyricsTiming", "snooze", "preview"]) expect(m.ids).not.toContain(hidden);
  });

  it("drops what the screen doesn't offer", () => {
    const np = layout(tracks(1), "nowPlaying", [...SONG_APPS, "lyricsTiming"], 1);
    expect(np.shown).toEqual(["addToPlaylist", "goToAlbum", "goToArtist"]);
    expect(np.more).toEqual(["stash", "pinToLibrary", "download", "share", "info"]);
    // Added by the user, the lyrics tools come back.
    const added = layout(tracks(1), "nowPlaying", ["share", "info", "genius", "lyricsTiming"], 1, JSON.stringify({ genius: "more", lyricsTiming: "more" }));
    expect(added.more.slice(-3)).toEqual(["info", "genius", "lyricsTiming"]);
    expect(layout(tracks(1), "albumPage", SONG_APPS, 0).ids).not.toContain("goToAlbum");
    expect(layout(tracks(1), "artistPage", SONG_APPS, 0).ids).not.toContain("goToArtist");
    expect(layout(tracks(1), "playlist", SONG_APPS, 0).ids.at(-1)).toBe("removeFromPlaylist");
  });

  it("shows a short menu whole, and leaves single-item rows out for several", () => {
    const several = layout(tracks(2), "list", ["share"], 0);
    expect(several.shown).toEqual(["addToPlaylist", "stash", "download", "share"]);
    expect(several.more).toEqual([]);
    expect(layout(album, "list", ["share", "info"], 0)).toMatchObject({ shown: ["addToPlaylist", "stash", "pinToLibrary", "goToArtist", "download", "share", "info"], more: [] });
    expect(layout(album, "list", ["share", "info"], 1)).toMatchObject({ shown: ["addToPlaylist", "goToArtist"], more: ["stash", "pinToLibrary", "download", "share", "info"] });
  });

  it("applies the user's overrides", () => {
    const m = layout(tracks(1), "list", SONG_APPS, 0, JSON.stringify({ genius: "shown", stash: "more", rating: "shown", share: "hidden" }));
    expect(m.shown).toEqual(["addToPlaylist", "goToAlbum", "goToArtist", "genius"]);
    expect(m.more).toEqual(["stash", "pinToLibrary", "download", "info"]);
    expect(m.ids).toContain("rating");
    expect(m.ids).not.toContain("share");
  });

  it("keeps a newer device's overrides in the setting and skips them when read", () => {
    expect(overridesValid({ genius: "shown", love: "hidden" })).toBe(true);
    expect(overridesValid([])).toBe(false);
    expect(overridesValid({ genius: "folded" })).toBe(false);
    // A newer device's row (or an older one's, like Preview) is kept and skipped.
    const newer = { nope: "shown", rating: "more", play: "hidden", genius: "shown", preview: "shown" };
    expect(overridesValid(newer)).toBe(true);
    expect([...parseOverrides(JSON.stringify(newer))]).toEqual([["genius", "shown"]]);
    expect(JSON.parse(withOverride(JSON.stringify(newer), "genius", "hidden"))).toEqual({ nope: "shown", rating: "more", play: "hidden", preview: "shown" });
  });

  it("keeps a row the user put under More there, in a short menu and alone", () => {
    const m = layout(tracks(2), "list", ["share"], 0, JSON.stringify({ snooze: "more" }));
    expect(m.shown).toEqual(["addToPlaylist", "stash", "download", "share"]);
    expect(m.more).toEqual(["snooze"]);
  });

  it("folds nothing in a menu of seven rows", () => {
    const m = layout(tracks(1), "list", ["share", "info"], 0, JSON.stringify({ pinToLibrary: "hidden" }));
    expect(m.more).toEqual([]);
    expect(m.shown.at(-1)).toBe("info");
  });

  it("keeps promoted rows and pushes the defaults down", () => {
    const m = layout(tracks(1), "list", SONG_APPS, 2, JSON.stringify({ info: "shown", genius: "shown", snooze: "shown" }));
    // Room is 6 - 2 extras - 3 promoted = 1: Go to artist, the highest default, keeps it.
    expect(m.shown).toEqual(["goToArtist", "info", "genius", "snooze"]);
    expect(m.more).toEqual(["addToPlaylist", "stash", "pinToLibrary", "goToAlbum", "download", "share"]);
  });

  it("writes overrides as the core does: a default clears, a tier the row can't take is refused", () => {
    const v = withOverride("{}", "genius", "shown");
    expect(JSON.parse(v)).toEqual({ genius: "shown" });
    expect(JSON.parse(withOverride(v, "genius", "hidden"))).toEqual({});
    expect(() => withOverride(v, "rating", "more")).toThrow();
    expect(() => withOverride(v, "play", "hidden")).toThrow();
    expect(() => withOverride(v, "preview", "shown")).toThrow();
    const info = menuItemsInfo(parseOverrides(JSON.stringify({ genius: "shown", nope: "more" })));
    expect(info.some((i) => i.id === "play" || i.id === "preview")).toBe(false);
    expect(info.find((i) => i.id === "genius")).toMatchObject({ defaultTier: "hidden", tier: "shown", appHandled: true });
    expect(info.find((i) => i.id === "rating")).toMatchObject({ defaultTier: "hidden", tier: "hidden" });
    expect(info.find((i) => i.id === "rating")?.choices).toEqual(["shown", "hidden"]);
  });
});
