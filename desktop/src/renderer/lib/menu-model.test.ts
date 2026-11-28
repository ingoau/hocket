import { describe, expect, it } from "vitest";
import type { MenuItem } from "../store/app";
import { appItemsFor, buildMenu, checkOf, labelFor, layoutActions, loveToast, nextRating, rateToast, ratingOf, removeToast, starLabel, INFO_ACTION, type MenuFacts, type MenuRow } from "./menu-model";
import { d, row, songLayout } from "./menu-model.fixture";

const song: MenuFacts = { kind: "song", count: 1, loved: 1, downloaded: 0, ratings: [4], playlistName: "Late night" };
const shape = (rows: MenuRow[]) => rows.map((r) => (r.kind === "action" ? r.action.id : r.kind === "check" ? `${r.id}:${String(r.checked)}` : r.kind === "item" ? `item:${r.item.id}` : r.kind === "submenu" ? `>${r.id}` : r.kind));

describe("context menu layout", () => {
  it("lays a song's menu out section by section, every row shown, a separator where the section changes", () => {
    expect(shape(buildMenu(songLayout({ playlist: true }), [], song))).toEqual([
      "strip", "sep",
      "rating", "love:true", "sep",
      ">addToPlaylist", "stash", "sep",
      "goToAlbum", "goToArtist", "sep",
      "download:false", ">share", "sep",
      "ui.info", "genius", "sep",
      ">snooze", "sep",
      "removeFromPlaylist",
    ]);
  });

  it("ignores the phone's fold: a folded row is drawn like the rest", () => {
    const folded = songLayout().map((i) => ({ ...i, folded: true }));
    expect(shape(buildMenu(folded, [], song))).toEqual(shape(buildMenu(songLayout(), [], song)));
  });

  it("names rows for the song: Loved with Ctrl+L's pair, Song info, Remove from the playlist", () => {
    const rows = buildMenu(songLayout({ playlist: true }), [], song);
    const love = rows.find((r) => r.kind === "check" && r.id === "love");
    expect(love).toMatchObject({ label: "Loved", runId: "unlove", ids: ["love", "unlove"] });
    expect(rows.find((r) => r.kind === "check" && r.id === "download")).toMatchObject({ label: "Download", runId: "download" });
    expect(rows.find((r) => r.kind === "submenu")).toMatchObject({ label: "Add to playlist" });
    expect(rows.find((r) => r.kind === "genius")).toMatchObject({ label: "Open in Genius" });
    const labels = rows.flatMap((r) => (r.kind === "action" ? [r.label] : []));
    expect(labels).toEqual(["Stash", "goToAlbum", "goToArtist", "Song info", "Remove from Late night"]);
    expect(rows.find((r) => r.kind === "strip")).toMatchObject({ cells: [{ label: "Play" }, { label: "Play next" }, { label: "Play later" }] });
    expect(rows.find((r) => r.kind === "action" && r.action.id === "removeFromPlaylist")).toMatchObject({ destructive: true });
  });

  it("puts Add to queue between Play next and Play later when the core offers it", () => {
    expect(buildMenu(songLayout({ addToQueue: true }), [], song).find((r) => r.kind === "strip")).toMatchObject({ cells: [{ id: "play" }, { id: "playNext" }, { id: "addToQueue", label: "Add to queue", icon: "queue" }, { id: "playLater" }] });
  });

  it("leaves out Love and the stars when the layout has none (the user hid them)", () => {
    const ids = shape(buildMenu(songLayout().filter((i) => i.section !== "rate"), [], song));
    expect(ids.slice(0, 3)).toEqual(["strip", "sep", ">addToPlaylist"]);
  });

  it("shows several songs' mixed states and counts", () => {
    const facts: MenuFacts = { kind: "song", count: 3, loved: 2, downloaded: 0, ratings: [4, 3, 0], playlistName: "Late night" };
    // The core leaves the single-item rows out for several songs.
    const single = new Set(["pinToLibrary", "goToAlbum", "goToArtist", "info", "genius"]);
    const rows = buildMenu(songLayout({ playlist: true }).filter((i) => !single.has(i.id)), [], facts);
    expect(rows.find((r) => r.kind === "rating")).toMatchObject({ mixed: true, value: 0, count: 3 });
    expect(rows.find((r) => r.kind === "check" && r.id === "love")).toMatchObject({ checked: "mixed", label: "Love", note: "2 of 3", runId: "love" });
    expect(rows.find((r) => r.kind === "check" && r.id === "download")).toMatchObject({ checked: false, label: "Download 3 songs" });
    expect(rows.flatMap((r) => (r.kind === "action" ? [r.label] : [])).pop()).toBe("Remove 3 from Late night");
    expect(rows.some((r) => r.kind === "genius")).toBe(false);
  });

  it("falls back to which half of a pair the core offers when the items aren't known", () => {
    const loved = buildMenu([row("love", "rate", [d("unlove", "library")]), row("download", "keep", [d("unpin", "library")])], [], undefined);
    expect(loved).toMatchObject([{ kind: "check", id: "love", checked: true, runId: "unlove" }, { kind: "sep" }, { kind: "check", id: "download", checked: true, label: "Downloaded", runId: "unpin" }]);
    const pinned = buildMenu([row("pinToLibrary", "organise", [d("pinToLibrary", "library", { enabled: false }), d("unpinFromLibrary", "library")])], [], undefined);
    expect(pinned).toMatchObject([{ kind: "check", id: "pin", checked: true, label: "Pinned", runId: "unpinFromLibrary" }]);
  });

  it("draws an album's menu: Go to artist, Album info, no rows the layout doesn't hold", () => {
    const album = [row("play", "play", [d("play", "playback")]), row("playShuffled", "play", [d("playShuffled", "playback")]), row("goToArtist", "goTo", [d("goToArtist", "navigate")]), row("info", "about")];
    const rows = buildMenu(album, [], { kind: "album", count: 1 });
    expect(shape(rows)).toEqual(["strip", "sep", "goToArtist", "sep", "ui.info"]);
    expect(rows.flatMap((r) => (r.kind === "action" ? [r.label] : []))).toEqual(["goToArtist", "Album info"]);
  });

  it("draws a playlist's delete red after a separator", () => {
    const rows = buildMenu([row("play", "play", [d("play", "playback")]), row("deletePlaylist", "danger", [d("deletePlaylist", "library")])], [], { kind: "playlist", count: 1 });
    expect(shape(rows)).toEqual(["strip", "sep", "deletePlaylist"]);
    expect(rows[2]).toMatchObject({ destructive: true });
  });

  it("lets a song with several artists pick one", () => {
    const rows = buildMenu(songLayout(), [], { ...song, artists: [{ id: "a", name: "A" }, { id: "b", name: "B" }] });
    expect(shape(rows)).toContain(">goToArtist");
    expect(shape(buildMenu(songLayout(), [], { ...song, artists: [{ id: "a", name: "A" }] }))).toContain("goToArtist");
  });

  it("offers Unsnooze for a snoozed song instead of the duration submenu", () => {
    const rows = shape(buildMenu(songLayout(), [], { ...song, snoozed: 1, snoozeUntil: null }));
    expect(rows).toContain("unsnooze");
    expect(rows).not.toContain(">snooze");
  });

  it("puts the renderer's items first, a heading and its radios as one choice", () => {
    const item = (id: string, extra: Partial<MenuItem> = {}): MenuItem => ({ id, label: id, icon: "", enabled: true, run: () => {}, ...extra });
    const items = [item("whenClicked", { heading: true, enabled: false }), item("tap-open", { checked: true }), item("tap-play", { checked: false }), item("pinUp"), item("pinDown")];
    const layout = [row("play", "play", [d("play", "playback")]), row("pinToLibrary", "organise", [d("pinToLibrary", "library", { enabled: false }), d("unpinFromLibrary", "library")]), row("info", "about")];
    const rows = buildMenu(layout, items, { kind: "album", count: 1 });
    expect(shape(rows)).toEqual(["choices", "sep", "item:pinUp", "item:pinDown", "sep", "strip", "sep", "pin:true", "sep", "ui.info"]);
    expect(rows[0]).toMatchObject({ kind: "choices", items: [{ id: "tap-open" }, { id: "tap-play" }] });
    // Items alone (a Home section's menu): no separators.
    expect(shape(buildMenu([], [item("a"), item("b")], undefined))).toEqual(["item:a", "item:b"]);
  });

  it("asks the core for the app's own rows the target can have", () => {
    expect(appItemsFor({ type: "tracks", data: { tracks: [{ source: "s", id: "t" }] } })).toEqual(["share", "info", "genius"]);
    expect(appItemsFor({ type: "tracks", data: { tracks: [{ source: "s", id: "t" }, { source: "s", id: "u" }] } })).toEqual(["share"]);
    expect(appItemsFor({ type: "albums", data: { ids: ["a"] } })).toEqual(["share", "info"]);
    expect(appItemsFor({ type: "artists", data: { ids: ["a"] } })).toEqual([]);
    expect(appItemsFor({ type: "playlists", data: { ids: ["p"] } })).toEqual(["share"]);
    expect(appItemsFor({ type: "savedQueue", data: { id: "q" } })).toEqual([]);
    expect(layoutActions(songLayout()).map((a) => a.id)).toContain("unlove");
  });
});

describe("menu states", () => {
  it("reads all, none or some", () => {
    expect(checkOf(3, 3)).toBe(true);
    expect(checkOf(0, 3)).toBe(false);
    expect(checkOf(2, 3)).toBe("mixed");
    expect(checkOf(undefined, 3)).toBeUndefined();
  });

  it("finds the shared rating or a mixed one; clicking the current star clears it", () => {
    expect(ratingOf([4])).toEqual({ value: 4, mixed: false });
    expect(ratingOf([3, 3])).toEqual({ value: 3, mixed: false });
    expect(ratingOf([4, 3, 0])).toEqual({ value: 0, mixed: true });
    expect(ratingOf(undefined)).toEqual({ value: 0, mixed: false });
    expect(nextRating(4, { value: 4, mixed: false })).toBe(0);
    expect(nextRating(2, { value: 4, mixed: false })).toBe(2);
    expect(nextRating(4, { value: 0, mixed: true })).toBe(4);
  });

  it("names stars and toasts in the design's words", () => {
    expect(starLabel(1, 1)).toBe("Rate 1 star");
    expect(starLabel(4, 1)).toBe("Rate 4 stars");
    expect(starLabel(4, 3)).toBe("Rate all 3 songs 4 stars");
    const three: MenuFacts = { kind: "song", count: 3, playlistName: "Late night" };
    expect(loveToast(three, true)).toBe("Loved 3 songs");
    expect(loveToast(three, false)).toBe("Unloved 3 songs");
    expect(loveToast({ kind: "song", count: 1 }, true)).toBeUndefined();
    expect(rateToast(three, 4)).toBe("Rated 3 songs 4 stars");
    expect(rateToast(three, 1)).toBe("Rated 3 songs 1 star");
    expect(rateToast(three, 0)).toBe("Cleared rating on 3 songs");
    expect(removeToast(three)).toBe("Removed 3 songs from Late night");
    expect(removeToast({ kind: "song", count: 1, playlistName: "Late night" })).toBe("Removed from Late night");
    expect(labelFor(d("ui.info", "navigate", { label: "Details" }), { kind: "album", count: 1 })).toBe("Album info");
  });

  it("names the app's info row for its target", () => {
    expect(labelFor(INFO_ACTION, { kind: "song", count: 1 })).toBe("Song info");
    expect(labelFor(INFO_ACTION, { kind: "album", count: 1 })).toBe("Album info");
  });
});
