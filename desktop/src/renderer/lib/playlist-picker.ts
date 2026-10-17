// The add to playlist submenu's model: which playlists it lists, in which groups (Pinned, Recent,
// All playlists; one Matches group while searching), and what each row shows and does for the
// songs being added, from the core's playlist membership (the indices where a playlist holds them).
import type { Playlist, PlaylistMembership } from "@core/api";
import type { Check } from "./menu-model";
import type { LibraryPin } from "./library-pins";

export type PickerGroupId = "pinned" | "recent" | "all" | "matches";

/** The playlists' order while the submenu is open: ids per group. */
export interface PickerLayout {
  pinned: string[];
  recent: string[];
  all: string[];
}

/** How many recently changed playlists the Recent group shows. */
export const RECENT_COUNT = 3;

/** Playlists songs can be added to: the user's own, not smart (rules-driven, read-only) ones. */
export function canAddTo(p: Playlist): boolean {
  return !p.isSmart && p.isMine;
}

/**
 * Pinned: the playlist pins, in pin order. Recent: the [RECENT_COUNT] most recently changed of the
 * rest. All playlists: the others, by name.
 */
export function layoutPlaylists(playlists: readonly Playlist[], pins: readonly LibraryPin[], recentCount = RECENT_COUNT): PickerLayout {
  const addable = playlists.filter(canAddTo);
  const byId = new Map(addable.map((p) => [p.id, p]));
  const pinned = pins.filter((p) => p.kind === "playlist" && byId.has(p.id)).map((p) => p.id);
  const pinnedSet = new Set(pinned);
  const others = addable.filter((p) => !pinnedSet.has(p.id));
  const recent = [...others].filter((p) => p.changed !== undefined).sort((a, b) => (b.changed ?? 0) - (a.changed ?? 0)).slice(0, recentCount).map((p) => p.id);
  const recentSet = new Set(recent);
  const all = others.filter((p) => !recentSet.has(p.id)).sort(byName).map((p) => p.id);
  return { pinned, recent, all };
}

function byName(a: Playlist, b: Playlist): number {
  return a.name.localeCompare(b.name, undefined, { sensitivity: "base" }) || a.id.localeCompare(b.id);
}

/**
 * The layout kept while the submenu is open: adding a song changes a playlist (and so what Recent
 * would hold), but rows must not jump under the pointer. Gone playlists drop out; a new one (just
 * created from the search) leads Recent, or joins Pinned when it is pinned.
 */
export function keepLayout(prev: PickerLayout, next: PickerLayout): PickerLayout {
  const live = new Set([...next.pinned, ...next.recent, ...next.all]);
  const known = new Set([...prev.pinned, ...prev.recent, ...prev.all]);
  const fresh = (ids: string[]) => ids.filter((id) => !known.has(id));
  const keep = (ids: string[]) => ids.filter((id) => live.has(id));
  return {
    pinned: [...keep(prev.pinned), ...fresh(next.pinned)],
    recent: [...fresh(next.recent), ...fresh(next.all), ...keep(prev.recent)],
    all: keep(prev.all),
  };
}

export interface PickerGroup {
  id: PickerGroupId;
  playlists: Playlist[];
}

/** The groups to draw: the three, or, while searching, one Matches group in the same order. Empty groups are left out. */
export function pickerGroups(layout: PickerLayout, byId: ReadonlyMap<string, Playlist>, query: string): PickerGroup[] {
  const pick = (ids: string[]) => ids.map((id) => byId.get(id)).filter((p): p is Playlist => !!p);
  const q = query.trim().toLocaleLowerCase();
  const groups: PickerGroup[] = q
    ? [{ id: "matches", playlists: pick([...layout.pinned, ...layout.recent, ...layout.all]).filter((p) => p.name.toLocaleLowerCase().includes(q)) }]
    : [
        { id: "pinned", playlists: pick(layout.pinned) },
        { id: "recent", playlists: pick(layout.recent) },
        { id: "all", playlists: pick(layout.all) },
      ];
  return groups.filter((g) => g.playlists.length);
}

/** One playlist's row for the songs being added. */
export interface RowState {
  /** In it: every song (true), some of them (mixed) or none. */
  checked: Check;
  /** Copies held, when every song is in it more than once (the ×2 badge); else 0. */
  copies: number;
  /** Every entry of the songs (Remove all). */
  indices: number[];
  /** The last copy of each song (Remove one). */
  lastCopies: number[];
  /** The songs it doesn't hold yet. */
  missing: string[];
}

export function rowState(trackIds: readonly string[], m: PlaylistMembership | undefined): RowState {
  const wanted = [...new Set(trackIds)];
  const byTrack = new Map<string, number[]>();
  for (const e of m?.entries ?? []) {
    if (!wanted.includes(e.trackId)) continue;
    byTrack.set(e.trackId, [...(byTrack.get(e.trackId) ?? []), e.index]);
  }
  const held = wanted.filter((id) => byTrack.has(id));
  const missing = wanted.filter((id) => !byTrack.has(id));
  const checked: Check = !held.length ? false : missing.length ? "mixed" : true;
  const least = held.length ? Math.min(...held.map((id) => byTrack.get(id)?.length ?? 0)) : 0;
  const indices = [...byTrack.values()].flat().sort((a, b) => a - b);
  const lastCopies = [...byTrack.values()].map((ix) => Math.max(...ix)).sort((a, b) => a - b);
  return { checked, copies: checked === true && least > 1 ? least : 0, indices, lastCopies, missing };
}

/**
 * What a plain click (or Enter / Space) on a row does: add the songs, add the ones it lacks, remove
 * them, or (holding several copies) ask whether to remove all or one.
 */
export function clickIntent(s: RowState): "add" | "addMissing" | "remove" | "ask" {
  if (s.checked === false) return "add";
  if (s.checked === "mixed") return "addMissing";
  return s.copies > 1 ? "ask" : "remove";
}
