// What each choose-and-order surface can hold, so Settings can list the
// actions that are switched off too (the core's Actions query returns only
// the chosen ones). Mirrors the default lists in
// crates/hocket-core/src/actions/defs.rs; anything else already in a stored
// order (navigateLibrary from an older phone build) is listed while it is on.
export const CUSTOMISABLE: Record<string, readonly string[]> = {
  sidebar: ["navigateHome", "navigateLibrary", "navigateTracks", "navigateAlbums", "navigateArtists", "navigatePlaylists", "navigateGenres", "navigateRecent", "navigateFilters", "navigateDownloads", "navigateStats"],
  contextMenu: ["play", "playShuffled", "playNext", "addToQueue", "playLater", "preview", "addToPlaylist", "stash", "love", "unlove", "rate5", "rate4", "rate3", "rate2", "rate1", "rate0", "download", "unpin", "goToAlbum", "goToArtist", "snooze", "unsnooze", "pinToLibrary", "unpinFromLibrary", "removeFromQueue", "removeFromPlaylist", "restoreSavedQueue", "pinSavedQueue", "unpinSavedQueue", "saveQueueAsPlaylist", "deleteSavedQueue", "deletePlaylist"],
  mediaSession: ["previous", "next", "shuffle", "repeat", "love", "stash"],
};

// Never listed, even when a stored order names them, because their switch would
// do nothing: Settings isn't a sidebar place (it's always in the sidebar's
// footer), and play/pause is always on every media session.
export const NOT_LISTED: Record<string, readonly string[]> = { sidebar: ["navigateSettings"], mediaSession: ["togglePlay"] };
