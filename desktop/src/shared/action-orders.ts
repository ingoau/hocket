// What each choose-and-order surface can hold, so Settings can list the
// actions that are switched off too (the core's Actions query returns only
// the chosen ones). Mirrors the default lists in
// crates/hocket-core/src/actions/defs.rs; anything else already in a stored
// order (navigateLibrary from an older phone build) is listed while it is on.
export const CUSTOMISABLE: Record<string, readonly string[]> = {
  sidebar: ["navigateHome", "navigateLibrary", "navigateTracks", "navigateAlbums", "navigateArtists", "navigatePlaylists", "navigateGenres", "navigateRecent", "navigateFilters", "navigateDownloads", "navigateStats"],
  contextMenu: ["play", "playShuffled", "playNext", "addToQueue", "playLater", "preview", "addToPlaylist", "stash", "love", "unlove", "addToLibrary", "removeFromLibrary", "rate5", "rate4", "rate3", "rate2", "rate1", "rate0", "download", "unpin", "goToAlbum", "goToArtist", "snooze", "unsnooze", "pinToLibrary", "unpinFromLibrary", "removeFromQueue", "removeFromPlaylist", "restoreSavedQueue", "pinSavedQueue", "unpinSavedQueue", "saveQueueAsPlaylist", "deleteSavedQueue", "deletePlaylist"],
  mediaSession: ["previous", "next", "shuffle", "repeat", "love", "stash"],
};

// The core's default order per surface (`default_order` in
// crates/hocket-core/src/actions/defs.rs): what an empty stored order means.
export const DEFAULT_ORDER: Record<string, readonly string[]> = {
  sidebar: ["navigateHome", "navigateLibrary"],
  contextMenu: CUSTOMISABLE.contextMenu ?? [],
  mediaSession: ["previous", "togglePlay", "next", "shuffle", "repeat", "love"],
};

// Never listed, even when a stored order names them, because their switch would
// do nothing: Settings isn't a sidebar place (it's always in the sidebar's
// footer), and play/pause is always on every media session.
export const NOT_LISTED: Record<string, readonly string[]> = { sidebar: ["navigateSettings"], mediaSession: ["togglePlay"] };

/**
 * A surface's new stored order after Settings' list changed: `shown` (the list's switched-on ids, in
 * its order) put back into `stored` (the order as stored, or the default), where every id the list
 * holds (`listed`, on or off) is a slot. Ids the list doesn't hold keep their places, so actions it
 * can't show (the context menu's ones for queue rows, saved queues or playlists, or ones hidden for
 * this target) aren't switched off by changing the others. Switched-on ids beyond the slots go last.
 */
export function mergeOrder(stored: readonly string[], listed: ReadonlySet<string>, shown: readonly string[]): string[] {
  const rest = [...shown];
  const out: string[] = [];
  for (const id of stored) {
    if (!listed.has(id)) out.push(id);
    else if (rest.length) out.push(rest.shift() as string);
  }
  return [...out, ...rest];
}
