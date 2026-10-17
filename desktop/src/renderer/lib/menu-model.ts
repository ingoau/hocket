// The context menu's layout, from the registry's descriptors and the renderer's own items: a header
// for an item target, the play actions as one icon-only strip, the six rate actions as an inline
// Rating row, Love / Download / Pin as checkboxes (one row for each pair the registry offers), Add
// to playlist as a submenu, then everything else in the registry's order with the destructive item
// last. Pure: the menu component draws and runs what this returns.
import type { ActionDescriptor } from "@core/api";
import { t } from "@shared/strings";
import type { MenuItem } from "../store/app";

export type Check = boolean | "mixed";

/** What the menu knows about its target beyond the descriptors, fetched when it opens (lib/menu-facts). */
export interface MenuFacts {
  /** Songs for tracks and queue items. */
  kind?: "song" | "album" | "artist" | "playlist";
  /** How many items the target names. */
  count: number;
  /** Of those, how many are loved and how many downloaded; undefined when not known. */
  loved?: number;
  downloaded?: number;
  /** One rating per item (0 = none), when known. */
  ratings?: number[];
  /** The playlist the songs are listed in, for "Remove from <playlist>". */
  playlistName?: string;
  header?: MenuHeader;
  /** The songs the target stands for: what the add to playlist submenu adds. */
  trackIds?: string[];
}

export interface MenuHeader {
  /** Cover art ids: one, or up to three stacked for several items. */
  covers: (string | undefined)[];
  title: string;
  subtitle?: string;
  /** An artist: a round picture. */
  round?: boolean;
}

export interface StripCell {
  id: string;
  label: string;
  icon: string;
  enabled: boolean;
}

export type MenuRow =
  | { kind: "strip"; cells: StripCell[] }
  | { kind: "rating"; value: number; mixed: boolean; count: number; enabled: boolean }
  | { kind: "check"; id: "love" | "download" | "pin"; label: string; checked: Check; note?: string; runId: string; ids: string[]; enabled: boolean }
  | { kind: "submenu"; id: string; label: string; enabled: boolean }
  | { kind: "action"; action: ActionDescriptor; label: string; destructive: boolean }
  | { kind: "item"; item: MenuItem }
  | { kind: "choices"; heading: MenuItem; items: MenuItem[] }
  | { kind: "sep" };

/** The play group, in strip order: Play leads and is the filled one. */
export const STRIP_IDS = ["play", "playShuffled", "playNext", "playLater"] as const;
const STRIP: Record<string, { label: Parameters<typeof t>[0]; icon: string }> = {
  play: { label: "menu.play", icon: "play" },
  playShuffled: { label: "menu.shuffle", icon: "shuffle" },
  playNext: { label: "menu.playNext", icon: "playNext" },
  playLater: { label: "menu.playLater", icon: "playLater" },
};
export const RATE_IDS = ["rate0", "rate1", "rate2", "rate3", "rate4", "rate5"] as const;
/** Removing or deleting the item: drawn in the danger colour, last, after a separator. */
const DESTRUCTIVE = new Set(["removeFromQueue", "removeFromPlaylist", "deletePlaylist", "deleteSavedQueue"]);
/** Pairs of actions that are one checkbox: [off → on, on → off]. */
const PAIRS = { love: ["love", "unlove"], download: ["download", "unpin"], pin: ["pinToLibrary", "unpinFromLibrary"] } as const;

/** All of [count], none, or some. */
export function checkOf(n: number | undefined, count: number): Check | undefined {
  if (n === undefined || count <= 0) return undefined;
  return n >= count ? true : n <= 0 ? false : "mixed";
}

/** The shared rating, or mixed when the items differ (unrated counts as 0). */
export function ratingOf(ratings: readonly number[] | undefined): { value: number; mixed: boolean } {
  if (!ratings?.length) return { value: 0, mixed: false };
  const first = ratings[0] ?? 0;
  return ratings.every((r) => r === first) ? { value: first, mixed: false } : { value: 0, mixed: true };
}

/** The rating a click on star [n] sets: clicking the current rating clears it. */
export function nextRating(n: number, current: { value: number; mixed: boolean }): number {
  return !current.mixed && current.value === n ? 0 : n;
}

export function buildMenu(actions: readonly ActionDescriptor[], items: readonly MenuItem[], facts: MenuFacts | undefined, opts: { inPlaylist?: boolean } = {}): MenuRow[] {
  const byId = new Map(actions.map((a) => [a.id, a]));
  const used = new Set<string>();
  const rows: MenuRow[] = [];

  // The renderer's own items first, as before: a heading and the radios under it are one labelled choice.
  for (const item of items) {
    const last = rows[rows.length - 1];
    if (item.heading) {
      if (rows.length) rows.push({ kind: "sep" });
      rows.push({ kind: "choices", heading: item, items: [] });
    } else if (last?.kind === "choices" && item.checked !== undefined) last.items.push(item);
    else {
      // What follows a choice is set apart from it.
      if (last?.kind === "choices") rows.push({ kind: "sep" });
      rows.push({ kind: "item", item });
    }
  }
  const top: MenuRow[] = [];

  const cells = STRIP_IDS.flatMap((id) => {
    const a = byId.get(id);
    if (!a) return [];
    used.add(id);
    const s = STRIP[id] as { label: Parameters<typeof t>[0]; icon: string };
    return [{ id, label: t(s.label), icon: s.icon, enabled: a.enabled }];
  });
  if (cells.length) top.push({ kind: "strip", cells });

  const rates = RATE_IDS.filter((id) => byId.has(id));
  if (rates.length) {
    rates.forEach((id) => used.add(id));
    const r = ratingOf(facts?.ratings);
    top.push({ kind: "rating", value: r.value, mixed: r.mixed, count: facts?.count ?? 1, enabled: rates.some((id) => byId.get(id)?.enabled) });
  }

  const count = facts?.count ?? 1;
  const songs = facts?.kind === "song" && count > 1;
  const check = (id: keyof typeof PAIRS, known: Check | undefined, label: (on: Check) => string, note?: (on: Check) => string | undefined) => {
    const [onId, offId] = PAIRS[id];
    const on = byId.get(onId);
    const off = byId.get(offId);
    if (!on && !off) return;
    used.add(onId);
    used.add(offId);
    // Known from the items themselves; otherwise from which half the registry offers.
    const checked: Check = known ?? (off && !on ? true : false);
    const runId = checked === true ? (off ? offId : onId) : on ? onId : offId;
    const enabled = byId.get(runId)?.enabled ?? false;
    top.push({ kind: "check", id, label: label(checked), checked, note: note?.(checked), runId, ids: [onId, offId], enabled });
  };
  check("love", checkOf(facts?.loved, count), (on) => t(on === true ? "menu.loved" : "menu.love"), (on) => (on === "mixed" ? t("menu.someOf", { n: facts?.loved ?? 0, total: count }) : undefined));
  check("download", checkOf(facts?.downloaded, count), (on) => (songs ? t(on === true ? "menu.downloadedSongs" : "menu.downloadSongs", { n: count }) : t(on === true ? "menu.downloaded" : "menu.download")));

  const add = byId.get("addToPlaylist");
  if (add) {
    used.add("addToPlaylist");
    // The submenu has its own New playlist.
    used.add("ui.newPlaylist");
    top.push({ kind: "submenu", id: "addToPlaylist", label: t("menu.addToPlaylist"), enabled: add.enabled });
  }
  check("pin", undefined, (on) => t(on === true ? "menu.pinned" : "menu.pin"));

  // Remove from playlist only where the songs are listed in one.
  if (!opts.inPlaylist) used.add("removeFromPlaylist");
  const rest = actions.filter((a) => !used.has(a.id));
  const plain = rest.filter((a) => !DESTRUCTIVE.has(a.id));
  const danger = rest.filter((a) => DESTRUCTIVE.has(a.id));

  if (top.length && rows.length) rows.push({ kind: "sep" });
  rows.push(...top);
  let category: string | undefined = top.length ? "top" : rows.length ? "items" : undefined;
  for (const a of plain) {
    if (category !== undefined && category !== a.category) rows.push({ kind: "sep" });
    category = a.category;
    rows.push({ kind: "action", action: a, label: labelFor(a, facts), destructive: a.destructive });
  }
  if (danger.length && rows.length) rows.push({ kind: "sep" });
  for (const a of danger) rows.push({ kind: "action", action: a, label: labelFor(a, facts), destructive: true });
  return rows;
}

/** A row's words: the registry's, except where the menu design names it for its target. */
export function labelFor(a: ActionDescriptor, facts: MenuFacts | undefined): string {
  const count = facts?.count ?? 1;
  if (a.id === "ui.info" && facts?.kind === "song") return t("menu.songInfo");
  if (a.id === "removeFromPlaylist" && facts?.playlistName) return count > 1 ? t("menu.removeSomeFrom", { n: count, playlist: facts.playlistName }) : t("menu.removeFrom", { playlist: facts.playlistName });
  return a.label;
}

/** A star's accessible name: "Rate 4 stars", or "Rate all 3 songs 4 stars" for several. */
export function starLabel(n: number, count: number): string {
  if (count > 1) return n === 1 ? t("menu.rateAllOne", { n: count }) : t("menu.rateAllMany", { n: count, r: n });
  return n === 1 ? t("menu.rateOne") : t("menu.rateMany", { r: n });
}

/** The toast after loving or rating several songs; a single song's change shows in its row. */
export function loveToast(facts: MenuFacts | undefined, loving: boolean): string | undefined {
  if (facts?.kind !== "song" || facts.count < 2) return undefined;
  return t(loving ? "toast.lovedSongs" : "toast.unlovedSongs", { n: facts.count });
}

export function rateToast(facts: MenuFacts | undefined, rating: number): string | undefined {
  if (facts?.kind !== "song" || facts.count < 2) return undefined;
  if (!rating) return t("toast.clearedRating", { n: facts.count });
  return rating === 1 ? t("toast.ratedSongsOne", { n: facts.count }) : t("toast.ratedSongs", { n: facts.count, r: rating });
}

export function removeToast(facts: MenuFacts | undefined): string | undefined {
  if (!facts?.playlistName) return undefined;
  return facts.count > 1 ? t("toast.removedSongsFrom", { n: facts.count, playlist: facts.playlistName }) : t("toast.removedFrom", { playlist: facts.playlistName });
}
