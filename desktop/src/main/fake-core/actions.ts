// The fake core's action registry. Mirrors what the real registry
// (crates/hocket-core/src/actions) exposes through Query.Actions: menus, the
// palette, media-session buttons and sidebar items are all generated from it.
// Ids prefixed `ui.` are executed by the renderer (they need a dialog, a
// navigation or a window), everything else by the core via RunAction.
import type { ActionDescriptor, ActionTarget } from "@core/api";

export type TargetKind = ActionTarget["type"];
export type Surface = "contextMenu" | "palette" | "sidebar" | "mediaSession" | "playerBar";

export interface RegistryEntry {
  id: string;
  label: string;
  icon: string;
  category: string;
  targets: TargetKind[];
  surfaces: Surface[];
  undoable?: boolean;
  destructive?: boolean;
  defaultShortcut?: string;
}

export const REGISTRY: RegistryEntry[] = [
  // Item actions
  { id: "play", label: "Play", icon: "play", category: "playback", targets: ["tracks", "albums", "artists", "playlists"], surfaces: ["contextMenu"] },
  { id: "playShuffled", label: "Shuffle play", icon: "shuffle", category: "playback", targets: ["albums", "artists", "playlists"], surfaces: ["contextMenu"] },
  { id: "playNext", label: "Play next", icon: "playNext", category: "queue", targets: ["tracks", "albums", "artists", "playlists", "queueItems"], surfaces: ["contextMenu"], undoable: true },
  { id: "playLater", label: "Play later", icon: "playLater", category: "queue", targets: ["tracks", "albums", "artists", "playlists"], surfaces: ["contextMenu"], undoable: true },
  { id: "addToPlaylist", label: "Add to playlist…", icon: "playlistAdd", category: "library", targets: ["tracks", "albums", "queueItems"], surfaces: ["contextMenu"], undoable: true },
  { id: "love", label: "Love", icon: "favorite", category: "library", targets: ["none", "tracks", "albums", "artists", "queueItems"], surfaces: ["contextMenu", "palette", "mediaSession"], undoable: true, defaultShortcut: "Mod+L" },
  { id: "unlove", label: "Unlove", icon: "heart_minus", category: "library", targets: ["none", "tracks", "albums", "artists", "queueItems"], surfaces: ["contextMenu", "palette"], undoable: true },
  { id: "rate5", label: "Rate 5 stars", icon: "star", category: "library", targets: ["tracks", "albums", "queueItems"], surfaces: ["contextMenu"], undoable: true, defaultShortcut: "5" },
  { id: "rate4", label: "Rate 4 stars", icon: "star", category: "library", targets: ["tracks", "albums", "queueItems"], surfaces: ["contextMenu"], undoable: true, defaultShortcut: "4" },
  { id: "rate3", label: "Rate 3 stars", icon: "star", category: "library", targets: ["tracks", "albums", "queueItems"], surfaces: ["contextMenu"], undoable: true, defaultShortcut: "3" },
  { id: "rate2", label: "Rate 2 stars", icon: "star", category: "library", targets: ["tracks", "albums", "queueItems"], surfaces: ["contextMenu"], undoable: true, defaultShortcut: "2" },
  { id: "rate1", label: "Rate 1 star", icon: "star", category: "library", targets: ["tracks", "albums", "queueItems"], surfaces: ["contextMenu"], undoable: true, defaultShortcut: "1" },
  { id: "rate0", label: "Clear rating", icon: "starOff", category: "library", targets: ["tracks", "albums", "queueItems"], surfaces: ["contextMenu"], undoable: true, defaultShortcut: "0" },
  { id: "download", label: "Download", icon: "download", category: "offline", targets: ["tracks", "albums", "playlists"], surfaces: ["contextMenu"] },
  { id: "unpin", label: "Remove download", icon: "downloadOff", category: "offline", targets: ["tracks", "albums", "playlists"], surfaces: ["contextMenu"], destructive: true },
  { id: "goToAlbum", label: "Go to album", icon: "album", category: "navigate", targets: ["tracks", "queueItems"], surfaces: ["contextMenu"] },
  { id: "goToArtist", label: "Go to artist", icon: "artist", category: "navigate", targets: ["tracks", "albums", "queueItems"], surfaces: ["contextMenu"] },
  { id: "ui.info", label: "Details", icon: "info", category: "navigate", targets: ["tracks"], surfaces: ["contextMenu"] },
  { id: "removeFromQueue", label: "Remove from queue", icon: "remove", category: "queue", targets: ["queueItems"], surfaces: ["contextMenu"], undoable: true },
  { id: "removeFromPlaylist", label: "Remove from playlist", icon: "remove", category: "library", targets: ["tracks"], surfaces: ["contextMenu"], undoable: true },
  { id: "ui.renamePlaylist", label: "Rename playlist…", icon: "edit", category: "library", targets: ["playlists"], surfaces: ["contextMenu"] },
  { id: "deletePlaylist", label: "Delete playlist", icon: "trash", category: "library", targets: ["playlists"], surfaces: ["contextMenu"], destructive: true },
  { id: "restoreSavedQueue", label: "Restore", icon: "restore", category: "queue", targets: ["savedQueue"], surfaces: ["contextMenu"], undoable: true },
  { id: "pinSavedQueue", label: "Pin", icon: "pin", category: "queue", targets: ["savedQueue"], surfaces: ["contextMenu"] },
  { id: "unpinSavedQueue", label: "Unpin", icon: "pinOff", category: "queue", targets: ["savedQueue"], surfaces: ["contextMenu"] },
  { id: "saveQueueAsPlaylist", label: "Save as playlist…", icon: "playlistAdd", category: "queue", targets: ["savedQueue"], surfaces: ["contextMenu"] },
  { id: "deleteSavedQueue", label: "Delete", icon: "trash", category: "queue", targets: ["savedQueue"], surfaces: ["contextMenu"], destructive: true },

  // Global actions (palette)
  { id: "togglePlay", label: "Play / pause", icon: "play_pause", category: "playback", targets: ["none"], surfaces: ["palette", "mediaSession"], defaultShortcut: "Space" },
  { id: "next", label: "Next track", icon: "skip_next", category: "playback", targets: ["none"], surfaces: ["palette", "mediaSession"], defaultShortcut: "Shift+ArrowRight" },
  { id: "previous", label: "Previous track", icon: "skip_previous", category: "playback", targets: ["none"], surfaces: ["palette", "mediaSession"], defaultShortcut: "Shift+ArrowLeft" },
  { id: "seekBackward", label: "Seek back 10 s", icon: "rewind", category: "playback", targets: ["none"], surfaces: ["palette"], defaultShortcut: "ArrowLeft" },
  { id: "seekForward", label: "Seek forward 10 s", icon: "forward", category: "playback", targets: ["none"], surfaces: ["palette"], defaultShortcut: "ArrowRight" },
  { id: "shuffle", label: "Shuffle", icon: "shuffle", category: "playback", targets: ["none"], surfaces: ["palette", "mediaSession"], undoable: true },
  { id: "repeat", label: "Repeat", icon: "repeat", category: "playback", targets: ["none"], surfaces: ["palette", "mediaSession"] },
  { id: "autoplay", label: "Toggle autoplay", icon: "autoplay", category: "playback", targets: ["none"], surfaces: ["palette"] },
  { id: "volumeUp", label: "Volume up", icon: "volume", category: "playback", targets: ["none"], surfaces: ["palette"], defaultShortcut: "Mod+ArrowUp" },
  { id: "volumeDown", label: "Volume down", icon: "volume", category: "playback", targets: ["none"], surfaces: ["palette"], defaultShortcut: "Mod+ArrowDown" },
  { id: "resumeHere", label: "Resume here", icon: "resume", category: "connect", targets: ["none"], surfaces: ["palette"] },
  { id: "stop", label: "Stop", icon: "stop", category: "playback", targets: ["none"], surfaces: ["palette", "mediaSession"] },
  { id: "clearQueue", label: "Clear queue", icon: "trash", category: "queue", targets: ["none"], surfaces: ["palette"], undoable: true },
  { id: "undo", label: "Undo", icon: "undo", category: "edit", targets: ["none"], surfaces: ["palette"], defaultShortcut: "Mod+Z" },
  { id: "redo", label: "Redo", icon: "redo", category: "edit", targets: ["none"], surfaces: ["palette"], defaultShortcut: "Mod+Shift+Z" },
  { id: "openCommandPalette", label: "Command palette", icon: "command", category: "app", targets: ["none"], surfaces: [], defaultShortcut: "Mod+K" },
  { id: "findInList", label: "Search", icon: "search", category: "app", targets: ["none"], surfaces: ["palette"], defaultShortcut: "Mod+F" },
  { id: "toggleQueuePanel", label: "Toggle queue panel", icon: "queue", category: "view", targets: ["none"], surfaces: ["palette"], defaultShortcut: "Q" },
  { id: "toggleLyrics", label: "Toggle lyrics panel", icon: "lyrics", category: "view", targets: ["none"], surfaces: ["palette"], defaultShortcut: "L" },
  { id: "toggleFullscreen", label: "Fullscreen player", icon: "fullscreen", category: "view", targets: ["none"], surfaces: ["palette"], defaultShortcut: "F" },
  { id: "toggleMiniPlayer", label: "Mini player", icon: "mini", category: "view", targets: ["none"], surfaces: ["palette"], defaultShortcut: "M" },
  { id: "handoff", label: "Play on…", icon: "devices", category: "connect", targets: ["none"], surfaces: ["palette"] },
  { id: "sleepTimer", label: "Sleep timer", icon: "sleep", category: "playback", targets: ["none"], surfaces: ["palette"] },
  { id: "navigateSettings", label: "Settings", icon: "settings", category: "app", targets: ["none"], surfaces: ["palette"], defaultShortcut: "Mod+," },
  { id: "ui.newFilter", label: "New filter", icon: "filterAdd", category: "library", targets: ["none"], surfaces: ["palette"] },
  { id: "ui.newPlaylist", label: "New playlist…", icon: "playlistAdd", category: "library", targets: ["none", "tracks"], surfaces: ["palette", "contextMenu"] },
  { id: "copyDiagnostics", label: "Copy diagnostics", icon: "bug", category: "app", targets: ["none"], surfaces: ["palette"] },
  { id: "selectAll", label: "Select all", icon: "selectAll", category: "edit", targets: ["none"], surfaces: [], defaultShortcut: "Mod+A" },
  { id: "remove", label: "Remove from playlist or queue", icon: "remove", category: "edit", targets: ["none"], surfaces: [], defaultShortcut: "Delete" },
  { id: "ui.back", label: "Back", icon: "back", category: "view", targets: ["none"], surfaces: [], defaultShortcut: "Alt+ArrowLeft" },
  { id: "ui.forward", label: "Forward", icon: "forward", category: "view", targets: ["none"], surfaces: [], defaultShortcut: "Alt+ArrowRight" },
  { id: "ui.escape", label: "Close / clear selection", icon: "close", category: "app", targets: ["none"], surfaces: [], defaultShortcut: "Escape" },

  // Sidebar items (choose-and-order)
  { id: "navigateHome", label: "Home", icon: "home", category: "sidebar", targets: ["none"], surfaces: ["sidebar", "palette"] },
  { id: "navigateAlbums", label: "Albums", icon: "album", category: "sidebar", targets: ["none"], surfaces: ["sidebar", "palette"] },
  { id: "navigateArtists", label: "Artists", icon: "artist", category: "sidebar", targets: ["none"], surfaces: ["sidebar", "palette"] },
  { id: "navigatePlaylists", label: "Playlists", icon: "playlist", category: "sidebar", targets: ["none"], surfaces: ["sidebar", "palette"] },
  { id: "navigateTracks", label: "Songs", icon: "song", category: "sidebar", targets: ["none"], surfaces: ["sidebar", "palette"] },
  { id: "navigateGenres", label: "Genres", icon: "genre", category: "sidebar", targets: ["none"], surfaces: ["sidebar", "palette"] },
  { id: "navigateDownloads", label: "Downloads", icon: "download", category: "sidebar", targets: ["none"], surfaces: ["sidebar", "palette"] },
  { id: "navigateFilters", label: "Filters", icon: "filter", category: "sidebar", targets: ["none"], surfaces: ["sidebar", "palette"] },
  { id: "navigateStats", label: "Stats", icon: "stats", category: "sidebar", targets: ["none"], surfaces: ["sidebar", "palette"] },

];

export const DEFAULT_ORDERS: Record<string, string[]> = {
  sidebar: ["navigateHome", "navigateTracks", "navigateAlbums", "navigateArtists", "navigatePlaylists", "navigateGenres", "navigateFilters", "navigateDownloads", "navigateStats"],
  mediaSession: ["previous", "togglePlay", "next", "shuffle", "repeat", "love"],
  contextMenu: ["play", "playShuffled", "playNext", "playLater", "addToPlaylist", "love", "unlove", "rate5", "rate4", "rate3", "rate2", "rate1", "rate0", "download", "unpin", "goToAlbum", "goToArtist", "ui.info", "removeFromQueue", "removeFromPlaylist", "ui.newPlaylist", "ui.renamePlaylist", "deletePlaylist", "restoreSavedQueue", "pinSavedQueue", "unpinSavedQueue", "saveQueueAsPlaylist", "deleteSavedQueue"],
};

export interface ActionContext {
  hasCurrent: boolean;
  canUndo: boolean;
  canRedo: boolean;
  hasResumeOffer: boolean;
  currentLoved?: boolean;
  /** For tracks/albums: whether all targets are already loved / downloaded etc. */
  allLoved?: boolean;
  anyDownloaded?: boolean;
  savedQueuePinned?: boolean;
  inPlaylist?: boolean;
}

export function describeActions(surface: string, target: ActionTarget, order: string[] | undefined, ctx: ActionContext): ActionDescriptor[] {
  const kind = target.type;
  const applicable = REGISTRY.filter((e) => {
    if (surface === "palette") return e.surfaces.includes("palette") && (e.targets.includes("none") || e.targets.includes(kind));
    if (surface === "sidebar" || surface === "mediaSession") return e.surfaces.includes(surface);
    if (surface === "contextMenu") return e.surfaces.includes("contextMenu") && e.targets.includes(kind);
    return e.surfaces.includes(surface as Surface) && e.targets.includes(kind);
  });
  const sequence = order ?? DEFAULT_ORDERS[surface];
  const sorted = sequence
    ? [...applicable].sort((a, b) => {
        const ia = sequence.indexOf(a.id);
        const ib = sequence.indexOf(b.id);
        return (ia === -1 ? 1e6 : ia) - (ib === -1 ? 1e6 : ib);
      })
    : applicable;
  const filtered = sequence && (surface === "sidebar" || surface === "mediaSession") ? sorted.filter((e) => sequence.includes(e.id)) : sorted;
  return filtered
    .filter((e) => {
      if (e.id === "love" && (ctx.allLoved || (kind === "none" && ctx.currentLoved))) return false;
      if (e.id === "unlove" && (ctx.allLoved === false || (kind === "none" && ctx.currentLoved === false))) return false;
      if (e.id === "unpin" && !ctx.anyDownloaded) return false;
      if (e.id === "download" && ctx.anyDownloaded && kind !== "playlists") return false;
      if (e.id === "pinSavedQueue" && ctx.savedQueuePinned) return false;
      if (e.id === "unpinSavedQueue" && !ctx.savedQueuePinned) return false;
      if (e.id === "removeFromPlaylist" && !ctx.inPlaylist) return false;
      if (e.id === "resumeHere" && !ctx.hasResumeOffer) return false;
      return true;
    })
    .map((e) => ({
      id: e.id,
      label: e.label,
      icon: e.icon,
      category: e.category,
      enabled: enabledFor(e.id, ctx),
      defaultShortcut: e.defaultShortcut,
      undoable: e.undoable ?? false,
      destructive: e.destructive ?? false,
    }));
}

function enabledFor(id: string, ctx: ActionContext): boolean {
  if (["next", "previous", "seekBackward", "seekForward", "shuffle", "repeat", "autoplay", "volumeUp", "volumeDown", "stop", "clearQueue"].includes(id)) return ctx.hasCurrent;
  if (id === "togglePlay") return true;
  if (id === "undo") return ctx.canUndo;
  if (id === "redo") return ctx.canRedo;
  return true;
}

/** All actions that carry a default shortcut, for Query.Shortcuts. */
export function shortcutDefaults(): { actionId: string; shortcut: string }[] {
  return REGISTRY.filter((e) => e.defaultShortcut).map((e) => ({ actionId: e.id, shortcut: e.defaultShortcut as string }));
}
