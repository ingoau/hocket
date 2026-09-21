// Default keyboard map from docs/design.md "Interface → Keyboard", keyed by
// the registry's CANONICAL action ids (crates/hocket-core/src/actions/defs.rs).
// Every entry is rebindable; Query.Shortcuts overrides by action id and
// SetShortcut persists the user's changes. Ids with a `ui.` prefix have no
// registry equivalent and are executed by the renderer only.
export interface DefaultBinding {
  actionId: string;
  shortcut: string;
  labelId: string;
  category: string;
}

export const DEFAULT_KEYMAP: DefaultBinding[] = [
  { actionId: "openCommandPalette", shortcut: "Mod+K", labelId: "action.palette", category: "app" },
  { actionId: "findInList", shortcut: "Mod+F", labelId: "action.search", category: "app" },
  { actionId: "togglePlay", shortcut: "Space", labelId: "action.togglePlay", category: "playback" },
  { actionId: "seekBackward", shortcut: "ArrowLeft", labelId: "action.seekBack", category: "playback" },
  { actionId: "seekForward", shortcut: "ArrowRight", labelId: "action.seekForward", category: "playback" },
  { actionId: "previous", shortcut: "Shift+ArrowLeft", labelId: "action.previous", category: "playback" },
  { actionId: "next", shortcut: "Shift+ArrowRight", labelId: "action.next", category: "playback" },
  { actionId: "toggleQueuePanel", shortcut: "Q", labelId: "action.toggleQueue", category: "view" },
  { actionId: "toggleLyrics", shortcut: "L", labelId: "action.toggleLyrics", category: "view" },
  { actionId: "toggleFullscreen", shortcut: "F", labelId: "action.fullscreen", category: "view" },
  { actionId: "toggleMiniPlayer", shortcut: "M", labelId: "action.miniPlayer", category: "view" },
  { actionId: "rate0", shortcut: "0", labelId: "action.rate0", category: "library" },
  { actionId: "rate1", shortcut: "1", labelId: "action.rate1", category: "library" },
  { actionId: "rate2", shortcut: "2", labelId: "action.rate2", category: "library" },
  { actionId: "rate3", shortcut: "3", labelId: "action.rate3", category: "library" },
  { actionId: "rate4", shortcut: "4", labelId: "action.rate4", category: "library" },
  { actionId: "rate5", shortcut: "5", labelId: "action.rate5", category: "library" },
  { actionId: "undo", shortcut: "Mod+Z", labelId: "action.undo", category: "edit" },
  { actionId: "redo", shortcut: "Mod+Shift+Z", labelId: "action.redo", category: "edit" },
  { actionId: "redoAlt", shortcut: "Mod+Y", labelId: "action.redo", category: "edit" },
  { actionId: "selectAll", shortcut: "Mod+A", labelId: "action.selectAll", category: "edit" },
  { actionId: "remove", shortcut: "Delete", labelId: "action.delete", category: "edit" },
  { actionId: "ui.escape", shortcut: "Escape", labelId: "action.escape", category: "app" },
  { actionId: "navigateSettings", shortcut: "Mod+,", labelId: "action.settings", category: "app" },
  { actionId: "shuffle", shortcut: "S", labelId: "action.shuffle", category: "playback" },
  { actionId: "repeat", shortcut: "R", labelId: "action.repeat", category: "playback" },
  { actionId: "volumeUp", shortcut: "Mod+ArrowUp", labelId: "action.volumeUp", category: "playback" },
  { actionId: "volumeDown", shortcut: "Mod+ArrowDown", labelId: "action.volumeDown", category: "playback" },
  { actionId: "love", shortcut: "Mod+L", labelId: "action.love", category: "library" },
  { actionId: "ui.back", shortcut: "Alt+ArrowLeft", labelId: "action.back", category: "view" },
  { actionId: "ui.forward", shortcut: "Alt+ArrowRight", labelId: "action.forward", category: "view" },
];

/**
 * Renderer-side aliases → canonical registry ids. The core accepts the same
 * aliases on input (actions/mod.rs ALIASES); descriptors always carry the
 * canonical id, so the UI keys on canonical ids everywhere.
 */
export const ACTION_ALIASES: Record<string, string> = {
  "rate.0": "rate0", "rate.1": "rate1", "rate.2": "rate2", "rate.3": "rate3", "rate.4": "rate4", "rate.5": "rate5",
  "track.love": "love", "track.unlove": "unlove",
  "transport.togglePlay": "togglePlay", "transport.next": "next", "transport.previous": "previous",
  "transport.seekBack": "seekBackward", "transport.seekForward": "seekForward", "transport.shuffle": "shuffle",
  "transport.repeat": "repeat", "transport.toggleAutoplay": "autoplay", "transport.volumeUp": "volumeUp",
  "transport.volumeDown": "volumeDown", "transport.resumeHere": "resumeHere",
  "ms.play": "togglePlay", "ms.pause": "pause", "ms.stop": "stop", "ms.next": "next", "ms.previous": "previous",
  "ms.shuffle": "shuffle", "ms.repeat": "repeat", "ms.love": "love",
  "nav.home": "navigateHome", "nav.songs": "navigateTracks", "nav.albums": "navigateAlbums", "nav.artists": "navigateArtists",
  "nav.playlists": "navigatePlaylists", "nav.genres": "navigateGenres", "nav.recent": "navigateRecent", "nav.filters": "navigateFilters",
  "nav.downloads": "navigateDownloads", "nav.stats": "navigateStats", "nav.settings": "navigateSettings",
  "ui.palette": "openCommandPalette", "ui.search": "findInList", "ui.queue": "toggleQueuePanel", "ui.fullscreen": "toggleFullscreen",
  "ui.miniPlayer": "toggleMiniPlayer", "ui.lyrics": "toggleLyrics", "ui.selectAll": "selectAll", "ui.saveQueueAsPlaylist": "saveQueueAsPlaylist",
  "ui.removeFromPlaylist": "removeFromPlaylist", "ui.removeDownload": "unpin", "ui.delete": "remove", "ui.goToArtist": "goToArtist",
  "ui.goToAlbum": "goToAlbum", "ui.deletePlaylist": "deletePlaylist", "ui.addToPlaylist": "addToPlaylist", "ui.sleepTimer": "sleepTimer",
  "ui.playOn": "handoff", "ui.settings": "navigateSettings", "ui.openStats": "navigateStats", "ui.openFilters": "navigateFilters",
  "ui.openDownloads": "navigateDownloads", "ui.copyDiagnostics": "copyDiagnostics", "redo.alt": "redoAlt",
};

export function canonicalActionId(id: string): string {
  return ACTION_ALIASES[id] ?? id;
}

/** Sidebar navigation ids → renderer views. */
export const NAV_VIEWS: Record<string, string> = {
  navigateHome: "home", navigateTracks: "songs", navigateAlbums: "albums", navigateArtists: "artists", navigatePlaylists: "playlists",
  navigateGenres: "genres", navigateFilters: "filters", navigateDownloads: "downloads", navigateStats: "stats", navigateSettings: "settings",
};
