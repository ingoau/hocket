// Dropping onto a playlist (its sidebar entry, its tile, its open view): songs,
// albums (their songs), other playlists (their songs) and queue rows are added,
// at the drop's place when it has one, with an undoable toast like the menu's
// Add to playlist. Smart playlists and other people's take nothing: the drop
// targets learn which those are when a drag starts (a dragover can't wait on a
// query), and the drop itself checks the playlist again before adding.
import type { Playlist } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { resolveTrackRefs } from "../store/actions";
import { toastUndoable } from "./ui-toast";
import { dropZone, onDragBegan, type DragPayload, type DropProps } from "./drag";

/** Playlist ids known not to take songs (smart, or someone else's). */
const readOnly = new Set<string>();

function writable(p: Pick<Playlist, "isSmart" | "isMine">): boolean {
  return !p.isSmart && p.isMine;
}

function refreshReadOnly(): void {
  const servers = useApp.getState().accounts;
  for (const s of servers) {
    void bridge()
      .query({ type: "playlists", data: { server_id: s.id } })
      .then((r) => {
        if (r.type !== "playlists") return;
        for (const p of r.data) {
          if (writable(p)) readOnly.delete(p.id);
          else readOnly.add(p.id);
        }
      })
      .catch(() => undefined);
  }
}

onDragBegan(refreshReadOnly);
let primed = false;

/** Whether `p` can be dropped on playlist `playlistId` (`readOnlyIds` stands in for what is known in tests). */
export function playlistAccepts(p: DragPayload, playlistId: string, readOnlyIds: ReadonlySet<string> = readOnly): boolean {
  if (readOnlyIds.has(playlistId)) return false;
  // A playlist's own rows reorder in its view; onto its entry they'd only duplicate.
  if (p.origin === `playlist:${playlistId}`) return false;
  switch (p.target.type) {
    case "tracks":
    case "albums":
    case "queueItems":
      return true;
    case "playlists":
      return !p.target.data.ids.includes(playlistId);
    default:
      // An artist is a whole discography: the menu doesn't offer it either.
      return false;
  }
}

/** Adds what `p` names to the playlist (at `atIndex`, else the end) and says so with an undoable toast. */
export async function dropOnPlaylist(p: DragPayload, playlistId: string, name: string, atIndex?: number): Promise<void> {
  const b = bridge();
  const detail = await b.query({ type: "playlist", data: { id: playlistId } }).catch(() => undefined);
  if (detail?.type === "playlistDetail" && detail.data && !writable(detail.data)) {
    readOnly.add(playlistId);
    return;
  }
  const tracks = await resolveTrackRefs(p.target);
  if (!tracks.length) return;
  toastUndoable(t("toast.addedToPlaylist", { playlist: name }), () =>
    b.dispatch({ type: "playlistAdd", data: { playlist_id: playlistId, tracks, at_index: atIndex } }));
}

/**
 * Drop-zone props for a playlist shown as one item (a sidebar entry, a tile). The data
 * attributes let a drag that isn't HTML5 (a queue row, which dnd-kit moves) find it.
 */
export function playlistDropZone(playlistId: string, name: string): DropProps & { "data-playlist-drop": string; "data-playlist-name": string } {
  // Known before the first drag, so a quick first drop on a smart playlist isn't let in.
  if (!primed && useApp.getState().accounts.length) {
    primed = true;
    queueMicrotask(refreshReadOnly);
  }
  return {
    ...dropZone({
      accepts: (p) => playlistAccepts(p, playlistId),
      onDrop: (p) => void dropOnPlaylist(p, playlistId, name),
    }),
    "data-playlist-drop": playlistId,
    "data-playlist-name": name,
  };
}

/** The playlist drop target at a point, for drags that don't fire HTML5 drag events. */
export function playlistTargetAt(x: number, y: number): { el: HTMLElement; id: string; name: string } | undefined {
  for (const node of document.elementsFromPoint(x, y)) {
    const el = (node as HTMLElement).closest<HTMLElement>("[data-playlist-drop]");
    if (!el) continue;
    const id = el.dataset.playlistDrop;
    if (id) return { el, id, name: el.dataset.playlistName ?? "" };
  }
  return undefined;
}
