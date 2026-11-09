// The sidebar's "Your library" list: every playlist, plus the albums and artists played lately
// (as a queue's context or through their songs), newest first by default. Pinned items are left
// out (the pins sit just above), the chips narrow it to one kind and the inline field filters it
// by name. Pure, so the rules are tested apart from the component (components/SidebarLibrary.tsx).
import type { PlayHistoryEntry, Playlist, QueueContext, SavedQueue } from "@core/api";

export type LibraryKind = "playlist" | "album" | "artist";
export const LIBRARY_KINDS: readonly LibraryKind[] = ["playlist", "album", "artist"];

export type LibrarySort = "recents" | "added" | "alpha";
export const LIBRARY_SORTS: readonly LibrarySort[] = ["recents", "added", "alpha"];

export interface LibraryEntry {
  kind: LibraryKind;
  id: string;
  title: string;
  /** The playlist's owner or the album's artist (artists have none). */
  owner?: string;
  coverArt?: string;
  /** When it was last played (a queue from it, or one of its songs). */
  playedAt?: number;
  /** When it came into the library, where known (playlists). */
  addedAt?: number;
}

/** How many albums and artists the history adds at most: the list is about what's current. */
export const RECENT_ALBUMS = 24;
export const RECENT_ARTISTS = 12;

export const entryKey = (e: { kind: string; id: string }): string => `${e.kind}:${e.id}`;

/** What is playing now, as the list counts it: the queue's context and when it started. */
export interface CurrentContext {
  kind: QueueContext["kind"];
  label: string;
  coverArt?: string;
  at: number;
}

/**
 * The list before filtering: playlists, then what the play history, the saved queues and the queue
 * playing now add, one entry per item with the latest play it has. Names come from the library
 * (playlists, the songs' album and artist) before a queue's label, which can be anything.
 */
export function collectLibrary(playlists: readonly Playlist[], queues: readonly SavedQueue[], history: readonly PlayHistoryEntry[], current?: CurrentContext): LibraryEntry[] {
  const out = new Map<string, LibraryEntry>();
  const seen = (e: LibraryEntry) => {
    const key = entryKey(e);
    const had = out.get(key);
    if (!had) {
      out.set(key, e);
      return;
    }
    // Keep the richer fields, and the latest play.
    out.set(key, {
      ...had,
      title: had.title || e.title,
      owner: had.owner ?? e.owner,
      coverArt: had.coverArt ?? e.coverArt,
      playedAt: Math.max(had.playedAt ?? 0, e.playedAt ?? 0) || undefined,
    });
  };
  for (const p of playlists) seen({ kind: "playlist", id: p.id, title: p.name, owner: p.owner, coverArt: p.coverArt, addedAt: p.created ?? p.changed });
  let albums = 0;
  let artists = 0;
  for (const h of history) {
    const tr = h.track;
    if (tr.albumId && (out.has(`album:${tr.albumId}`) || albums < RECENT_ALBUMS)) {
      if (!out.has(`album:${tr.albumId}`)) albums++;
      seen({ kind: "album", id: tr.albumId, title: tr.album ?? "", owner: tr.artist, coverArt: tr.coverArt, playedAt: h.playedAt });
    }
    const artistId = tr.artistId ?? tr.artists?.[0]?.id;
    const artistName = tr.artists?.[0]?.name ?? tr.artist;
    if (artistId && artistName && (out.has(`artist:${artistId}`) || artists < RECENT_ARTISTS)) {
      if (!out.has(`artist:${artistId}`)) artists++;
      seen({ kind: "artist", id: artistId, title: artistName, playedAt: h.playedAt });
    }
  }
  const fromQueue = (k: QueueContext["kind"], label: string, coverArt: string | undefined, at: number) => {
    if (k.type !== "album" && k.type !== "artist" && k.type !== "playlist") return;
    // A queue from a playlist the library no longer has (deleted, another server) isn't listed.
    if (k.type === "playlist" && !out.has(`playlist:${k.data.id}`)) return;
    seen({ kind: k.type, id: k.data.id, title: label, coverArt: k.type === "artist" ? undefined : coverArt, playedAt: at });
  };
  for (const q of queues) fromQueue(q.context.kind, q.context.label || q.label, q.coverArt, q.lastInteractedAt);
  if (current) fromQueue(current.kind, current.label, current.coverArt, current.at);
  // Nothing without a name: an unnamed album from a song with no album tag.
  return [...out.values()].filter((e) => e.title.trim() !== "");
}

export interface LibraryView {
  kind?: LibraryKind;
  sort: LibrarySort;
  /** The inline filter's text. */
  query?: string;
}

/** What the list shows: without [pinned] keys (kind:id), of one kind, matching the filter, sorted. */
export function viewLibrary(entries: readonly LibraryEntry[], pinned: ReadonlySet<string>, view: LibraryView): LibraryEntry[] {
  const q = view.query?.trim().toLocaleLowerCase() ?? "";
  const list = entries.filter((e) => !pinned.has(entryKey(e)) && (!view.kind || e.kind === view.kind) && (!q || e.title.toLocaleLowerCase().includes(q) || (e.owner ?? "").toLocaleLowerCase().includes(q)));
  const byTitle = (a: LibraryEntry, b: LibraryEntry) => a.title.localeCompare(b.title, undefined, { sensitivity: "base" }) || entryKey(a).localeCompare(entryKey(b));
  // Recents: the last play, else when it was added (a playlist never played sits by its date).
  const recent = (e: LibraryEntry) => e.playedAt ?? e.addedAt ?? 0;
  switch (view.sort) {
    case "alpha": return list.sort(byTitle);
    // Recently added: what has a date (playlists) newest first, then what the library gives none
    // (albums and artists met through plays) by their last play.
    case "added": return list.sort((a, b) => Number(a.addedAt === undefined) - Number(b.addedAt === undefined) || (b.addedAt ?? 0) - (a.addedAt ?? 0) || recent(b) - recent(a) || byTitle(a, b));
    default: return list.sort((a, b) => recent(b) - recent(a) || byTitle(a, b));
  }
}

/** A stored view (from local storage), with anything unknown dropped. */
export function parseLibraryView(raw: unknown): LibraryView {
  const v = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const kind = LIBRARY_KINDS.includes(v.kind as LibraryKind) ? (v.kind as LibraryKind) : undefined;
  const sort = LIBRARY_SORTS.includes(v.sort as LibrarySort) ? (v.sort as LibrarySort) : "recents";
  return { kind, sort };
}
