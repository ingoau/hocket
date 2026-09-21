// Executes an action id against a target. `ui.*` ids are renderer-side
// (navigation, dialogs, windows); everything else goes to the core through
// Command.RunAction so the registry stays the single source of truth.
import type { ActionTarget } from "@core/api";
import { bridge } from "../core/bridge";
import { useApp } from "./app";
import { explicitIds } from "./selection";
import { t } from "@shared/strings";

export interface ActionContext {
  playlistId?: string;
  /** Row indices for PlaylistRemove. */
  indices?: number[];
  savedQueueId?: string;
}

export function targetTrackIds(target: ActionTarget): string[] {
  return target.type === "tracks" ? target.data.ids : [];
}

export async function executeAction(actionId: string, target: ActionTarget = { type: "none" }, ctx: ActionContext = {}): Promise<void> {
  const app = useApp.getState();
  const b = bridge();
  if (!actionId.startsWith("ui.")) {
    if (actionId.startsWith("nav.")) {
      app.navigate({ view: actionId.slice(4) as never });
      return;
    }
    app.runAction(actionId, target);
    return;
  }
  switch (actionId) {
    case "ui.palette":
      app.setPaletteOpen(!app.paletteOpen);
      return;
    case "ui.search":
      app.focusSearch();
      return;
    case "ui.queue":
      app.setPanels(app.panels.rightOpen && !app.panels.queueCollapsed ? { queueCollapsed: true } : { rightOpen: true, queueCollapsed: false });
      return;
    case "ui.lyrics":
      app.setPanels(app.panels.rightOpen && !app.panels.lyricsCollapsed ? { lyricsCollapsed: true } : { rightOpen: true, lyricsCollapsed: false });
      return;
    case "ui.fullscreen":
      app.setFullscreen(!app.fullscreen);
      return;
    case "ui.miniPlayer":
      b.window.openMiniPlayer();
      return;
    case "ui.settings":
      app.navigate({ view: "settings" });
      return;
    case "ui.openDownloads":
      app.navigate({ view: "downloads" });
      return;
    case "ui.openStats":
      app.navigate({ view: "stats" });
      return;
    case "ui.openFilters":
      app.navigate({ view: "filters" });
      return;
    case "ui.newFilter":
      app.navigate({ view: "filter", id: "new" });
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
      else app.clearSelection();
      return;
    case "ui.selectAll":
      // Handled by the focused list (it knows the count); nothing global.
      return;
    case "ui.delete":
      return;
    case "ui.playOn":
      b.dispatch({ type: "openHandoffPicker" });
      app.openDialog({ kind: "connect" });
      return;
    case "ui.sleepTimer":
      app.openDialog({ kind: "sleepTimer" });
      return;
    case "ui.copyDiagnostics": {
      const r = await b.query({ type: "diagnostics" });
      if (r.type === "text") {
        b.clipboard.writeText(r.data);
        app.applyEvent({ type: "toast", data: { toast: { id: `diag-${Date.now()}`, message: t("settings.diagnosticsCopied"), actionLabel: undefined, actionCommand: undefined, durationMs: 3000 } } });
      }
      return;
    }
    case "ui.goToAlbum": {
      const id = await firstTrackField(target, "albumId");
      if (id) app.navigate({ view: "album", id });
      return;
    }
    case "ui.goToArtist": {
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
    case "ui.addToPlaylist": {
      const ids = await resolveTrackIds(target);
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
    case "ui.removeFromPlaylist": {
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
    case "ui.deletePlaylist": {
      if (target.type !== "playlists") return;
      for (const id of target.data.ids) {
        const r = await b.query({ type: "playlist", data: { id } });
        const name = r.type === "playlistDetail" ? (r.data?.name ?? id) : id;
        app.openDialog({ kind: "confirm", title: t("action.deletePlaylist"), message: t("dialog.deletePlaylist", { name }), confirmLabel: t("dialog.delete"), destructive: true, onConfirm: () => b.dispatch({ type: "deletePlaylist", data: { playlist_id: id } }) });
      }
      return;
    }
    case "ui.removeDownload": {
      const pinTarget = target.type === "albums" && target.data.ids[0] ? { type: "album" as const, data: { id: target.data.ids[0] } } : target.type === "playlists" && target.data.ids[0] ? { type: "playlist" as const, data: { id: target.data.ids[0] } } : target.type === "tracks" && target.data.ids[0] ? { type: "track" as const, data: { id: target.data.ids[0] } } : undefined;
      if (!pinTarget) return;
      app.openDialog({ kind: "confirm", title: t("action.removeDownload"), message: t("dialog.deleteDownload", { name: pinTarget.data.id }), confirmLabel: t("dialog.delete"), destructive: true, onConfirm: () => b.dispatch({ type: "unpin", data: { target: pinTarget } }) });
      return;
    }
    case "ui.saveQueueAsPlaylist": {
      const sq = ctx.savedQueueId ?? (target.type === "savedQueue" ? target.data.id : undefined);
      app.openDialog({ kind: "prompt", title: t("dialog.saveQueueAsPlaylist"), label: t("dialog.playlistName"), onConfirm: (name) => b.dispatch({ type: "saveQueueAsPlaylist", data: { saved_queue_id: sq, name } }) });
      return;
    }
    default:
      console.warn("[actions] unhandled ui action", actionId);
  }
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
