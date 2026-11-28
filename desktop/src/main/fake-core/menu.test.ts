// The fake core's item menus follow the core's (crates/hocket-core/src/actions/menu.rs tests).
import { describe, expect, it } from "vitest";
import type { ActionDescriptor, ActionTarget, MenuScreen } from "@core/api";
import { menuItemsInfo, menuLayout, parseOverrides, withOverride } from "./menu";

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
    expect(m.shown).toEqual(["addToPlaylist", "stash", "goToAlbum", "goToArtist", "download", "share"]);
    expect(m.more).toEqual(["pinToLibrary", "preview", "info", "genius", "snooze"]);
    expect(m.ids).not.toContain("removeFromPlaylist");
  });

  it("drops what the screen doesn't offer", () => {
    const np = layout(tracks(1), "nowPlaying", [...SONG_APPS, "lyricsTiming"], 1);
    expect(np.shown).toEqual(["addToPlaylist", "stash", "goToAlbum", "goToArtist", "share"]);
    expect(np.more).toEqual(["pinToLibrary", "download", "info", "genius", "lyricsTiming", "snooze"]);
    expect(layout(tracks(1), "albumPage", SONG_APPS, 0).ids).not.toContain("goToAlbum");
    expect(layout(tracks(1), "artistPage", SONG_APPS, 0).ids).not.toContain("goToArtist");
    expect(layout(tracks(1), "playlist", SONG_APPS, 0).ids.at(-1)).toBe("removeFromPlaylist");
  });

  it("shows a short menu whole, and leaves single-item rows out for several", () => {
    const several = layout(tracks(2), "list", ["share"], 0);
    expect(several.shown).toEqual(["addToPlaylist", "stash", "download", "share", "snooze"]);
    expect(several.more).toEqual([]);
    expect(layout(album, "list", ["share", "info"], 0)).toMatchObject({ shown: ["addToPlaylist", "stash", "pinToLibrary", "goToArtist", "download", "share", "info"], more: [] });
    expect(layout(album, "list", ["share", "info"], 1)).toMatchObject({ shown: ["addToPlaylist", "stash", "goToArtist", "download", "share"], more: ["pinToLibrary", "info"] });
  });

  it("applies the user's overrides", () => {
    const m = layout(tracks(1), "list", SONG_APPS, 0, JSON.stringify({ genius: "shown", stash: "more", rating: "hidden", share: "hidden" }));
    expect(m.shown).toEqual(["addToPlaylist", "goToAlbum", "goToArtist", "download", "genius"]);
    expect(m.more).toEqual(["stash", "pinToLibrary", "preview", "info", "snooze"]);
    expect(m.ids).not.toContain("rating");
  });

  it("writes overrides as the core does: a default clears, a tier the row can't take is refused", () => {
    const v = withOverride("{}", "genius", "shown");
    expect(JSON.parse(v)).toEqual({ genius: "shown" });
    expect(JSON.parse(withOverride(v, "genius", "more"))).toEqual({});
    expect(() => withOverride(v, "rating", "more")).toThrow();
    expect(() => withOverride(v, "play", "hidden")).toThrow();
    const info = menuItemsInfo(parseOverrides(JSON.stringify({ genius: "shown", nope: "more" })));
    expect(info.some((i) => i.id === "play")).toBe(false);
    expect(info.find((i) => i.id === "genius")).toMatchObject({ defaultTier: "more", tier: "shown", appHandled: true });
    expect(info.find((i) => i.id === "rating")?.choices).toEqual(["shown", "hidden"]);
  });
});
