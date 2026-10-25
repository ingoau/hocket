// The Queues page, drilled into from the queue's name in the side panel: the
// playing queue (rename, save as playlist) above the pinned and recent saved
// queues, each with rename, pin and delete. Delete is immediate; the core
// makes it undoable like any queue change. Restoring one returns to the queue.
import { useEffect, useRef, useState } from "react";
import { t } from "@shared/strings";
import type { SavedQueue } from "@core/api";
import { showSideView, useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { openContextMenu } from "./ContextMenu";
import { Icon } from "./Icon";
import { fmtRelative } from "../lib/format";
import { executeAction } from "../store/actions";

export function SavedQueues() {
  const queues = useApp((s) => s.savedQueues);
  const label = useApp((s) => s.queue.contextLabel);
  const count = useApp((s) => s.queue.history.length + (s.queue.current ? 1 : 0) + s.queue.playingNext.length + s.queue.totalUpcoming);
  const [renaming, setRenaming] = useState<string | null | undefined>(undefined);
  const d = bridge().dispatch;
  const rename = (saved_queue_id: string | null, value: string) => {
    setRenaming(undefined);
    if (value.trim()) d({ type: "renameQueue", data: { saved_queue_id: saved_queue_id ?? undefined, label: value.trim() } });
  };
  const sorted = [...queues].sort((a, b) => b.lastInteractedAt - a.lastInteractedAt);
  const pinned = sorted.filter((q) => q.pinned);
  const recent = sorted.filter((q) => !q.pinned);
  const row = (q: SavedQueue) => (
    <li key={q.id} className="sq-row" onContextMenu={(e) => void openContextMenu(e, { type: "savedQueue", data: { id: q.id } }, { savedQueueId: q.id })} data-testid="saved-queue-row">
      {renaming === q.id ? <RenameField initial={q.label} onDone={(v) => rename(q.id, v)} onCancel={() => setRenaming(undefined)} /> : (
        <button type="button" className="sq-main" title={t("savedQueues.restore")} onClick={() => { d({ type: "restoreSavedQueue", data: { id: q.id } }); showSideView("queue"); }} data-testid="saved-queue-restore">
          <span className="truncate sq-name">{q.label}</span>
          <span className="small muted truncate">{t("savedQueues.tracks", { count: q.trackCount })} · {fmtRelative(q.lastInteractedAt)}</span>
        </button>
      )}
      <button type="button" className="btn icon sm" title={t("savedQueues.rename")} aria-label={t("savedQueues.renameNamed", { name: q.label })} onClick={() => setRenaming(q.id)} data-testid="saved-queue-rename"><Icon name="edit" size={16} /></button>
      <button type="button" className={`btn icon sm ${q.pinned ? "on" : ""}`} aria-pressed={q.pinned} title={q.pinned ? t("savedQueues.unpin") : t("savedQueues.pin")} aria-label={`${q.pinned ? t("savedQueues.unpin") : t("savedQueues.pin")} ${q.label}`} onClick={() => d({ type: "pinSavedQueue", data: { id: q.id, pinned: !q.pinned } })} data-testid="saved-queue-pin"><Icon name="pin" size={16} filled={q.pinned} /></button>
      <button type="button" className="btn icon sm" title={t("savedQueues.delete")} aria-label={`${t("savedQueues.delete")} ${q.label}`} onClick={() => d({ type: "deleteSavedQueue", data: { id: q.id } })} data-testid="saved-queue-delete"><Icon name="trash" size={16} /></button>
    </li>
  );
  return (
    <div className="side-scroll queues-page" data-testid="saved-queues">
      <div className="sq-current" role="group" aria-labelledby="sq-playing" data-testid="saved-queues-current">
        <div className="sq-eyebrow" id="sq-playing">{t("queue.nowPlaying")}</div>
        {renaming === null ? <RenameField initial={label ?? ""} onDone={(v) => rename(null, v)} onCancel={() => setRenaming(undefined)} /> : (
          <div className="sq-current-name">
            <span className="truncate">{label ?? t("queue.untitled")}</span>
            {label ? <button type="button" className="btn icon sm" title={t("savedQueues.rename")} aria-label={t("savedQueues.renameNamed", { name: label })} onClick={() => setRenaming(null)} data-testid="saved-queues-current-rename"><Icon name="edit" size={16} /></button> : null}
          </div>
        )}
        <div className="small muted">{t("savedQueues.tracks", { count })}</div>
        <button type="button" className="btn tonal sm sq-create" disabled={count === 0} onClick={() => void executeAction("ui.saveQueueAsPlaylist")} data-testid="saved-queues-create-playlist"><Icon name="playlistAdd" size={18} />{t("savedQueues.createPlaylist")}</button>
      </div>
      {pinned.length ? <Group title={t("savedQueues.pinned")} id="sq-pinned">{pinned.map(row)}</Group> : null}
      {recent.length ? <Group title={t("savedQueues.recent")} id="sq-recent">{recent.map(row)}</Group> : null}
      {!queues.length ? <p className="sq-empty small muted" data-testid="saved-queues-empty">{t("savedQueues.empty")}</p> : null}
    </div>
  );
}

function Group({ title, id, children }: { title: string; id: string; children: React.ReactNode }) {
  return (
    <div className="sq-group">
      <h3 className="sq-eyebrow" id={id}>{title}</h3>
      <ul className="plain-list" aria-labelledby={id}>{children}</ul>
    </div>
  );
}

/** Enter or leaving the field saves; Escape cancels; a blank name changes nothing. */
function RenameField({ initial, onDone, onCancel }: { initial: string; onDone: (value: string) => void; onCancel: () => void }) {
  const input = useRef<HTMLInputElement>(null);
  const done = useRef(false);
  useEffect(() => { input.current?.focus(); input.current?.select(); }, []);
  const finish = (save: boolean) => {
    if (done.current) return;
    done.current = true;
    if (save) onDone(input.current?.value ?? "");
    else onCancel();
  };
  return (
    <input ref={input} className="input sq-rename" defaultValue={initial} aria-label={t("savedQueues.rename")} maxLength={200}
      onKeyDown={(e) => {
        if (e.key === "Enter") { e.preventDefault(); finish(true); }
        if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); finish(false); }
      }}
      onBlur={() => finish(true)} data-testid="saved-queue-rename-input" />
  );
}
