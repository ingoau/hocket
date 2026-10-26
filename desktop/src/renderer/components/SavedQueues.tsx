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

/** "Played 3 d ago", "Played just now". */
function fmtPlayed(at: number): string {
  return t("savedQueues.played", { when: fmtRelative(at) });
}
import { executeAction } from "../store/actions";

export function SavedQueues() {
  const queues = useApp((s) => s.savedQueues);
  const label = useApp((s) => s.queue.contextLabel);
  const count = useApp((s) => s.queue.history.length + (s.queue.current ? 1 : 0) + s.queue.playingNext.length + s.queue.totalUpcoming);
  const position = useApp((s) => (s.queue.current ? s.queue.history.length + 1 : 0));
  const [renaming, setRenaming] = useState<string | null | undefined>(undefined);
  const d = bridge().dispatch;
  // After Enter or Escape in a rename, focus goes back to that queue's Rename button (it unmounted the field).
  const [refocus, setRefocus] = useState<string | undefined>(undefined);
  useEffect(() => {
    if (refocus === undefined || renaming !== undefined) return;
    document.querySelector<HTMLElement>(`[data-rename-for="${CSS.escape(refocus)}"]`)?.focus();
    setRefocus(undefined);
  }, [refocus, renaming]);
  const rename = (saved_queue_id: string | null, value: string) => {
    setRenaming(undefined);
    if (value.trim()) d({ type: "renameQueue", data: { saved_queue_id: saved_queue_id ?? undefined, label: value.trim() } });
  };
  const sorted = [...queues].sort((a, b) => b.lastInteractedAt - a.lastInteractedAt);
  const pinned = sorted.filter((q) => q.pinned);
  const recent = sorted.filter((q) => !q.pinned);
  // One grouped list per section: the first and last rows round their outer corners.
  const row = (q: SavedQueue) => (
    <li key={q.id} className="sq-row" onContextMenu={(e) => void openContextMenu(e, { type: "savedQueue", data: { id: q.id } }, { savedQueueId: q.id })} data-testid="saved-queue-row">
      {renaming === q.id ? (
        <div className="sq-main editing">
          <RenameField initial={q.label} onDone={(v, key) => { rename(q.id, v); if (key) setRefocus(q.id); }} onCancel={() => { setRenaming(undefined); setRefocus(q.id); }} />
          <span className="small muted sq-hint">{t("savedQueues.renameHint")}</span>
        </div>
      ) : (
        <button type="button" className="sq-main" title={t("savedQueues.restoreNamed", { name: q.label })} onClick={() => { d({ type: "restoreSavedQueue", data: { id: q.id } }); showSideView("queue"); }} data-testid="saved-queue-restore">
          <span className="truncate sq-name">{q.label}</span>
          <span className="small muted truncate">{t("savedQueues.meta", { count: q.trackCount, when: fmtPlayed(q.lastInteractedAt) })}</span>
        </button>
      )}
      <button type="button" className={`btn icon sm sq-pin ${q.pinned ? "on" : ""}`} aria-pressed={q.pinned} title={q.pinned ? t("savedQueues.unpin") : t("savedQueues.pin")} aria-label={`${q.pinned ? t("savedQueues.unpin") : t("savedQueues.pin")} ${q.label}`} onClick={() => d({ type: "pinSavedQueue", data: { id: q.id, pinned: !q.pinned } })} data-testid="saved-queue-pin"><Icon name="pin" size={18} filled={q.pinned} /></button>
      {renaming === q.id ? null : <button type="button" className="btn icon sm sq-ren" title={t("savedQueues.rename")} aria-label={t("savedQueues.renameNamed", { name: q.label })} onClick={() => setRenaming(q.id)} data-rename-for={q.id} data-testid="saved-queue-rename"><Icon name="edit" size={18} /></button>}
      <button type="button" className="btn icon sm" title={t("savedQueues.delete")} aria-label={`${t("savedQueues.delete")} ${q.label}`} onClick={() => d({ type: "deleteSavedQueue", data: { id: q.id } })} data-testid="saved-queue-delete"><Icon name="trash" size={18} /></button>
    </li>
  );
  return (
    <div className="side-scroll queues-page" data-testid="saved-queues">
      <h3 className="sq-eyebrow" id="sq-playing">{t("savedQueues.playingNow")}</h3>
      <div className="sq-current" role="group" aria-labelledby="sq-playing" data-testid="saved-queues-current">
        <div className="sq-current-head">
          <div className="grow" style={{ minWidth: 0 }}>
            {renaming === null ? <RenameField initial={label ?? ""} onDone={(v, key) => { rename(null, v); if (key) setRefocus(""); }} onCancel={() => { setRenaming(undefined); setRefocus(""); }} /> : (
              <div className="sq-current-name truncate">{label ?? t("queue.untitled")}</div>
            )}
            <div className="sq-current-meta truncate">{t("savedQueues.tracks", { count })}</div>
            {position ? <div className="sq-current-where truncate">{t("savedQueues.position", { n: position, count })}</div> : null}
          </div>
          {position ? <Icon name="equalizer" size={22} /> : null}
        </div>
        <div className="sq-current-actions">
          <button type="button" className="btn sm sq-card-btn" disabled={count === 0} onClick={() => void executeAction("ui.saveQueueAsPlaylist")} data-testid="saved-queues-create-playlist"><Icon name="playlistAdd" size={18} />{t("savedQueues.createPlaylist")}</button>
          {label ? <button type="button" className="btn sm sq-card-btn" aria-label={t("savedQueues.renameNamed", { name: label })} onClick={() => setRenaming(null)} data-rename-for="" data-testid="saved-queues-current-rename"><Icon name="edit" size={18} />{t("savedQueues.rename")}</button> : null}
        </div>
      </div>
      {pinned.length ? <Group title={t("savedQueues.pinned")} id="sq-pinned">{pinned.map(row)}</Group> : null}
      <Group title={t("savedQueues.recent")} id="sq-recent">{recent.map(row)}</Group>
      {!recent.length ? <p className="sq-empty" data-testid="saved-queues-empty">{t("savedQueues.empty")}</p> : null}
    </div>
  );
}

function Group({ title, id, children }: { title: string; id: string; children: React.ReactNode }) {
  return (
    <div className="sq-group">
      <h3 className="sq-eyebrow" id={id}>{title}</h3>
      {Array.isArray(children) && children.length === 0 ? null : <ul className="plain-list sq-list" aria-labelledby={id}>{children}</ul>}
    </div>
  );
}

/** Enter or leaving the field saves; Escape cancels; a blank name changes nothing. */
function RenameField({ initial, onDone, onCancel }: { initial: string; onDone: (value: string, byKey: boolean) => void; onCancel: () => void }) {
  const input = useRef<HTMLInputElement>(null);
  const done = useRef(false);
  useEffect(() => { input.current?.focus(); input.current?.select(); }, []);
  const finish = (save: boolean, byKey = true) => {
    if (done.current) return;
    done.current = true;
    if (save) onDone(input.current?.value ?? "", byKey);
    else onCancel();
  };
  return (
    <input ref={input} className="input sq-rename" defaultValue={initial} aria-label={t("savedQueues.rename")} maxLength={200}
      onKeyDown={(e) => {
        if (e.key === "Enter") { e.preventDefault(); finish(true); }
        if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); finish(false); }
      }}
      onBlur={() => finish(true, false)} data-testid="saved-queue-rename-input" />
  );
}
