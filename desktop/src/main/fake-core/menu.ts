// The fake core's item menus: a port of crates/hocket-core/src/actions/menu.rs. One layout for
// every platform: each row has a section (its place in the order), a default tier (the phone's
// short list or under More) and a priority (which default rows fold first when the short list is
// full). `menuLayout` applies what the target and screen offer, the user's `menu.overrides`, and
// the fold rule. Desktop draws every row; the phone folds.
import type { ActionDescriptor, ActionTarget, MenuItem, MenuItemInfo, MenuLayout, MenuScreen, MenuSection, MenuTier } from "@core/api";

/** Most rows in the phone's short list by default, counting the app's own extras. */
export const SHORT_LIST_MAX = 6;
/** A menu with this many rows or fewer shows them all, with no More. */
export const NO_FOLD_MAX = 7;

interface Def {
  id: string;
  section: MenuSection;
  /** Default tier: the short list (false) or under More (true). */
  folded: boolean;
  /** Which default short-list rows stay when it is full: higher stays. */
  priority: number;
  /** Only for one item, not a selection. */
  single: boolean;
  /** Registry actions in order (both halves of a pair), or "app": offered when the app lists it. */
  source: readonly string[] | "app";
}

const def = (id: string, section: MenuSection, folded: boolean, priority: number, single: boolean, source: readonly string[] | "app"): Def => ({ id, section, folded, priority, single, source });

/** Every row, in display order. */
export const MENU_DEFS: readonly Def[] = [
  def("play", "play", false, 0, false, ["play"]),
  def("playShuffled", "play", false, 0, false, ["playShuffled"]),
  def("playNext", "play", false, 0, false, ["playNext"]),
  def("addToQueue", "play", false, 0, false, ["addToQueue"]),
  def("playLater", "play", false, 0, false, ["playLater"]),
  def("rating", "rate", false, 0, false, ["rate5", "rate4", "rate3", "rate2", "rate1", "rate0"]),
  def("love", "rate", false, 0, false, ["love", "unlove"]),
  def("addToPlaylist", "organise", false, 80, false, ["addToPlaylist"]),
  def("stash", "organise", false, 60, false, ["stash"]),
  def("pinToLibrary", "organise", true, 30, true, ["pinToLibrary", "unpinFromLibrary"]),
  def("addToLibrary", "organise", true, 25, false, ["addToLibrary", "removeFromLibrary"]),
  def("restoreSavedQueue", "organise", false, 90, false, ["restoreSavedQueue"]),
  def("pinSavedQueue", "organise", false, 70, false, ["pinSavedQueue", "unpinSavedQueue"]),
  def("saveQueueAsPlaylist", "organise", false, 65, false, ["saveQueueAsPlaylist"]),
  def("goToAlbum", "goTo", false, 95, true, ["goToAlbum"]),
  def("goToArtist", "goTo", false, 100, true, ["goToArtist"]),
  def("download", "keep", false, 20, false, ["download", "unpin"]),
  def("share", "keep", false, 50, false, "app"),
  def("preview", "about", true, 15, true, ["preview"]),
  def("info", "about", true, 12, true, "app"),
  def("genius", "about", true, 10, true, "app"),
  def("lyricsTiming", "about", true, 8, true, "app"),
  def("snooze", "tune", true, 5, false, ["snooze", "unsnooze"]),
  def("removeFromQueue", "danger", false, 0, false, ["removeFromQueue"]),
  def("removeFromPlaylist", "danger", false, 0, false, ["removeFromPlaylist"]),
  def("deleteSavedQueue", "danger", false, 0, false, ["deleteSavedQueue"]),
  def("deletePlaylist", "danger", false, 0, false, ["deletePlaylist"]),
];

const find = (id: string) => MENU_DEFS.find((d) => d.id === id);

/** The tiers a row may take: the play strip none; Rating, Love and removing shown or hidden. */
function choices(d: Def): MenuTier[] {
  if (d.section === "play") return [];
  if (d.section === "rate" || d.section === "danger") return ["shown", "hidden"];
  return ["shown", "more", "hidden"];
}

const defaultTier = (d: Def): MenuTier => (d.folded ? "more" : "shown");

/** `menu.overrides` (`{ id: tier }`), skipping what the core would refuse. */
export function parseOverrides(raw: string | undefined): Map<string, MenuTier> {
  const out = new Map<string, MenuTier>();
  let v: unknown;
  try {
    v = JSON.parse(raw ?? "{}");
  } catch {
    return out;
  }
  if (!v || typeof v !== "object" || Array.isArray(v)) return out;
  for (const [id, tier] of Object.entries(v as Record<string, unknown>)) {
    const d = find(id);
    if (d && choices(d).includes(tier as MenuTier)) out.set(id, tier as MenuTier);
  }
  return out;
}

/** `menu.overrides` with `id` set to `tier` (its default removes the entry); throws like the core refuses. */
export function withOverride(raw: string | undefined, id: string, tier: MenuTier): string {
  const d = find(id);
  if (!d) throw new Error(`unknown menu item ${id}`);
  if (!choices(d).includes(tier)) throw new Error(`${id} can't be ${tier}`);
  const map = Object.fromEntries(parseOverrides(raw));
  if (tier === defaultTier(d)) delete map[id];
  else map[id] = tier;
  return JSON.stringify(map);
}

/** The settings editor's list: every customisable row in menu order. */
export function menuItemsInfo(overrides: Map<string, MenuTier>): MenuItemInfo[] {
  return MENU_DEFS.filter((d) => choices(d).length).map((d) => ({
    id: d.id,
    section: d.section,
    defaultTier: defaultTier(d),
    tier: overrides.get(d.id) ?? defaultTier(d),
    choices: choices(d),
    appHandled: d.source === "app",
  }));
}

/** Whether the screen leaves the row out: the player doesn't preview what is playing, a page
 * doesn't link to itself, and only an editable playlist removes songs from itself. */
function screenDrops(id: string, screen: MenuScreen): boolean {
  switch (id) {
    case "preview": return screen === "nowPlaying";
    case "goToAlbum": return screen === "albumPage";
    case "goToArtist": return screen === "artistPage";
    case "removeFromPlaylist": return screen !== "playlist";
    default: return false;
  }
}

function targetCount(t: ActionTarget): number {
  switch (t.type) {
    case "tracks": return t.data.tracks.length;
    case "queueItems": return t.data.keys.length;
    case "albums":
    case "artists":
    case "playlists": return t.data.ids.length;
    case "savedQueue": return 1;
    default: return 0;
  }
}

export interface MenuRequest {
  target: ActionTarget;
  screen: MenuScreen;
  appItems: readonly string[];
  extras: number;
  overrides: Map<string, MenuTier>;
  /** The registry's context-menu descriptors that apply to the target and are offered. */
  descriptors: readonly ActionDescriptor[];
}

/** The menu for a target, in display order. */
export function menuLayout(req: MenuRequest): MenuLayout {
  const count = targetCount(req.target);
  const byId = new Map(req.descriptors.map((a) => [a.id, a]));
  const rows: { d: Def; actions: ActionDescriptor[]; tier: MenuTier | undefined }[] = [];
  for (const d of MENU_DEFS) {
    const tier = req.overrides.get(d.id);
    if (tier === "hidden" || screenDrops(d.id, req.screen)) continue;
    if (d.single && count > 1) continue;
    let actions: ActionDescriptor[] = [];
    if (d.source === "app") {
      if (!req.appItems.includes(d.id)) continue;
    } else {
      actions = d.source.flatMap((id) => byId.get(id) ?? []);
      if (!actions.length) continue;
    }
    rows.push({ d, actions, tier });
  }

  // The fold: only the list rows (not the strip, the stars, Love or removing).
  const foldable = (d: Def) => d.section !== "play" && d.section !== "rate" && d.section !== "danger";
  const listed = rows.filter((r) => foldable(r.d)).length;
  const folded = rows.map(() => false);
  if (listed + req.extras > NO_FOLD_MAX) {
    // Rows the user put in the menu always stay; the defaults fill what's left of the short list by priority.
    const pinned = rows.filter((r) => foldable(r.d) && r.tier === "shown").length;
    let room = Math.max(0, SHORT_LIST_MAX - req.extras - pinned);
    const byPriority = rows.map((_, i) => i).filter((i) => foldable(rows[i]!.d) && rows[i]!.tier === undefined && !rows[i]!.d.folded);
    byPriority.sort((a, b) => rows[b]!.d.priority - rows[a]!.d.priority);
    rows.forEach((r, i) => {
      if (foldable(r.d)) folded[i] = r.tier === "shown" ? false : r.tier !== undefined ? true : r.d.folded;
    });
    for (const i of byPriority) {
      if (room > 0) room--;
      else folded[i] = true;
    }
    // A More holding one row is a tap for nothing.
    if (folded.filter(Boolean).length === 1) folded.fill(false);
  }

  return { items: rows.map((r, i): MenuItem => ({ id: r.d.id, section: r.d.section, folded: folded[i] ?? false, actions: r.actions })) };
}
