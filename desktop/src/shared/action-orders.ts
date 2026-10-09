// What each choose-and-order surface can hold, so Settings can list the
// actions that are switched off too (the core's Actions query returns only
// the chosen ones). Mirrors the default lists in
// crates/hocket-core/src/actions/defs.rs; anything else already in a stored
// order (navigateLibrary from an older phone build) is listed while it is on.
export const CUSTOMISABLE: Record<string, readonly string[]> = {
  sidebar: ["navigateHome", "navigateTracks", "navigateAlbums", "navigateArtists", "navigatePlaylists", "navigateGenres", "navigateRecent", "navigateFilters", "navigateDownloads", "navigateStats", "navigateSettings"],
  contextMenu: ["play", "playShuffled", "playNext", "playLater", "addToPlaylist", "love", "unlove", "rate5", "rate4", "rate3", "rate2", "rate1", "rate0", "download", "unpin", "goToAlbum", "goToArtist", "removeFromQueue", "removeFromPlaylist", "restoreSavedQueue", "pinSavedQueue", "unpinSavedQueue", "saveQueueAsPlaylist", "deleteSavedQueue", "deletePlaylist"],
  mediaSession: ["previous", "togglePlay", "next", "shuffle", "repeat", "love"],
};
