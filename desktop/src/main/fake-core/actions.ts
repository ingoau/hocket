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
  { id: "shuffle", label: "Shuffle", icon: "shuffle", category: "playback", targets: ["albums", "artists", "playlists"], surfaces: ["contextMenu"] },
  { id: "playNext", label: "Play next", icon: "playNext", category: "queue", targets: ["tracks", "albums", "artists", "playlists", "queueItems"], surfaces: ["contextMenu"], undoable: true },
  { id: "playLater", label: "Play later", icon: "playLater", category: "queue", targets: ["tracks", "albums", "artists", "playlists"], surfaces: ["contextMenu"], undoable: true },
  { id: "ui.addToPlaylist", label: "Add to playlist…", icon: "playlistAdd", category: "library", targets: ["tracks", "albums", "queueItems"], surfaces: ["contextMenu"], undoable: true },
  { id: "love", label: "Love", icon: "heart", category: "library", targets: ["tracks", "albums", "artists", "queueItems"], surfaces: ["contextMenu"], undoable: true },
  { id: "unlove", label: "Unlove", icon: "heartOff", category: "library", targets: ["tracks", "albums", "artists", "queueItems"], surfaces: ["contextMenu"], undoable: true },
  { id: "rate.5", label: "Rate ★★★★★", icon: "star", category: "library", targets: ["tracks", "albums", "queueItems"], surfaces: ["contextMenu"], undoable: true, defaultShortcut: "5" },
  { id: "rate.4", label: "Rate ★★★★", icon: "star", category: "library", targets: ["tracks", "albums", "queueItems"], surfaces: ["contextMenu"], undoable: true, defaultShortcut: "4" },
  { id: "rate.3", label: "Rate ★★★", icon: "star", category: "library", targets: ["tracks", "albums", "queueItems"], surfaces: ["contextMenu"], undoable: true, defaultShortcut: "3" },
  { id: "rate.2", label: "Rate ★★", icon: "star", category: "library", targets: ["tracks", "albums", "queueItems"], surfaces: ["contextMenu"], undoable: true, defaultShortcut: "2" },
  { id: "rate.1", label: "Rate ★", icon: "star", category: "library", targets: ["tracks", "albums", "queueItems"], surfaces: ["contextMenu"], undoable: true, defaultShortcut: "1" },
  { id: "rate.0", label: "Clear rating", icon: "starOff", category: "library", targets: ["tracks", "albums", "queueItems"], surfaces: ["contextMenu"], undoable: true, defaultShortcut: "0" },
  { id: "download", label: "Download", icon: "download", category: "offline", targets: ["tracks", "albums", "playlists"], surfaces: ["contextMenu"] },
  { id: "ui.removeDownload", label: "Remove download", icon: "downloadOff", category: "offline", targets: ["tracks", "albums", "playlists"], surfaces: ["contextMenu"], destructive: true },
  { id: "ui.goToAlbum", label: "Go to album", icon: "album", category: "navigate", targets: ["tracks", "queueItems"], surfaces: ["contextMenu"] },
  { id: "ui.goToArtist", label: "Go to artist", icon: "artist", category: "navigate", targets: ["tracks", "albums", "queueItems"], surfaces: ["contextMenu"] },
  { id: "ui.info", label: "Details", icon: "info", category: "navigate", targets: ["tracks"], surfaces: ["contextMenu"] },
  { id: "removeFromQueue", label: "Remove from queue", icon: "remove", category: "queue", targets: ["queueItems"], surfaces: ["contextMenu"], undoable: true },
  { id: "ui.removeFromPlaylist", label: "Remove from playlist", icon: "remove", category: "library", targets: ["tracks"], surfaces: ["contextMenu"], undoable: true },
  { id: "ui.renamePlaylist", label: "Rename playlist…", icon: "edit", category: "library", targets: ["playlists"], surfaces: ["contextMenu"] },
  { id: "ui.deletePlaylist", label: "Delete playlist", icon: "trash", category: "library", targets: ["playlists"], surfaces: ["contextMenu"], destructive: true },
  { id: "restoreSavedQueue", label: "Restore", icon: "restore", category: "queue", targets: ["savedQueue"], surfaces: ["contextMenu"], undoable: true },
  { id: "pinSavedQueue", label: "Pin", icon: "pin", category: "queue", targets: ["savedQueue"], surfaces: ["contextMenu"] },
  { id: "unpinSavedQueue", label: "Unpin", icon: "pinOff", category: "queue", targets: ["savedQueue"], surfaces: ["contextMenu"] },
  { id: "ui.saveQueueAsPlaylist", label: "Save as playlist…", icon: "playlistAdd", category: "queue", targets: ["savedQueue"], surfaces: ["contextMenu"] },
  { id: "deleteSavedQueue", label: "Delete", icon: "trash", category: "queue", targets: ["savedQueue"], surfaces: ["contextMenu"], destructive: true },

  // Global actions (palette)
  { id: "transport.togglePlay", label: "Play / pause", icon: "play", category: "playback", targets: ["none"], surfaces: ["palette"], defaultShortcut: "Space" },
  { id: "transport.next", label: "Next track", icon: "next", category: "playback", targets: ["none"], surfaces: ["palette"], defaultShortcut: "Shift+ArrowRight" },
  { id: "transport.previous", label: "Previous track", icon: "previous", category: "playback", targets: ["none"], surfaces: ["palette"], defaultShortcut: "Shift+ArrowLeft" },
  { id: "transport.seekBack", label: "Seek back 10 s", icon: "rewind", category: "playback", targets: ["none"], surfaces: ["palette"], defaultShortcut: "ArrowLeft" },
  { id: "transport.seekForward", label: "Seek forward 10 s", icon: "forward", category: "playback", targets: ["none"], surfaces: ["palette"], defaultShortcut: "ArrowRight" },
  { id: "transport.shuffle", label: "Toggle shuffle", icon: "shuffle", category: "playback", targets: ["none"], surfaces: ["palette"], undoable: true, defaultShortcut: "S" },
  { id: "transport.repeat", label: "Cycle repeat", icon: "repeat", category: "playback", targets: ["none"], surfaces: ["palette"], defaultShortcut: "R" },
  { id: "transport.toggleAutoplay", label: "Toggle autoplay", icon: "autoplay", category: "playback", targets: ["none"], surfaces: ["palette"] },
  { id: "transport.volumeUp", label: "Volume up", icon: "volume", category: "playback", targets: ["none"], surfaces: ["palette"], defaultShortcut: "Mod+ArrowUp" },
  { id: "transport.volumeDown", label: "Volume down", icon: "volume", category: "playback", targets: ["none"], surfaces: ["palette"], defaultShortcut: "Mod+ArrowDown" },
  { id: "transport.resumeHere", label: "Resume here", icon: "resume", category: "connect", targets: ["none"], surfaces: ["palette"] },
  { id: "track.love", label: "Love current track", icon: "heart", category: "library", targets: ["none"], surfaces: ["palette"], undoable: true, defaultShortcut: "Mod+L" },
  { id: "clearQueue", label: "Clear queue", icon: "trash", category: "queue", targets: ["none"], surfaces: ["palette"], undoable: true },
  { id: "undo", label: "Undo", icon: "undo", category: "edit", targets: ["none"], surfaces: ["palette"], defaultShortcut: "Mod+Z" },
  { id: "redo", label: "Redo", icon: "redo", category: "edit", targets: ["none"], surfaces: ["palette"], defaultShortcut: "Mod+Shift+Z" },
  { id: "ui.palette", label: "Command palette", icon: "command", category: "app", targets: ["none"], surfaces: [], defaultShortcut: "Mod+K" },
  { id: "ui.search", label: "Search", icon: "search", category: "app", targets: ["none"], surfaces: ["palette"], defaultShortcut: "Mod+F" },
  { id: "ui.queue", label: "Toggle queue panel", icon: "queue", category: "view", targets: ["none"], surfaces: ["palette"], defaultShortcut: "Q" },
  { id: "ui.lyrics", label: "Toggle lyrics panel", icon: "lyrics", category: "view", targets: ["none"], surfaces: ["palette"], defaultShortcut: "L" },
  { id: "ui.fullscreen", label: "Fullscreen player", icon: "fullscreen", category: "view", targets: ["none"], surfaces: ["palette"], defaultShortcut: "F" },
  { id: "ui.miniPlayer", label: "Mini player", icon: "mini", category: "view", targets: ["none"], surfaces: ["palette"], defaultShortcut: "M" },
  { id: "ui.playOn", label: "Play on…", icon: "devices", category: "connect", targets: ["none"], surfaces: ["palette"] },
  { id: "ui.sleepTimer", label: "Sleep timer", icon: "sleep", category: "playback", targets: ["none"], surfaces: ["palette"] },
  { id: "ui.settings", label: "Settings", icon: "settings", category: "app", targets: ["none"], surfaces: ["palette"], defaultShortcut: "Mod+," },
  { id: "ui.openDownloads", label: "Open downloads", icon: "download", category: "navigate", targets: ["none"], surfaces: ["palette"] },
  { id: "ui.openStats", label: "Open stats", icon: "stats", category: "navigate", targets: ["none"], surfaces: ["palette"] },
  { id: "ui.openFilters", label: "Open filters", icon: "filter", category: "navigate", targets: ["none"], surfaces: ["palette"] },
  { id: "ui.newFilter", label: "New filter", icon: "filterAdd", category: "library", targets: ["none"], surfaces: ["palette"] },
  { id: "ui.newPlaylist", label: "New playlist…", icon: "playlistAdd", category: "library", targets: ["none", "tracks"], surfaces: ["palette", "contextMenu"] },
  { id: "ui.copyDiagnostics", label: "Copy diagnostics", icon: "bug", category: "app", targets: ["none"], surfaces: ["palette"] },
  { id: "ui.selectAll", label: "Select all", icon: "selectAll", category: "edit", targets: ["none"], surfaces: [], defaultShortcut: "Mod+A" },
  { id: "ui.delete", label: "Remove from playlist or queue", icon: "remove", category: "edit", targets: ["none"], surfaces: [], defaultShortcut: "Delete" },
  { id: "ui.back", label: "Back", icon: "back", category: "view", targets: ["none"], surfaces: [], defaultShortcut: "Alt+ArrowLeft" },
  { id: "ui.forward", label: "Forward", icon: "forward", category: "view", targets: ["none"], surfaces: [], defaultShortcut: "Alt+ArrowRight" },
  { id: "ui.escape", label: "Close / clear selection", icon: "close", category: "app", targets: ["none"], surfaces: [], defaultShortcut: "Escape" },

  // Sidebar items (choose-and-order)
  { id: "nav.home", label: "Home", icon: "home", category: "sidebar", targets: ["none"], surfaces: ["sidebar"] },
  { id: "nav.albums", label: "Albums", icon: "album", category: "sidebar", targets: ["none"], surfaces: ["sidebar"] },
  { id: "nav.artists", label: "Artists", icon: "artist", category: "sidebar", targets: ["none"], surfaces: ["sidebar"] },
  { id: "nav.playlists", label: "Playlists", icon: "playlist", category: "sidebar", targets: ["none"], surfaces: ["sidebar"] },
  { id: "nav.songs", label: "Songs", icon: "song", category: "sidebar", targets: ["none"], surfaces: ["sidebar"] },
  { id: "nav.genres", label: "Genres", icon: "genre", category: "sidebar", targets: ["none"], surfaces: ["sidebar"] },
  { id: "nav.downloads", label: "Downloads", icon: "download", category: "sidebar", targets: ["none"], surfaces: ["sidebar"] },
  { id: "nav.filters", label: "Filters", icon: "filter", category: "sidebar", targets: ["none"], surfaces: ["sidebar"] },
  { id: "nav.stats", label: "Stats", icon: "stats", category: "sidebar", targets: ["none"], surfaces: ["sidebar"] },

  // Media session buttons (choose-and-order)
  { id: "ms.play", label: "Play / pause", icon: "play", category: "mediaSession", targets: ["none"], surfaces: ["mediaSession"] },
  { id: "ms.previous", label: "Previous", icon: "previous", category: "mediaSession", targets: ["none"], surfaces: ["mediaSession"] },
  { id: "ms.next", label: "Next", icon: "next", category: "mediaSession", targets: ["none"], surfaces: ["mediaSession"] },
  { id: "ms.seek", label: "Seek", icon: "seek", category: "mediaSession", targets: ["none"], surfaces: ["mediaSession"] },
  { id: "ms.shuffle", label: "Shuffle", icon: "shuffle", category: "mediaSession", targets: ["none"], surfaces: ["mediaSession"] },
  { id: "ms.repeat", label: "Repeat", icon: "repeat", category: "mediaSession", targets: ["none"], surfaces: ["mediaSession"] },
  { id: "ms.love", label: "Love", icon: "heart", category: "mediaSession", targets: ["none"], surfaces: ["mediaSession"] },
  { id: "ms.rate", label: "Rate", icon: "star", category: "mediaSession", targets: ["none"], surfaces: ["mediaSession"] },
  { id: "ms.stop", label: "Stop", icon: "stop", category: "mediaSession", targets: ["none"], surfaces: ["mediaSession"] },
];

export const DEFAULT_ORDERS: Record<string, string[]> = {
  sidebar: ["nav.home", "nav.albums", "nav.artists", "nav.playlists", "nav.songs", "nav.genres", "nav.downloads", "nav.filters", "nav.stats"],
  mediaSession: ["ms.previous", "ms.play", "ms.next", "ms.seek", "ms.shuffle", "ms.repeat", "ms.love"],
  contextMenu: ["play", "shuffle", "playNext", "playLater", "ui.addToPlaylist", "love", "unlove", "rate.5", "rate.4", "rate.3", "rate.2", "rate.1", "rate.0", "download", "ui.removeDownload", "ui.goToAlbum", "ui.goToArtist", "ui.info", "removeFromQueue", "ui.removeFromPlaylist", "ui.newPlaylist", "ui.renamePlaylist", "ui.deletePlaylist", "restoreSavedQueue", "pinSavedQueue", "unpinSavedQueue", "ui.saveQueueAsPlaylist", "deleteSavedQueue"],
};

export interface ActionContext {
  hasCurrent: boolean;
  canUndo: boolean;
  canRedo: boolean;
  hasResumeOffer: boolean;
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
      if (e.id === "love" && ctx.allLoved) return false;
      if (e.id === "unlove" && ctx.allLoved === false) return false;
      if (e.id === "ui.removeDownload" && !ctx.anyDownloaded) return false;
      if (e.id === "download" && ctx.anyDownloaded && kind !== "playlists") return false;
      if (e.id === "pinSavedQueue" && ctx.savedQueuePinned) return false;
      if (e.id === "unpinSavedQueue" && !ctx.savedQueuePinned) return false;
      if (e.id === "ui.removeFromPlaylist" && !ctx.inPlaylist) return false;
      if (e.id === "transport.resumeHere" && !ctx.hasResumeOffer) return false;
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
  if (id.startsWith("transport.") && id !== "transport.resumeHere") return ctx.hasCurrent || id === "transport.togglePlay";
  if (id === "track.love" || id === "clearQueue") return ctx.hasCurrent;
  if (id === "undo") return ctx.canUndo;
  if (id === "redo") return ctx.canRedo;
  return true;
}

/** All actions that carry a default shortcut, for Query.Shortcuts. */
export function shortcutDefaults(): { actionId: string; shortcut: string }[] {
  return REGISTRY.filter((e) => e.defaultShortcut).map((e) => ({ actionId: e.id, shortcut: e.defaultShortcut as string }));
}
