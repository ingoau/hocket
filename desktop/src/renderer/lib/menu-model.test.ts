import { describe, expect, it } from "vitest";
import type { ActionDescriptor } from "@core/api";
import type { MenuItem } from "../store/app";
import { buildMenu, checkOf, labelFor, loveToast, nextRating, rateToast, ratingOf, removeToast, starLabel, type MenuFacts, type MenuRow } from "./menu-model";

const d = (id: string, category: string, extra: Partial<ActionDescriptor> = {}): ActionDescriptor => ({ id, label: id, icon: "", category, enabled: true, undoable: false, destructive: false, ...extra });
// The registry's context menu for one song, in its order (crates/hocket-core/src/actions/defs.rs CONTEXT_MENU).
const songActions = [
  d("play", "playback"), d("playShuffled", "playback"), d("playNext", "queue"), d("playLater", "queue"), d("addToPlaylist", "library", { label: "Add to playlist…" }),
  d("love", "library"), d("unlove", "library"), d("rate5", "library"), d("rate4", "library"), d("rate3", "library"), d("rate2", "library"), d("rate1", "library"), d("rate0", "library"),
  d("download", "library"), d("unpin", "library", { destructive: true }), d("goToAlbum", "navigate"), d("goToArtist", "navigate"), d("pinToLibrary", "library"),
  d("ui.info", "navigate", { label: "Details" }), d("removeFromPlaylist", "library", { label: "Remove from playlist" }), d("ui.newPlaylist", "library"),
];
const song: MenuFacts = { kind: "song", count: 1, loved: 1, downloaded: 0, ratings: [4], playlistName: "Late night" };
const shape = (rows: MenuRow[]) => rows.map((r) => (r.kind === "action" ? r.action.id : r.kind === "check" ? `${r.id}:${String(r.checked)}` : r.kind === "item" ? `item:${r.item.id}` : r.kind));

describe("context menu layout", () => {
  it("lays a song's menu out as the design: strip, rating, checks, submenu, pin, navigation, remove last", () => {
    expect(shape(buildMenu(songActions, [], song, { inPlaylist: true }))).toEqual([
      "strip", "rating", "love:true", "download:false", "submenu", "pin:false", "sep", "goToAlbum", "goToArtist", "ui.info", "sep", "removeFromPlaylist",
    ]);
  });

  it("names rows for the song: Loved with Ctrl+L's pair, Song info, Remove from the playlist", () => {
    const rows = buildMenu(songActions, [], song, { inPlaylist: true });
    const love = rows.find((r) => r.kind === "check" && r.id === "love");
    expect(love).toMatchObject({ label: "Loved", runId: "unlove", ids: ["love", "unlove"] });
    expect(rows.find((r) => r.kind === "check" && r.id === "download")).toMatchObject({ label: "Download", runId: "download" });
    expect(rows.find((r) => r.kind === "submenu")).toMatchObject({ label: "Add to playlist" });
    const labels = rows.flatMap((r) => (r.kind === "action" ? [r.label] : []));
    expect(labels).toEqual(["goToAlbum", "goToArtist", "Song info", "Remove from Late night"]);
    expect(rows.find((r) => r.kind === "strip")).toMatchObject({ cells: [{ label: "Play" }, { label: "Shuffle" }, { label: "Play next" }, { label: "Play later" }] });
  });

  it("drops Remove from playlist outside a playlist and New playlist beside the submenu", () => {
    const ids = shape(buildMenu(songActions, [], song));
    expect(ids).not.toContain("removeFromPlaylist");
    expect(ids).not.toContain("ui.newPlaylist");
    expect(ids[ids.length - 1]).toBe("ui.info");
  });

  it("shows several songs' mixed states and counts", () => {
    const facts: MenuFacts = { kind: "song", count: 3, loved: 2, downloaded: 0, ratings: [4, 3, 0], playlistName: "Late night" };
    const rows = buildMenu(songActions, [], facts, { inPlaylist: true });
    expect(rows.find((r) => r.kind === "rating")).toMatchObject({ mixed: true, value: 0, count: 3 });
    expect(rows.find((r) => r.kind === "check" && r.id === "love")).toMatchObject({ checked: "mixed", label: "Love", note: "2 of 3", runId: "love" });
    expect(rows.find((r) => r.kind === "check" && r.id === "download")).toMatchObject({ checked: false, label: "Download 3 songs" });
    expect(rows.flatMap((r) => (r.kind === "action" ? [r.label] : [])).pop()).toBe("Remove 3 from Late night");
  });

  it("falls back to which half of a pair the registry offers when the items aren't known", () => {
    const loved = buildMenu([d("unlove", "library"), d("unpin", "library")], [], undefined);
    expect(loved).toMatchObject([{ kind: "check", id: "love", checked: true, runId: "unlove" }, { kind: "check", id: "download", checked: true, label: "Downloaded", runId: "unpin" }]);
    const pinned = buildMenu([d("unpinFromLibrary", "library")], [], undefined);
    expect(pinned).toMatchObject([{ kind: "check", id: "pin", checked: true, label: "Pinned", runId: "unpinFromLibrary" }]);
  });

  it("keeps an album's menu without a rating when the registry offers none, and separates categories", () => {
    const rows = buildMenu([d("play", "playback"), d("playShuffled", "playback"), d("goToArtist", "navigate"), d("ui.renamePlaylist", "library"), d("deletePlaylist", "library", { destructive: true })], [], { kind: "playlist", count: 1 });
    expect(shape(rows)).toEqual(["strip", "sep", "goToArtist", "sep", "ui.renamePlaylist", "sep", "deletePlaylist"]);
  });

  it("puts the renderer's items first, a heading and its radios as one choice", () => {
    const item = (id: string, extra: Partial<MenuItem> = {}): MenuItem => ({ id, label: id, icon: "", enabled: true, run: () => {}, ...extra });
    const items = [item("whenClicked", { heading: true, enabled: false }), item("tap-open", { checked: true }), item("tap-play", { checked: false }), item("pinUp"), item("pinDown")];
    const rows = buildMenu([d("play", "playback"), d("unpinFromLibrary", "library")], items, { kind: "album", count: 1 });
    expect(shape(rows)).toEqual(["choices", "sep", "item:pinUp", "item:pinDown", "sep", "strip", "pin:true", "sep", "ui.info"]);
    expect(rows[0]).toMatchObject({ kind: "choices", items: [{ id: "tap-open" }, { id: "tap-play" }] });
    // Items alone (a Home section's menu): no separators.
    expect(shape(buildMenu([], [item("a"), item("b")], undefined))).toEqual(["item:a", "item:b"]);
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

  it("adds the app's own Song info / Album info after the navigation rows when the registry has none", () => {
    const registry = songActions.filter((a) => a.id !== "ui.info");
    expect(shape(buildMenu(registry, [], song))).toEqual(["strip", "rating", "love:true", "download:false", "submenu", "pin:false", "sep", "goToAlbum", "goToArtist", "ui.info"]);
    const album = buildMenu([d("play", "playback"), d("goToArtist", "navigate"), d("pinToLibrary", "library")], [], { kind: "album", count: 1 });
    expect(album.flatMap((r) => (r.kind === "action" ? [r.label] : []))).toEqual(["goToArtist", "Album info"]);
    // Not for several songs, artists or playlists.
    expect(shape(buildMenu(registry, [], { kind: "song", count: 3 }))).not.toContain("ui.info");
    expect(shape(buildMenu([d("play", "playback")], [], { kind: "artist", count: 1 }))).not.toContain("ui.info");
  });
});
