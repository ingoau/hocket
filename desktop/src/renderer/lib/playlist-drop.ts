// Dropping onto a playlist (its sidebar pin, its tile, its open view): songs,
// albums (their songs), other playlists (their songs) and queue rows are added,
// at the drop's place when it has one, with an undoable toast like the menu's
// Add to playlist. Smart playlists and other people's take nothing; which those
// are is read once per drag (a drop target can't wait on a query mid-dragover).
import type { Playlist } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { resolveTrackIds } from "../store/actions";
import { toastUndoable } from "./ui-toast";
import { dropZone, onDragBegan, type DragPayload, type DropProps, type DropSpec } from "./drag";

/** Playlist ids known not to take songs (smart, or someone else's). */
const readOnly = new Set<string>();

function noteWritable(list: Playlist[]): void {
  readOnly.clear();
  for (const p of list) if (p.isSmart || !p.isMine) readOnly.add(p.id);
}

onDragBegan(() => {
  const serverId = useApp.getState().servers[0]?.id;
  if (!serverId) return;
  void bridge()
    .query({ type: "playlists", data: { server_id: serverId } })
    .then((r) => { if (r.type === "playlists") noteWritable(r.data); })
    .catch(() => undefined);
});

/** Whether `p` can be dropped on playlist `playlistId` (pure but for the read-only set; `readOnlyIds` overrides it in tests). */
export function playlistAccepts(p: DragPayload, playlistId: string, readOnlyIds: ReadonlySet<string> = readOnly): boolean {
  if (readOnlyIds.has(playlistId)) return false;
  // A playlist's own rows reorder in its view; onto its pin they'd only duplicate.
  if (p.origin === `playlist:${playlistId}`) return false;
  switch (p.target.type) {
    case "tracks":
    case "albums":
    case "queueItems":
      return true;
    case "playlists":
      return !p.target.data.ids.includes(playlistId);
    default:
      return false;
  }
}

/** Adds what `p` names to the playlist (at `atIndex`, else the end) and says so with an undoable toast. */
export async function dropOnPlaylist(p: DragPayload, playlistId: string, name: string, atIndex?: number): Promise<void> {
  const ids = await resolveTrackIds(p.target);
  if (!ids.length) return;
  toastUndoable(t("toast.addedToPlaylist", { playlist: name }), () =>
    bridge().dispatch({ type: "playlistAdd", data: { playlist_id: playlistId, track_ids: ids, at_index: atIndex } }));
}

/** Drop-zone props for a playlist shown as one item (a pin, a tile). */
export function playlistDropZone(playlistId: string, name: string, extra: Partial<DropSpec> = {}): DropProps {
  return dropZone({
    accepts: (p) => playlistAccepts(p, playlistId),
    onDrop: (p) => void dropOnPlaylist(p, playlistId, name),
    ...extra,
  });
}
