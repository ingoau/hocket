// Executes an action id against a target. Ids are the registry's canonical
// ids (aliases are normalised first). Actions that need UI — a dialog, a
// navigation, a window — run here; everything else goes to the core through
// Command.RunAction so the registry stays the single source of truth.
import type { ActionTarget } from "@core/api";
import { NAV_VIEWS, canonicalActionId } from "@shared/keymap";
import { t } from "@shared/strings";
import { bridge } from "../core/bridge";
import { isNarrow, useApp, type DialogState, type ViewName } from "./app";
import { explicitIds } from "./selection";

export interface ActionContext {
  playlistId?: string;
  /** Row indices for PlaylistRemove. */
  indices?: number[];
  savedQueueId?: string;
}

/** Ids the renderer executes itself (they need UI); everything else is RunAction. */
const RENDERER_HANDLED = new Set([
  "openCommandPalette", "findInList", "toggleQueuePanel", "toggleLyrics", "toggleFullscreen", "toggleMiniPlayer",
  "navigateRecent", "ui.back", "ui.forward", "ui.escape", "selectAll", "remove", "handoff", "sleepTimer", "copyDiagnostics",
  "goToAlbum", "goToArtist", "ui.info", "addToPlaylist", "ui.newPlaylist", "removeFromPlaylist", "ui.renamePlaylist",
  "deletePlaylist", "unpin", "saveQueueAsPlaylist",
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
    case "findInList":
      app.focusSearch();
      return;
    case "toggleQueuePanel":
      if (isNarrow()) {
        // The drawer: open it on the queue, or close it when the queue is already showing.
        const showing = app.drawerOpen && !app.panels.queueCollapsed;
        app.setDrawerOpen(!showing);
        if (!showing) app.setPanels({ queueCollapsed: false });
        return;
      }
      app.setPanels(app.panels.rightOpen && !app.panels.queueCollapsed ? { queueCollapsed: true } : { rightOpen: true, queueCollapsed: false });
      return;
    case "toggleLyrics":
      if (isNarrow()) {
        const showing = app.drawerOpen && !app.panels.lyricsCollapsed;
        app.setDrawerOpen(!showing);
        if (!showing) app.setPanels({ lyricsCollapsed: false });
        return;
      }
      app.setPanels(app.panels.rightOpen && !app.panels.lyricsCollapsed ? { lyricsCollapsed: true } : { rightOpen: true, lyricsCollapsed: false });
      return;
    case "toggleFullscreen":
      app.setFullscreen(!app.fullscreen);
      return;
    case "toggleMiniPlayer":
      b.window.openMiniPlayer();
      return;
    case "navigateRecent":
      if (isNarrow()) app.setDrawerOpen(true);
      app.setPanels({ rightOpen: true, queueCollapsed: false });
      app.setQueueTab("recent");
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
    case "copyDiagnostics": {
      const r = await b.query({ type: "diagnostics" });
      if (r.type === "text") {
        b.clipboard.writeText(r.data);
        app.applyEvent({ type: "toast", data: { toast: { id: `diag-${Date.now()}`, message: t("settings.diagnosticsCopied"), actionLabel: undefined, actionCommand: undefined, durationMs: 3000 } } });
      }
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
      const ids = await resolveTrackIds(target);
      if (ids[0]) app.openDialog({ kind: "trackInfo", trackId: ids[0] });
      return;
    }
    case "addToPlaylist": {
      const ids = target.type === "none" && app.nowPlaying ? [app.nowPlaying.track.id] : await resolveTrackIds(target);
      if (ids.length) app.openDialog({ kind: "addToPlaylist", trackIds: ids });
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

async function resolveTrackIds(target: ActionTarget): Promise<string[]> {
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
