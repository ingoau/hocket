// The context menu's Add to playlist submenu: a search field (focused, filters live), New playlist,
// then the playlists you can add to grouped Pinned / Recent / All playlists (one Matches group while
// searching). Each row is a checkbox: ticked when the playlist holds the song(s), mixed when it holds
// some of several, a ×2 badge when it holds each more than once. Changes apply at once, each with a
// toast and Undo. A hover + (or Alt+click, or Shift+Enter) adds another copy; unticking a playlist
// that holds several copies asks Remove both / Remove one.
//
// Keyboard: focus stays in the search field; ↑↓ move through the rows (aria-activedescendant),
// Enter (or Space while the field is empty) toggles, Shift+Enter adds another copy, ← at the start
// of the field or Escape goes back to the menu, Tab closes both.
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { ActionTarget, Playlist } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { executeAction } from "../store/actions";
import { usePins } from "../lib/use-pins";
import { showToast, toastUndoable } from "../lib/ui-toast";
import { clickIntent, keepLayout, layoutPlaylists, pickerGroups, rowState, type PickerGroupId, type PickerLayout, type RowState } from "../lib/playlist-picker";
import { Icon } from "./Icon";

const GROUP_LABEL: Record<PickerGroupId, Parameters<typeof t>[0]> = { pinned: "picker.pinned", recent: "picker.recent", all: "picker.all", matches: "picker.matches" };

interface Props {
  trackIds: string[];
  /** The menu's left/right edges and the Add to playlist row's top, in window coordinates. */
  anchor: { left: number; right: number; top: number };
  /** The menu is on its way out: the submenu leaves with it, inert. */
  closing: boolean;
  motion: string;
  /** Back to the menu (← or Escape). */
  onBack: () => void;
  /** Close the menu and the submenu, then run [then] (opening a dialog, say). */
  onClose: (then?: () => void) => void;
}

type Row = { kind: "new" } | { kind: "playlist"; playlist: Playlist; state: RowState };

export function PlaylistSubmenu({ trackIds, anchor, closing, motion, onBack, onClose }: Props) {
  const serverId = useApp((s) => s.servers[0]?.id);
  const dispatch = useApp((s) => s.dispatch);
  const pins = usePins();
  const ids = JSON.stringify(trackIds);
  const { data: playlists } = useQuery(() => (serverId ? { type: "playlists", data: { server_id: serverId } } : null), "playlists", [serverId], { keep: true });
  const { data: membership } = useQuery(() => (serverId ? { type: "playlistMembership", data: { server_id: serverId, track_ids: trackIds } } : null), "membership", [serverId, ids], { keep: true });
  const [layout, setLayout] = useState<PickerLayout | undefined>(undefined);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(-1);
  /** The playlist whose Remove both / Remove one is open, and which of the two is active. */
  const [asking, setAsking] = useState<{ id: string; active: number } | null>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | undefined>(undefined);
  const ref = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);

  // The order is taken when the submenu opens and kept while it is open (keepLayout).
  useEffect(() => {
    if (!playlists) return;
    const next = layoutPlaylists(playlists, pins);
    setLayout((prev) => (prev ? keepLayout(prev, next) : next));
  }, [playlists, pins]);

  const byId = useMemo(() => new Map((playlists ?? []).map((p) => [p.id, p])), [playlists]);
  const memberOf = useMemo(() => new Map((membership ?? []).map((m) => [m.playlistId, m])), [membership]);
  const groups = layout ? pickerGroups(layout, byId, query) : [];
  const rows: Row[] = [{ kind: "new" }, ...groups.flatMap((g) => g.playlists.map((p): Row => ({ kind: "playlist", playlist: p, state: rowState(trackIds, memberOf.get(p.id)) })))];
  const ready = !!layout && !!membership;
  const typed = query.trim();

  // Beside the menu, level with the row; on the other side when there's no room. Placed once, when it
  // first draws in full, so filtering (a shorter list) doesn't move it.
  useLayoutEffect(() => {
    if (!ready || !ref.current) return;
    const w = ref.current.offsetWidth;
    const h = ref.current.offsetHeight;
    if (!pos) {
      const x = anchor.right - 4 + w <= window.innerWidth - 6 ? anchor.right - 4 : Math.max(4, anchor.left - w + 4);
      setPos({ x, y: Math.max(4, Math.min(anchor.top - 6, window.innerHeight - h - 6)) });
    } else if (pos.y + h > window.innerHeight - 6) {
      // Grown past the bottom (the playlists arrived, a search was cleared): lift it, never lower it.
      setPos({ x: pos.x, y: Math.max(4, window.innerHeight - h - 6) });
    }
  }, [ready, pos, anchor, layout, query, membership]);

  // Focused once placed (a hidden field can't take focus).
  const placed = !!pos;
  useEffect(() => {
    if (placed) input.current?.focus({ preventScroll: true });
  }, [placed]);

  // Keep the active row in view in a long list.
  useEffect(() => {
    if (active < 0) return;
    document.getElementById(`pp-row-${active}`)?.scrollIntoView({ block: "nearest" });
  }, [active]);

  if (!ready) return null;

  const add = (p: Playlist, track_ids: string[], again: boolean) =>
    toastUndoable(t(again ? "toast.addedAgain" : "toast.addedToPlaylist", { playlist: p.name }), () => dispatch({ type: "playlistAdd", data: { playlist_id: p.id, track_ids, at_index: undefined } }));
  const remove = (p: Playlist, indices: number[], one: boolean) => {
    setAsking(null);
    if (indices.length) toastUndoable(t(one ? "toast.removedOneFrom" : "toast.removedFrom", { playlist: p.name }), () => dispatch({ type: "playlistRemove", data: { playlist_id: p.id, indices } }));
  };
  const addCopy = (p: Playlist, s: RowState) => {
    setAsking(null);
    add(p, trackIds, s.checked !== false);
  };
  const toggle = (p: Playlist, s: RowState, i: number) => {
    switch (clickIntent(s)) {
      case "add":
        setAsking(null);
        add(p, trackIds, false);
        return;
      case "addMissing":
        setAsking(null);
        add(p, s.missing, false);
        return;
      case "remove":
        remove(p, s.indices, false);
        return;
      case "ask":
        setActive(i);
        setAsking((a) => (a?.id === p.id ? null : { id: p.id, active: 0 }));
    }
  };
  const createNew = () => {
    if (!typed) {
      // No name yet: the app's own New playlist dialog, with the songs.
      const target: ActionTarget = { type: "tracks", data: { ids: trackIds } };
      onClose(() => void executeAction("ui.newPlaylist", target));
      return;
    }
    if (!serverId) return;
    dispatch({ type: "createPlaylist", data: { server_id: serverId, name: typed, track_ids: trackIds } });
    showToast(t("toast.addedToPlaylist", { playlist: typed }));
    setQuery("");
    setActive(-1);
  };
  const activate = (r: Row | undefined, i: number, e?: { altKey?: boolean; shiftKey?: boolean }) => {
    if (!r) return;
    if (r.kind === "new") createNew();
    else if (e?.altKey || e?.shiftKey) addCopy(r.playlist, r.state);
    else toggle(r.playlist, r.state, i);
  };

  const askRow = asking ? rows.findIndex((r) => r.kind === "playlist" && r.playlist.id === asking.id) : -1;
  const askFor = askRow >= 0 ? (rows[askRow] as Extract<Row, { kind: "playlist" }>) : undefined;
  const askChoices = askFor ? [() => remove(askFor.playlist, askFor.state.indices, false), () => remove(askFor.playlist, askFor.state.lastCopies, true)] : [];

  const onKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    e.stopPropagation();
    if (asking && askFor) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setAsking({ ...asking, active: asking.active ? 0 : 1 });
        return;
      }
      if (e.key === "Enter" || (e.key === " " && !query)) {
        e.preventDefault();
        askChoices[asking.active]?.();
        return;
      }
      if (e.key === "Escape" || e.key === "ArrowLeft") {
        e.preventDefault();
        setAsking(null);
        return;
      }
    }
    if (e.key === "Tab") {
      e.preventDefault();
      onClose();
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      onBack();
      return;
    }
    if (e.key === "ArrowLeft" && e.currentTarget.selectionStart === 0 && e.currentTarget.selectionEnd === 0) {
      e.preventDefault();
      onBack();
      return;
    }
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const n = rows.length;
      setActive((a) => (e.key === "ArrowDown" ? (a + 1) % n : a <= 0 ? n - 1 : a - 1));
      return;
    }
    if (e.key === "Home" && active >= 0 && !query) {
      e.preventDefault();
      setActive(0);
      return;
    }
    if (e.key === "End" && active >= 0 && !query) {
      e.preventDefault();
      setActive(rows.length - 1);
      return;
    }
    if ((e.key === "Enter" || (e.key === " " && !query)) && active >= 0) {
      e.preventDefault();
      activate(rows[active], active, { shiftKey: e.shiftKey && e.key === "Enter" });
    }
  };

  const noMatches = !!typed && !groups.length;
  let at = 0;
  return (
    <div
      ref={ref}
      className={`menu cm cm-sub ${motion}`}
      style={{ left: pos?.x ?? -9999, top: pos?.y ?? 0, visibility: pos ? undefined : "hidden" }}
      inert={closing || undefined}
      data-context-layer=""
      data-testid="playlist-submenu"
      // Clicks on rows keep the focus in the field.
      onMouseDown={(e) => { if (e.target !== input.current) e.preventDefault(); }}
    >
      <label className="cm-search">
        <Icon name="search" size={15} />
        <span className="sr-only">{t("picker.search")}</span>
        <input
          ref={input}
          type="text"
          value={query}
          placeholder={t("picker.search")}
          spellCheck={false}
          aria-controls="pp-list"
          aria-activedescendant={asking && askFor ? `pp-ask-${asking.active}` : active >= 0 ? `pp-row-${active}` : undefined}
          onChange={(e) => { setQuery(e.target.value); setActive(-1); setAsking(null); }}
          onKeyDown={onKey}
          data-testid="playlist-search"
        />
      </label>
      <div className="cm-sub-list" role="menu" id="pp-list" aria-label={t("menu.addToPlaylist")}>
        <div id={`pp-row-${at++}`} role="menuitem" className={`mi cm-new ${active === 0 ? "active" : ""}`} onMouseMove={(e) => { if (e.movementX || e.movementY) { setActive(0); setAsking(null); } }} onClick={createNew} data-testid="playlist-new">
          <Icon name="plus" size={16} />
          <span className="grow truncate">{typed ? t("picker.newNamed", { query: typed }) : t("picker.new")}</span>
        </div>
        {groups.map((g) => (
          <div key={g.id} role="group" aria-labelledby={`pp-group-${g.id}`}>
            <div className="sep" role="separator" />
            <div className="cm-group-label" id={`pp-group-${g.id}`}>
              {g.id === "pinned" ? <Icon name="pin" size={12} /> : null}
              {t(GROUP_LABEL[g.id])}
            </div>
            {g.playlists.map((p) => {
              const i = at++;
              const r = rows[i] as Extract<Row, { kind: "playlist" }>;
              const s = r.state;
              return (
                <div key={p.id} role="none" className={`cm-prow ${active === i ? "active" : ""} ${asking?.id === p.id ? "asking" : ""}`} onMouseMove={(e) => { if (e.movementX || e.movementY) { setActive(i); if (asking && asking.id !== p.id) setAsking(null); } }}>
                  <div
                    id={`pp-row-${i}`}
                    role="menuitemcheckbox"
                    aria-checked={s.checked}
                    aria-haspopup={s.copies > 1 ? "menu" : undefined}
                    aria-expanded={s.copies > 1 ? asking?.id === p.id : undefined}
                    aria-label={s.copies > 1 ? `${p.name}, ${t("picker.copiesLabel", { n: s.copies })}` : p.name}
                    title={p.name}
                    className="cm-prow-main"
                    onClick={(e) => activate(r, i, { altKey: e.altKey })}
                    data-testid="playlist-row"
                    data-playlist={p.id}
                  >
                    <span className={`cm-box ${s.checked === true ? "on" : s.checked === "mixed" ? "mixed" : ""}`} aria-hidden="true">
                      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" strokeWidth="3.4" strokeLinecap="round" strokeLinejoin="round">
                        {s.checked === "mixed" ? <path d="M6 12h12" /> : <path d="M5 12.5l4.5 4.5L19 7.5" />}
                      </svg>
                    </span>
                    <span className="grow truncate">{p.name}</span>
                    {s.copies > 1 ? <span className="cm-badge" aria-hidden="true">{t("picker.copies", { n: s.copies })}</span> : null}
                  </div>
                  <div role="menuitem" tabIndex={-1} className="cm-plus" aria-label={t("picker.addCopyTo", { playlist: p.name })} title={t("picker.addCopy")} onClick={() => addCopy(p, s)} data-testid="playlist-add-copy">
                    <Icon name="plus" size={14} />
                  </div>
                </div>
              );
            })}
          </div>
        ))}
      </div>
      {noMatches ? (
        <>
          <div className="sep" role="separator" />
          <div className="cm-empty truncate" role="status">{t("picker.none", { query: typed })}</div>
        </>
      ) : null}
      {askFor ? <AskMenu playlist={askFor.playlist} copies={askFor.state.copies} active={asking?.active ?? 0} rowId={`pp-row-${askRow}`} onPick={(k) => askChoices[k]?.()} /> : null}
    </div>
  );
}

/** Remove both (or all N) / Remove one, under the row it is for (above it near the window's bottom). */
function AskMenu({ playlist, copies, active, rowId, onPick }: { playlist: Playlist; copies: number; active: number; rowId: string; onPick: (k: number) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [at, setAt] = useState<{ left: number; top: number } | undefined>(undefined);
  useLayoutEffect(() => {
    const row = document.getElementById(rowId)?.parentElement;
    const box = ref.current;
    if (!row || !box) return;
    const r = row.getBoundingClientRect();
    const h = box.offsetHeight;
    const top = r.bottom - 4 + h <= window.innerHeight - 6 ? r.bottom - 4 : Math.max(4, r.top - h + 4);
    setAt({ left: r.right - box.offsetWidth - 6, top });
  }, [rowId]);
  const labels = [copies === 2 ? t("picker.removeBoth") : t("picker.removeAll", { n: copies }), t("picker.removeOne")];
  return (
    <div ref={ref} className="menu cm cm-ask fade-in" role="menu" aria-label={t("picker.removeFrom", { playlist: playlist.name })} style={{ left: at?.left ?? 0, top: at?.top ?? 0, visibility: at ? undefined : "hidden" }} data-testid="playlist-ask">
      {labels.map((l, k) => (
        <div key={l} id={`pp-ask-${k}`} role="menuitem" className={`mi ${k === 0 ? "destructive" : ""} ${active === k ? "active" : ""}`} onClick={() => onPick(k)} data-testid={k === 0 ? "playlist-remove-all" : "playlist-remove-one"}>
          <span className="grow truncate">{l}</span>
        </div>
      ))}
    </div>
  );
}
