// Which LibraryChanged events a view has to refetch for. The core sends one for every stream-cache
// entry and every recorded play, so refetching every mounted query on each (#312) re-ran server-backed
// queries (top songs, live home shelves) that wait out a timeout when the server is away.
//
// The store keeps a short log of recent changes; a view says which mirror tables it reads
// (`LibraryDep`) and refetches only when a change since it last looked touches one of them.
import type { Query } from "@core/api";

/** One LibraryChanged (or a finished sync, which is `tables: []`). Empty `tables` means everything. */
export interface LibraryChange {
  tables: string[];
  ids: string[];
}

export interface LibraryChangeLog {
  /** Bumped for every change; views compare it with the one they last saw. */
  seq: number;
  /** The last few changes, oldest first, each with the `seq` it got. */
  recent: (LibraryChange & { seq: number })[];
}

/** Changes kept for views that look late (a hidden window, a batch of events in one render). */
export const LIBRARY_LOG_CAP = 64;

export const emptyLibraryLog: LibraryChangeLog = { seq: 0, recent: [] };

export function recordLibraryChange(log: LibraryChangeLog, change: LibraryChange): LibraryChangeLog {
  const seq = log.seq + 1;
  const recent = [...log.recent, { ...change, seq }];
  return { seq, recent: recent.length > LIBRARY_LOG_CAP ? recent.slice(recent.length - LIBRARY_LOG_CAP) : recent };
}

/** The changes after `seen`, or "all" when some of them already fell out of the log. */
export function changesSince(log: LibraryChangeLog, seen: number): LibraryChange[] | "all" {
  if (log.seq <= seen) return [];
  const first = log.recent[0];
  if (!first || first.seq > seen + 1) return "all";
  return log.recent.filter((c) => c.seq > seen);
}

/**
 * A mirror table a view reads. With `id`, the view shows that one item: a change of `table` that
 * names other ids passes it by. With `rows`, a change of `table` that names ids only matters when
 * one of them is among the rows on screen (a stream-cache entry or a play for some other track).
 * Only addressed lists use `rows`; a filtered list could gain a row from any track.
 */
export interface LibraryDep {
  table: string;
  id?: string;
  rows?: boolean;
}

/** What a view reads; `undefined` means anything (refetch on every change). */
export type LibraryDeps = readonly LibraryDep[] | undefined;

export function touches(deps: LibraryDeps, change: LibraryChange, shownIds?: () => Iterable<string> | undefined): boolean {
  if (!deps || change.tables.length === 0) return true;
  return deps.some((d) => {
    if (!change.tables.includes(d.table)) return false;
    if (change.ids.length === 0) return true;
    if (d.id !== undefined) return change.ids.includes(d.id);
    if (d.rows) {
      const shown = shownIds?.();
      if (!shown) return true;
      const ids = new Set(change.ids);
      for (const id of shown) if (ids.has(id)) return true;
      return false;
    }
    return true;
  });
}

export function touchesAny(deps: LibraryDeps, changes: LibraryChange[] | "all", shownIds?: () => Iterable<string> | undefined): boolean {
  return changes === "all" || changes.some((c) => touches(deps, c, shownIds));
}

const T = (table: string, more: Omit<LibraryDep, "table"> = {}): LibraryDep => ({ table, ...more });
const TRACK_ROWS = T("tracks", { rows: true });
/** A filtered or sorted track list: any change to tracks may add, drop or move a row. */
export const TRACKS_DEPS: LibraryDeps = [T("tracks")];
/**
 * Albums (a grid, a page): their own changes, and changes to tracks that name no ids (downloads
 * finished, a stream cache cleared: an album's offline badge). Track ids never name an album row.
 */
export const ALBUM_DEPS: LibraryDeps = [T("albums"), TRACK_ROWS];
export const ARTIST_DEPS: LibraryDeps = [T("artists"), T("albums")];
/** Tracks, albums, artists and playlists by id (pins, pinned items). */
export const LIBRARY_ITEM_DEPS: LibraryDeps = [T("tracks"), T("albums"), T("artists"), T("playlists")];

/** The mirror tables a query's answer comes from (table names as `crates/hocket-core` emits them). */
export function queryLibraryDeps(q: Query): LibraryDeps {
  switch (q.type) {
    case "tracks":
    case "trackCount":
    case "storage":
    case "trackSources":
    case "linkCandidates":
      return [T("tracks")];
    case "track":
      return [T("tracks", { id: q.data.track.id })];
    case "tracksByIds":
    case "related":
      return [TRACK_ROWS];
    case "albums":
    case "artistAppearsOn":
      return ALBUM_DEPS;
    case "albumCount":
      return [T("albums")];
    case "albumsOfTracks":
      return [T("albums"), T("tracks")];
    case "album":
      // Its own changes, and track changes that name no ids (its offline badge).
      return [T("albums", { id: q.data.id }), T("tracks", { id: q.data.id })];
    case "albumTracks":
      return [T("albums", { id: q.data.id }), TRACK_ROWS];
    case "albumResume":
      return [T("albums", { id: q.data.id }), T("play_history")];
    case "artists":
      return [T("artists")];
    case "artist":
      return [T("artists", { id: q.data.id }), T("albums")];
    case "artistTopSongs":
      // A cached answer the core refreshed in the background is `artistTopSongs` with the artist's id.
      return [T("artistTopSongs", { id: q.data.id }), TRACK_ROWS];
    case "artistInfo":
    case "albumInfo":
      // The server's notes: nothing in the mirror changes them, only a full sync asks again.
      return [];
    case "genres":
      return [T("genres")];
    case "playlists":
      return [T("playlists")];
    case "playlist":
      return [T("playlists", { id: q.data.id }), T("playlist_tracks", { id: q.data.id })];
    case "playlistTracks":
      return [T("playlists", { id: q.data.id }), T("playlist_tracks", { id: q.data.id }), TRACK_ROWS];
    case "playlistMembership":
      return [T("playlists"), T("playlist_tracks")];
    case "search":
      return [TRACK_ROWS, T("albums"), T("artists"), T("playlists")];
    case "homeShelves":
      return [T("home"), T("play_history"), T("albums"), T("playlists"), TRACK_ROWS];
    case "recentlyPlayed":
    case "stats":
      return [T("play_history"), T("tracks"), T("albums"), T("artists")];
    case "pins":
      return LIBRARY_ITEM_DEPS;
    default:
      return undefined;
  }
}

/**
 * The ids of the rows in a query answer (tracks, albums, playlists: whatever it lists), for `rows`
 * deps; `undefined` when the answer isn't a list of rows with ids.
 */
export function shownIds(data: unknown): string[] | undefined {
  const ids: string[] = [];
  const add = (list: unknown): boolean => {
    if (!Array.isArray(list)) return false;
    for (const x of list as unknown[]) if (x && typeof x === "object" && "id" in x && typeof x.id === "string") ids.push(x.id);
    return true;
  };
  if (Array.isArray(data)) {
    for (const x of data as unknown[]) {
      if (!x || typeof x !== "object") return undefined;
      if ("tracks" in x && "albums" in x) {
        // HomeShelf: its rows, not the shelf.
        add(x.tracks);
        add(x.albums);
        if ("playlists" in x) add(x.playlists);
      } else if ("id" in x && typeof x.id === "string") ids.push(x.id);
      else return undefined;
    }
    return ids;
  }
  if (data && typeof data === "object") {
    let lists = false;
    for (const [k, v] of Object.entries(data)) if (["items", "tracks", "albums", "artists", "playlists"].includes(k)) lists = add(v) || lists;
    return lists ? ids : undefined;
  }
  return undefined;
}

/**
 * The rows on screen, for `rows` deps, once what is on screen is settled: `undefined` (anything
 * matches) while an answer is still on its way. A change that lands then may already be past the
 * core's answer, and matched against the rows before it (none, on a first load) it would be dropped.
 */
export function settledIds(waiting: boolean, ids: () => Iterable<string> | undefined): Iterable<string> | undefined {
  return waiting ? undefined : ids();
}

/**
 * Whether the window shows the library, so changes are acted on now rather than held for later.
 * Electron's shown/minimised counts: Chromium can call an unfocused fullscreen window hidden (#45).
 */
export function showsLibrary(s: { pageVisible: boolean; windowState: { visible: boolean } }): boolean {
  return s.pageVisible || s.windowState.visible;
}
