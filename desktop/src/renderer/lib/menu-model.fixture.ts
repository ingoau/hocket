// Test fixtures: item-menu layouts as the core's Query.Menu returns them
// (crates/hocket-core/src/actions/menu.rs), for the menu and palette model tests.
import type { ActionDescriptor, MenuItem as LayoutItem, MenuSection } from "@core/api";

export const d = (id: string, category: string, extra: Partial<ActionDescriptor> = {}): ActionDescriptor => ({ id, label: id, icon: "", category, enabled: true, undoable: false, destructive: false, ...extra });

export const row = (id: string, section: MenuSection, actions: ActionDescriptor[] = [], folded = false): LayoutItem => ({ id, section, folded, actions });

const rates = () => [5, 4, 3, 2, 1, 0].map((n) => d(`rate${n}`, "library"));

/** One song in a list with Share, info and Genius offered: the core's layout. */
export function songLayout(opts: { playlist?: boolean; addToQueue?: boolean; labels?: boolean } = {}): LayoutItem[] {
  const l = (label: string) => (opts.labels ? { label } : {});
  return [
    row("play", "play", [d("play", "playback")]),
    row("playNext", "play", [d("playNext", "queue")]),
    ...(opts.addToQueue ? [row("addToQueue", "play", [d("addToQueue", "queue", { label: "Add to queue" })])] : []),
    row("playLater", "play", [d("playLater", "queue")]),
    row("rating", "rate", rates()),
    row("love", "rate", [d("love", "library", l("Love")), d("unlove", "library", l("Unlove"))]),
    row("addToPlaylist", "organise", [d("addToPlaylist", "library", { label: "Add to playlist…" })]),
    row("stash", "organise", [d("stash", "queue", { label: "Stash" })]),
    row("pinToLibrary", "organise", [d("pinToLibrary", "library", { label: "Pin to Library", enabled: false }), d("unpinFromLibrary", "library", { label: "Unpin from Library", enabled: false })], true),
    row("goToAlbum", "goTo", [d("goToAlbum", "navigate", l("Go to album"))]),
    row("goToArtist", "goTo", [d("goToArtist", "navigate", l("Go to artist"))]),
    row("download", "keep", [d("download", "library", l("Download")), d("unpin", "library", { label: opts.labels ? "Remove download" : "unpin", destructive: true })]),
    row("share", "keep"),
    row("info", "about", [], true),
    row("genius", "about", [], true),
    row("snooze", "tune", [d("snooze", "queue"), d("unsnooze", "queue", { label: "Unsnooze" })], true),
    ...(opts.playlist ? [row("removeFromPlaylist", "danger", [d("removeFromPlaylist", "library", { label: "Remove from playlist" })])] : []),
  ];
}
