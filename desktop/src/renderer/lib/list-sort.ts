// The library lists' sorts: which orders each list offers (with the icon and the direction wording
// the sort popover shows), the order each opens with, what it remembers (device-local, one entry per
// list), and the renderer-side sorts of the lists the core hands over whole (playlists, genres).
import type { Genre, Playlist, SortOrder } from "@core/api";
import type { StringId } from "@shared/strings";
import { loadLocal, saveLocal } from "./local-settings";

export type ListId = "albums" | "artists" | "playlists" | "songs" | "genres";

/** How a sort names its two directions: A–Z / Z–A, Oldest / Newest, Fewest / Most and so on. */
export type DirectionWords = "alpha" | "time" | "count" | "rating" | "length" | "plain";

export interface SortOption {
  /** A core SortOrder for the paged lists; the renderer's own key for playlists and genres. */
  value: string;
  label: StringId;
  icon: string;
  words: DirectionWords;
}

export interface ListSort {
  sort: string;
  descending: boolean;
}

/** Playlists and genres come whole and are sorted here, by these keys. */
export type PlaylistSort = "name" | "changed" | "created" | "songCount" | "duration";
export type GenreSort = "name" | "songCount" | "albumCount";

/** The artist-only orders: everywhere else the core reads them as its default order. */
export const ARTIST_ONLY_SORTS: readonly SortOrder[] = ["albumCount", "songCount"];

const opt = (value: string, label: StringId, icon: string, words: DirectionWords): SortOption => ({ value, label, icon, words });

const LISTS: Record<ListId, { options: SortOption[]; fallback: ListSort }> = {
  albums: {
    options: [
      opt("title", "sort.title", "sortAlpha", "alpha"),
      opt("artist", "sort.artist", "artist", "alpha"),
      opt("year", "sort.year", "calendar", "time"),
      opt("dateAdded", "sort.dateAdded", "schedule", "time"),
      opt("rating", "sort.rating", "star", "rating"),
      opt("playCount", "sort.playCount", "headphones", "count"),
      opt("random", "sort.random", "shuffle", "plain"),
    ],
    fallback: { sort: "dateAdded", descending: true },
  },
  artists: {
    options: [
      opt("title", "sort.name", "sortAlpha", "alpha"),
      opt("albumCount", "sort.albumCount", "album", "count"),
      opt("songCount", "sort.songCount", "song", "count"),
      opt("playCount", "sort.playCount", "headphones", "count"),
      opt("dateAdded", "sort.dateAdded", "schedule", "time"),
      opt("random", "sort.random", "shuffle", "plain"),
    ],
    fallback: { sort: "title", descending: false },
  },
  playlists: {
    options: [
      opt("name", "sort.name", "sortAlpha", "alpha"),
      opt("changed", "sort.changed", "update", "time"),
      opt("created", "sort.created", "schedule", "time"),
      opt("songCount", "sort.songCount", "song", "count"),
      opt("duration", "sort.duration", "timer", "length"),
    ],
    fallback: { sort: "name", descending: false },
  },
  songs: {
    // The orders the table's column headers offer, plus when a song was added.
    options: [
      opt("title", "sort.title", "sortAlpha", "alpha"),
      opt("artist", "sort.artist", "artist", "alpha"),
      opt("album", "sort.album", "album", "alpha"),
      opt("year", "sort.year", "calendar", "time"),
      opt("dateAdded", "sort.dateAdded", "schedule", "time"),
      opt("rating", "sort.rating", "star", "rating"),
      opt("playCount", "sort.playCount", "headphones", "count"),
      opt("duration", "sort.duration", "timer", "length"),
      opt("bpm", "sort.bpm", "speed", "count"),
    ],
    fallback: { sort: "title", descending: false },
  },
  genres: {
    options: [
      opt("name", "sort.name", "sortAlpha", "alpha"),
      opt("songCount", "sort.songCount", "song", "count"),
      opt("albumCount", "sort.albumCount", "album", "count"),
    ],
    fallback: { sort: "name", descending: false },
  },
};

export function sortOptions(list: ListId): readonly SortOption[] {
  return LISTS[list].options;
}

export function defaultSort(list: ListId): ListSort {
  return { ...LISTS[list].fallback };
}

/** The labels of a sort's two directions, ascending first. */
export function directionLabels(words: DirectionWords): [StringId, StringId] {
  return words === "plain" ? ["sort.dir.asc", "sort.dir.desc"] : [`sort.dir.${words}.asc`, `sort.dir.${words}.desc`];
}

const storageKey = (list: ListId) => `listSort.${list}`;

/** Before one entry per list, Albums and Songs kept the sort and the direction apart. */
const LEGACY: Partial<Record<ListId, [string, string]>> = { albums: ["albums.sort", "albums.desc"], songs: ["songs.sort", "songs.desc"] };

/**
 * A stored value as the list's sort: the stored sort when the list still offers it (a later release
 * may drop one), its direction when it is a boolean, else the list's default for either.
 */
export function parseListSort(list: ListId, raw: unknown): ListSort {
  const fallback = defaultSort(list);
  if (!raw || typeof raw !== "object") return fallback;
  const o = raw as Record<string, unknown>;
  const known = typeof o.sort === "string" && LISTS[list].options.some((x) => x.value === o.sort);
  return { sort: known ? (o.sort as string) : fallback.sort, descending: typeof o.descending === "boolean" ? o.descending : fallback.descending };
}

export function loadListSort(list: ListId): ListSort {
  const stored = loadLocal<unknown>(storageKey(list), undefined);
  if (stored !== undefined) return parseListSort(list, stored);
  const legacy = LEGACY[list];
  if (legacy) {
    const sort = loadLocal<unknown>(legacy[0], undefined);
    const descending = loadLocal<unknown>(legacy[1], undefined);
    if (sort !== undefined || descending !== undefined) return parseListSort(list, { sort, descending });
  }
  return defaultSort(list);
}

export function saveListSort(list: ListId, value: ListSort): void {
  saveLocal(storageKey(list), { sort: value.sort, descending: value.descending });
}

const byName = (a: string, b: string) => a.localeCompare(b, undefined, { sensitivity: "base" });

/** Playlists in [sort] order; ties (and playlists missing a date) fall back to name order. */
export function sortPlaylists(playlists: readonly Playlist[], sort: string, descending: boolean): Playlist[] {
  const key = (p: Playlist): number | undefined => {
    switch (sort as PlaylistSort) {
      case "changed": return p.changed;
      case "created": return p.created;
      case "songCount": return p.songCount;
      case "duration": return p.durationMs;
      default: return undefined;
    }
  };
  return sortWith(playlists, key, (p) => p.name, (p) => p.id, descending);
}

/** Genres in [sort] order; ties fall back to name order. */
export function sortGenres(genres: readonly Genre[], sort: string, descending: boolean): Genre[] {
  const key = (g: Genre): number | undefined => (sort === "songCount" ? g.songCount : sort === "albumCount" ? g.albumCount : undefined);
  return sortWith(genres, key, (g) => g.name, (g) => g.name, descending);
}

/**
 * By [key] (missing keys last in either direction), else by name, in [descending] order for the key
 * or, when there is no key (a name sort), for the name; ties on the key always read A–Z.
 */
function sortWith<T>(items: readonly T[], key: (x: T) => number | undefined, name: (x: T) => string, id: (x: T) => string, descending: boolean): T[] {
  const dir = descending ? -1 : 1;
  return [...items].sort((a, b) => {
    const ka = key(a);
    const kb = key(b);
    if (ka === undefined && kb === undefined) return dir * byName(name(a), name(b)) || id(a).localeCompare(id(b));
    if (ka === undefined) return 1;
    if (kb === undefined) return -1;
    return dir * (ka - kb) || byName(name(a), name(b)) || id(a).localeCompare(id(b));
  });
}
