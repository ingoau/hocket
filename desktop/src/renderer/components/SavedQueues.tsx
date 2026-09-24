// "Recent" tab beside the queue: saved queues with pin/restore/delete/save-as-playlist.
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { openContextMenu } from "./ContextMenu";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";
import { EmptyState } from "./EmptyState";
import { fmtRelative } from "../lib/format";
import { executeAction } from "../store/actions";

export function SavedQueues() {
  const queues = useApp((s) => s.savedQueues);
  const d = bridge().dispatch;
  if (!queues.length) return <EmptyState message={t("savedQueues.empty")} icon="restore" testId="saved-queues-empty" />;
  const sorted = [...queues].sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.lastInteractedAt - a.lastInteractedAt);
  return (
    <div className="pane-body" role="list" data-testid="saved-queues">
      {sorted.map((q) => (
        <div key={q.id} className="sq-row" role="listitem" onDoubleClick={() => d({ type: "restoreSavedQueue", data: { id: q.id } })} onContextMenu={(e) => void openContextMenu(e, { type: "savedQueue", data: { id: q.id } }, { savedQueueId: q.id })} data-testid="saved-queue-row">
          <Artwork id={q.coverArt} size={64} className="art" />
          <div className="grow" style={{ minWidth: 0 }}>
            <div className="truncate">{q.label}{q.pinned ? <span className="badge synced" style={{ marginLeft: 6 }}>{t("savedQueues.pinned")}</span> : null}</div>
            <div className="small muted truncate">{t("savedQueues.tracks", { count: q.trackCount })} · {fmtRelative(q.lastInteractedAt)}</div>
          </div>
          <button type="button" className="btn icon sm" title={t("savedQueues.restore")} aria-label={t("savedQueues.restore")} onClick={() => d({ type: "restoreSavedQueue", data: { id: q.id } })}><Icon name="restore" size={14} /></button>
          <button type="button" className={`btn icon sm ${q.pinned ? "on" : ""}`} title={q.pinned ? t("savedQueues.unpin") : t("savedQueues.pin")} aria-label={q.pinned ? t("savedQueues.unpin") : t("savedQueues.pin")} onClick={() => d({ type: "pinSavedQueue", data: { id: q.id, pinned: !q.pinned } })}><Icon name="pin" size={14} /></button>
          <button type="button" className="btn icon sm" title={t("savedQueues.saveAsPlaylist")} aria-label={t("savedQueues.saveAsPlaylist")} onClick={() => void executeAction("ui.saveQueueAsPlaylist", { type: "savedQueue", data: { id: q.id } }, { savedQueueId: q.id })}><Icon name="playlistAdd" size={14} /></button>
          <button type="button" className="btn icon sm" title={t("savedQueues.delete")} aria-label={t("savedQueues.delete")} onClick={() => d({ type: "deleteSavedQueue", data: { id: q.id } })}><Icon name="trash" size={14} /></button>
        </div>
      ))}
    </div>
  );
}
