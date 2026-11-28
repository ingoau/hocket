// The context menu's layout, from the core's item-menu layout (Query.Menu) and the renderer's own
// items: the renderer's items first, then the play actions as one icon-only strip, then the core's
// rows in its order with a separator wherever its section changes. The six rate actions are an
// inline Rating row; Love / Download / Pin are checkboxes (one row for each pair the core offers);
// Add to playlist, Share, Snooze (and Go to artist for a song with several artists) are submenus;
// the danger section is drawn red. Desktop shows every row: the phone's fold (`folded`) is ignored.
// Pure: the menu component draws and runs what this returns.
import type { ActionDescriptor, ActionTarget, MenuItem as LayoutItem, MenuSection, TrackRef } from "@core/api";
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
  /** The same songs as references (their accounts too). */
  trackRefs?: TrackRef[];
  /** How many of those songs are snoozed. */
  snoozed?: number;
  /** When the one song's snooze ends. Null means it doesn't; absent means it isn't snoozed. */
  snoozeUntil?: number | null;
  /** One song's artists that have a page (Go to artist lets you pick when there are several). */
  artists?: { id: string; name: string }[];
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
  | { kind: "genius"; label: string; enabled: boolean }
  | { kind: "action"; action: ActionDescriptor; label: string; destructive: boolean }
  | { kind: "item"; item: MenuItem }
  | { kind: "choices"; heading: MenuItem; items: MenuItem[] }
  | { kind: "sep" };

/** The play group's look, by id: Play leads and is the filled one. Add to queue is only there
 * when the core offers it (the Queue section setting on). */
const STRIP: Record<string, { label: Parameters<typeof t>[0]; icon: string }> = {
  play: { label: "menu.play", icon: "play" },
  playShuffled: { label: "menu.shuffle", icon: "shuffle" },
  playNext: { label: "menu.playNext", icon: "playNext" },
  addToQueue: { label: "menu.addToQueue", icon: "queue" },
  playLater: { label: "menu.playLater", icon: "playLater" },
};
export const RATE_IDS = ["rate0", "rate1", "rate2", "rate3", "rate4", "rate5"] as const;
/** Pairs of actions that are one checkbox: [off → on, on → off]. */
const PAIRS = { love: ["love", "unlove"], download: ["download", "unpin"], pin: ["pinToLibrary", "unpinFromLibrary"] } as const;
/** The layout's rows a checkbox draws, by the row's id. */
const CHECK_OF: Record<string, keyof typeof PAIRS> = { love: "love", download: "download", pinToLibrary: "pin" };

/** The app's own Song info / Album info row (the core lists `info` with no descriptor). */
export const INFO_ACTION: ActionDescriptor = { id: "ui.info", label: "", icon: "info", category: "navigate", enabled: true, undoable: false, destructive: false };

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

/** Every registry action the layout's rows carry, in menu order: what shortcuts and the palette look up. */
export function layoutActions(layout: readonly LayoutItem[]): ActionDescriptor[] {
  return layout.flatMap((i) => i.actions);
}

/** The app-handled rows the app can offer for a target (Query.Menu's `app_items`): judged from the
 * target alone, so the menu and its facts can be fetched together. */
export function appItemsFor(target: ActionTarget): string[] {
  const shape = targetShape(target);
  if (!shape) return [];
  const out: string[] = [];
  if (shareApplies(shape)) out.push("share");
  if (infoApplies(shape)) out.push("info");
  if (geniusApplies(shape)) out.push("genius");
  return out;
}

/** What a target is and how many items it names, for the app's own rows. */
function targetShape(target: ActionTarget): MenuFacts | undefined {
  switch (target.type) {
    case "tracks": return { kind: "song", count: target.data.tracks.length };
    case "queueItems": return { kind: "song", count: target.data.keys.length };
    case "albums": return { kind: "album", count: target.data.ids.length };
    case "artists": return { kind: "artist", count: target.data.ids.length };
    case "playlists": return { kind: "playlist", count: target.data.ids.length };
    default: return undefined;
  }
}

export function buildMenu(layout: readonly LayoutItem[], items: readonly MenuItem[], facts: MenuFacts | undefined): MenuRow[] {
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

  const count = facts?.count ?? 1;
  const songs = facts?.kind === "song" && count > 1;
  const body: MenuRow[] = [];
  let section: MenuSection | undefined;
  let strip: Extract<MenuRow, { kind: "strip" }> | undefined;

  const check = (id: keyof typeof PAIRS, actions: readonly ActionDescriptor[]): MenuRow | undefined => {
    const [onId, offId] = PAIRS[id];
    // A library pin offers only the half that applies (the other is disabled).
    const usable = id === "pin" ? actions.filter((a) => a.enabled) : actions;
    const on = usable.find((a) => a.id === onId);
    const off = usable.find((a) => a.id === offId);
    if (!on && !off) return undefined;
    // Known from the items themselves; otherwise from which half the core offers.
    const known = id === "love" ? checkOf(facts?.loved, count) : id === "download" ? checkOf(facts?.downloaded, count) : undefined;
    const checked: Check = known ?? (off && !on ? true : false);
    const runId = checked === true ? (off ? offId : onId) : on ? onId : offId;
    const enabled = usable.find((a) => a.id === runId)?.enabled ?? false;
    const label = id === "love"
      ? t(checked === true ? "menu.loved" : "menu.love")
      : id === "download"
        ? (songs ? t(checked === true ? "menu.downloadedSongs" : "menu.downloadSongs", { n: count }) : t(checked === true ? "menu.downloaded" : "menu.download"))
        : t(checked === true ? "menu.pinned" : "menu.pin");
    const note = id === "love" && checked === "mixed" ? t("menu.someOf", { n: facts?.loved ?? 0, total: count }) : undefined;
    return { kind: "check", id, label, checked, note, runId, ids: [onId, offId], enabled };
  };
  const plain = (a: ActionDescriptor, destructive = false): MenuRow => ({ kind: "action", action: a, label: labelFor(a, facts), destructive: destructive || a.destructive });

  /** The rows one layout item draws (none when it has nothing to show here). */
  const rowsFor = (it: LayoutItem): MenuRow[] => {
    const first = it.actions[0];
    if (it.section === "danger") return it.actions.map((a) => plain(a, true));
    const pair = CHECK_OF[it.id];
    if (pair) {
      const r = check(pair, it.actions);
      return r ? [r] : [];
    }
    switch (it.id) {
      case "rating": {
        const r = ratingOf(facts?.ratings);
        return [{ kind: "rating", value: r.value, mixed: r.mixed, count, enabled: it.actions.some((a) => a.enabled) }];
      }
      case "addToPlaylist":
        return first ? [{ kind: "submenu", id: "addToPlaylist", label: t("menu.addToPlaylist"), enabled: first.enabled }] : [];
      case "goToArtist":
        // A song with several artists: pick one, as the player's artist line does.
        if (first && (facts?.artists?.length ?? 0) > 1) return [{ kind: "submenu", id: "goToArtist", label: first.label, enabled: first.enabled }];
        return first ? [plain(first)] : [];
      case "share":
        return facts && shareApplies(facts) ? [{ kind: "submenu", id: "share", label: t("menu.share"), enabled: true }] : [];
      case "info":
        return infoApplies(facts) ? [plain(INFO_ACTION)] : [];
      case "genius":
        return geniusApplies(facts) ? [{ kind: "genius", label: t("menu.openGenius"), enabled: true }] : [];
      case "lyricsTiming":
        return [];
      case "snooze": {
        // A snoozed song offers Unsnooze, and when the snooze ends, instead of the duration submenu.
        if (facts?.kind !== "song") return [];
        const snoozedN = facts.snoozed ?? 0;
        const snooze = it.actions.find((a) => a.id === "snooze");
        const unsnooze = it.actions.find((a) => a.id === "unsnooze");
        const out: MenuRow[] = [];
        if (unsnooze && snoozedN > 0) out.push(plain(unsnooze));
        if (snooze && snoozedN < count) out.push({ kind: "submenu", id: "snooze", label: t("action.snooze"), enabled: snooze.enabled });
        return out;
      }
      default:
        return it.actions.map((a) => plain(a));
    }
  };

  for (const it of layout) {
    if (it.section === "play") {
      const a = it.actions[0];
      const s = STRIP[it.id];
      if (!a || !s) continue;
      if (!strip) {
        if (section !== undefined) body.push({ kind: "sep" });
        section = "play";
        strip = { kind: "strip", cells: [] };
        body.push(strip);
      }
      strip.cells.push({ id: a.id, label: t(s.label), icon: s.icon, enabled: a.enabled });
      continue;
    }
    const r = rowsFor(it);
    if (!r.length) continue;
    if (section !== undefined && section !== it.section) body.push({ kind: "sep" });
    section = it.section;
    body.push(...r);
  }

  if (body.length && rows.length) rows.push({ kind: "sep" });
  rows.push(...body);
  return rows;
}

/** Share belongs on an item menu: a song, album or playlist. An artist has nothing to share. */
export function shareApplies(facts: MenuFacts | undefined): boolean {
  if (!facts || facts.count < 1) return false;
  return facts.kind === "song" || facts.kind === "album" || facts.kind === "playlist";
}

/** Open in Genius searches one song by its title and artist. */
export function geniusApplies(facts: MenuFacts | undefined): boolean {
  return facts?.kind === "song" && facts.count === 1;
}

/** Song info / Album info: for one song or one album. */
export function infoApplies(facts: MenuFacts | undefined): boolean {
  return facts?.count === 1 && (facts.kind === "song" || facts.kind === "album");
}

/** A row's words: the registry's, except where the menu design names it for its target. */
export function labelFor(a: ActionDescriptor, facts: MenuFacts | undefined): string {
  const count = facts?.count ?? 1;
  if (a.id === "unsnooze" && facts?.count === 1) {
    return facts.snoozeUntil == null ? `${a.label} · ${t("snooze.untilNever")}` : `${a.label} · ${t("snooze.until", { when: new Date(facts.snoozeUntil).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }) })}`;
  }
  if (a.id === "ui.info" && facts?.kind === "song") return t("menu.songInfo");
  if (a.id === "ui.info" && facts?.kind === "album") return t("menu.albumInfo");
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
