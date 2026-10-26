// Executes an action id against a target. Ids are the registry's canonical
// ids (aliases are normalised first). Actions that need UI — a dialog, a
// navigation, a window — run here; everything else goes to the core through
// Command.RunAction so the registry stays the single source of truth.
import type { ActionTarget } from "@core/api";
import { NAV_VIEWS, canonicalActionId } from "@shared/keymap";
import { SK } from "@shared/settings-keys";
import { t } from "@shared/strings";
import { bridge } from "../core/bridge";
import { isNarrow, showSideView, toggleSideView, useApp, type DialogState, type ViewName } from "./app";
import { settingValue } from "./reducer";
import { explicitIds } from "./selection";

export interface ActionContext {
  playlistId?: string;
  /** Row indices for PlaylistRemove. */
  indices?: number[];
  savedQueueId?: string;
  /** The button that ran it, for surfaces that open beside it (the playlist picker). */
  anchor?: HTMLElement;
}

/** Ids the renderer executes itself (they need UI); everything else is RunAction. */
const RENDERER_HANDLED = new Set([
  "openCommandPalette", "findInList", "toggleQueuePanel", "toggleLyrics", "toggleFullscreen", "toggleMiniPlayer",
  "navigateRecent", "ui.back", "ui.forward", "ui.escape", "selectAll", "remove", "handoff", "sleepTimer", "copyDiagnostics",
  "goToAlbum", "goToArtist", "ui.info", "addToPlaylist", "ui.newPlaylist", "removeFromPlaylist", "ui.renamePlaylist",
  "deletePlaylist", "unpin", "saveQueueAsPlaylist", "ui.developerDiagnostics",
]);

export async function executeAction(rawId: string, target: ActionTarget = { type: "none" }, ctx: ActionContext = {}): Promise<void> {
  const actionId = canonicalActionId(rawId);
  const app = useApp.getState();
  const b = bridge();
  const view = NAV_VIEWS[actionId];
  if (view) {
    app.navigate({ view: view as ViewName });
    return;
  }
  if (!RENDERER_HANDLED.has(actionId)) {
    app.runAction(actionId, target);
    return;
  }
  switch (actionId) {
    case "openCommandPalette":
      app.setPaletteOpen(!app.paletteOpen);
      return;
    case "findInList": {
      // On Settings, Find searches the settings; everywhere else, the library.
      const settingsSearch = app.route.view === "settings" ? document.querySelector<HTMLInputElement>('[data-testid="settings-search"]') : null;
      if (settingsSearch) { settingsSearch.focus(); settingsSearch.select(); return; }
      app.focusSearch();
      return;
    }
    case "toggleQueuePanel":
      // In the fullscreen player Q and L switch its stage instead of the hidden side panel.
      if (app.fullscreen) {
        app.setNowPlayingMode(app.nowPlayingMode === "queue" ? "art" : "queue");
        return;
      }
      toggleSideView("queue");
      return;
    case "toggleLyrics":
      if (app.fullscreen) {
        app.setNowPlayingMode(app.nowPlayingMode === "lyrics" ? "art" : "lyrics");
        return;
      }
      toggleSideView("lyrics");
      return;
    case "toggleFullscreen":
      app.setFullscreen(!app.fullscreen);
      return;
    case "toggleMiniPlayer":
      b.window.openMiniPlayer();
      return;
    case "navigateRecent":
      showSideView("recent");
      return;
    case "ui.back":
      app.back();
      return;
    case "ui.forward":
      app.forward();
      return;
    case "ui.escape":
      if (app.contextMenu) app.closeContextMenu();
      else if (app.dialog) app.closeDialog();
      else if (app.paletteOpen) app.setPaletteOpen(false);
      else if (app.fullscreen) app.setFullscreen(false);
      else if (app.drawerOpen && isNarrow()) app.setDrawerOpen(false);
      else app.clearSelection();
      return;
    case "selectAll":
    case "remove":
      // Handled by the focused list (it knows the count / the containing list).
      return;
    case "handoff":
      b.dispatch({ type: "openHandoffPicker" });
      app.openDialog({ kind: "connect" });
      return;
    case "sleepTimer":
      app.openDialog({ kind: "sleepTimer" });
      return;
    case "ui.developerDiagnostics":
      // The shortcut is developer mode's; the page itself is always in Settings → Advanced.
      if (settingValue<boolean>(app, SK.developerMode, false)) app.navigate({ view: "developer" });
      return;
    case "copyDiagnostics": {
      const r = await b.query({ type: "diagnostics" });
      if (r.type === "text") b.clipboard.writeText(r.data);
      return;
    }
    case "goToAlbum": {
      const id = target.type === "none" ? app.nowPlaying?.track.albumId : await firstTrackField(target, "albumId");
      if (id) app.navigate({ view: "album", id });
      return;
    }
    case "goToArtist": {
      if (target.type === "none") {
        if (app.nowPlaying?.track.artistId) app.navigate({ view: "artist", id: app.nowPlaying.track.artistId });
        return;
      }
      if (target.type === "albums" && target.data.ids[0]) {
        const r = await b.query({ type: "album", data: { id: target.data.ids[0] } });
        if (r.type === "albumDetail" && r.data?.artistId) app.navigate({ view: "artist", id: r.data.artistId });
        return;
      }
      const id = await firstTrackField(target, "artistId");
      if (id) app.navigate({ view: "artist", id });
      return;
    }
    case "ui.info": {
      if (target.type === "albums") {
        if (target.data.ids[0]) app.openDialog({ kind: "albumInfo", albumId: target.data.ids[0] });
        return;
      }
      const ids = await resolveTrackIds(target);
      if (ids[0]) app.openDialog({ kind: "trackInfo", trackId: ids[0] });
      return;
    }
    case "addToPlaylist": {
      // The + again closes the picker it opened.
      if (ctx.anchor && app.playlistPicker) {
        app.closePlaylistPicker();
        return;
      }
      const ids = target.type === "none" && app.nowPlaying ? [app.nowPlaying.track.id] : await resolveTrackIds(target);
      if (ids.length) app.openPlaylistPicker({ trackIds: ids, anchor: pickerAnchor(ctx.anchor) });
      return;
    }
    case "ui.newPlaylist": {
      const ids = await resolveTrackIds(target);
      const serverId = app.servers[0]?.id;
      if (!serverId) return;
      app.openDialog({
        kind: "prompt",
        title: t("dialog.newPlaylist"),
        label: t("dialog.playlistName"),
        onConfirm: (name) => b.dispatch({ type: "createPlaylist", data: { server_id: serverId, name, track_ids: ids } }),
      });
      return;
    }
    case "removeFromPlaylist": {
      if (!ctx.playlistId || !ctx.indices?.length) return;
      b.dispatch({ type: "playlistRemove", data: { playlist_id: ctx.playlistId, indices: ctx.indices } });
      return;
    }
    case "ui.renamePlaylist": {
      if (target.type !== "playlists" || !target.data.ids[0]) return;
      const id = target.data.ids[0];
      const r = await b.query({ type: "playlist", data: { id } });
      const current = r.type === "playlistDetail" ? r.data?.name : undefined;
      app.openDialog({ kind: "prompt", title: t("dialog.renamePlaylist"), label: t("dialog.playlistName"), initial: current, onConfirm: (name) => b.dispatch({ type: "renamePlaylist", data: { playlist_id: id, name, comment: undefined, public: undefined } }) });
      return;
    }
    case "deletePlaylist": {
      if (target.type !== "playlists" || !target.data.ids.length) return;
      // One confirm for the whole selection (a dialog per id would replace
      // the previous one and only the last playlist would be deleted).
      const names: string[] = [];
      for (const id of target.data.ids) {
        const r = await b.query({ type: "playlist", data: { id } });
        names.push(r.type === "playlistDetail" ? (r.data?.name ?? id) : id);
      }
      app.openDialog(deletePlaylistsDialog(target.data.ids, names, (id) => b.dispatch({ type: "deletePlaylist", data: { playlist_id: id } })));
      return;
    }
    case "unpin": {
      const pinTarget = target.type === "albums" && target.data.ids[0] ? { type: "album" as const, data: { id: target.data.ids[0] } } : target.type === "playlists" && target.data.ids[0] ? { type: "playlist" as const, data: { id: target.data.ids[0] } } : target.type === "tracks" && target.data.ids[0] ? { type: "track" as const, data: { id: target.data.ids[0] } } : undefined;
      if (!pinTarget) return;
      app.openDialog({ kind: "confirm", title: t("action.removeDownload"), message: t("dialog.deleteDownload", { name: pinTarget.data.id }), confirmLabel: t("dialog.delete"), destructive: true, onConfirm: () => b.dispatch({ type: "unpin", data: { target: pinTarget } }) });
      return;
    }
    case "saveQueueAsPlaylist": {
      const sq = ctx.savedQueueId ?? (target.type === "savedQueue" ? target.data.id : undefined);
      app.openDialog({ kind: "prompt", title: t("dialog.saveQueueAsPlaylist"), label: t("dialog.playlistName"), onConfirm: (name) => b.dispatch({ type: "saveQueueAsPlaylist", data: { saved_queue_id: sq, name } }) });
      return;
    }
    default:
      console.warn("[actions] unhandled ui action", actionId);
  }
}

/** The single confirm dialog for deleting `ids` (named `names`); confirming deletes every one. */
export function deletePlaylistsDialog(ids: string[], names: string[], deleteOne: (id: string) => void): Extract<DialogState, { kind: "confirm" }> {
  const message = ids.length === 1 ? t("dialog.deletePlaylist", { name: names[0] ?? ids[0] ?? "" }) : t("dialog.deletePlaylists", { n: ids.length, names: names.map((n) => `“${n}”`).join(", ") });
  return { kind: "confirm", title: t("action.deletePlaylist"), message, confirmLabel: t("dialog.delete"), destructive: true, onConfirm: () => ids.forEach(deleteOne) };
}

/** The songs a target stands for (an album's, a playlist's, the queue items'). */
export async function resolveTrackIds(target: ActionTarget): Promise<string[]> {
  const b = bridge();
  switch (target.type) {
    case "tracks":
      return target.data.ids;
    case "albums": {
      const out: string[] = [];
      for (const id of target.data.ids) {
        const r = await b.query({ type: "albumTracks", data: { id } });
        if (r.type === "trackList") out.push(...r.data.map((x) => x.id));
      }
      return out;
    }
    case "playlists": {
      const out: string[] = [];
      for (const id of target.data.ids) {
        const r = await b.query({ type: "playlistTracks", data: { id, page: { offset: 0, limit: 10_000 } } });
        if (r.type === "tracks") out.push(...r.data.items.map((x) => x.id));
      }
      return out;
    }
    case "queueItems": {
      const q = useApp.getState().queue;
      const all = [...q.history, ...(q.current ? [q.current] : []), ...q.playingNext, ...q.upcoming];
      return target.data.keys.map((k) => all.find((e) => e.item.key === k)?.track.id).filter((x): x is string => !!x);
    }
    default:
      return [];
  }
}

async function firstTrackField(target: ActionTarget, field: "albumId" | "artistId"): Promise<string | undefined> {
  const ids = await resolveTrackIds(target);
  if (!ids[0]) return undefined;
  const r = await bridge().query({ type: "track", data: { id: ids[0] } });
  return r.type === "trackDetail" ? r.data?.[field] : undefined;
}

/** Build the ActionTarget for the current selection in a list of tracks. */
export function selectionTarget(kind: "tracks" | "albums" | "artists" | "playlists", fallbackId?: string): ActionTarget | undefined {
  const sel = useApp.getState().selection;
  const ids = explicitIds(sel);
  if (ids && ids.length) return { type: kind, data: { ids } } as ActionTarget;
  if (fallbackId) return { type: kind, data: { ids: [fallbackId] } } as ActionTarget;
  return undefined;
}

/**
 * Where the playlist picker opens: beside the button that ran it, else the focused item (a shortcut
 * on a row), else the player bar's own +, else the middle of the window.
 */
function pickerAnchor(el?: HTMLElement) {
  const focused = document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null;
  const from = [el, focused, document.querySelector<HTMLElement>('[data-testid="bar-add"]')].find((e) => e?.isConnected && e.getClientRects().length);
  if (from) {
    const r = from.getBoundingClientRect();
    return { left: r.left, right: r.right, top: r.top, bottom: r.bottom };
  }
  const x = window.innerWidth / 2;
  const y = window.innerHeight / 3;
  return { left: x, right: x, top: y, bottom: y };
}
