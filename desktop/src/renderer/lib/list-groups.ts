// The groups at the top of each library list: Pinned (that kind's Library pins, in pin order), then
// Recent (what was played lately, or for playlists the most recently changed), then the whole list.
// A group is short, and Recent leaves out what Pinned already shows.
import type { PlayHistoryEntry, Playlist, TrackSummary } from "@core/api";

/** The most items a Recent group shows (a single row of tiles on the grid pages). */
export const GROUP_SIZE = 6;

/**
 * The distinct ids [pick] reads off the recently played songs, most recent first, without those in
 * [exclude], at most [limit].
 */
export function recentFromHistory(history: readonly PlayHistoryEntry[], pick: (t: TrackSummary) => string | undefined, exclude: ReadonlySet<string> = new Set(), limit = GROUP_SIZE): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const h of history) {
    const id = pick(h.track);
    if (!id || seen.has(id) || exclude.has(id)) continue;
    seen.add(id);
    out.push(id);
    if (out.length >= limit) break;
  }
  return out;
}

/** The most recently changed playlists that aren't pinned (as the add to playlist submenu's Recent). */
export function recentPlaylists(playlists: readonly Playlist[], pinned: ReadonlySet<string>, limit = GROUP_SIZE): Playlist[] {
  return playlists
    .filter((p) => p.changed !== undefined && !pinned.has(p.id))
    .sort((a, b) => (b.changed ?? 0) - (a.changed ?? 0) || a.id.localeCompare(b.id))
    .slice(0, limit);
}

/**
 * The genres of the recently played songs, most recent first: [genreOf] maps a song id to its genre
 * (songs it doesn't know are skipped); only genres in [known] count, pinned ones are left out.
 */
export function recentGenres(trackIds: readonly string[], genreOf: ReadonlyMap<string, string | undefined>, known: ReadonlySet<string>, pinned: ReadonlySet<string>, limit = GROUP_SIZE): string[] {
  const out: string[] = [];
  for (const id of trackIds) {
    const g = genreOf.get(id)?.trim();
    if (!g || !known.has(g) || pinned.has(g) || out.includes(g)) continue;
    out.push(g);
    if (out.length >= limit) break;
  }
  return out;
}
