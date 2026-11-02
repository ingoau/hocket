// The song stash (#150) in the top bar: a chip that grows in once something is stashed (a fan of
// the first two covers and the count) and opens a popover listing the stash, with actions on the
// whole of it or on a selection within it. Acting on the stash keeps it; Clear empties it (the core
// toasts an Undo). The list reorders by drag and removes with × or Delete.
import { useEffect, useMemo, useRef, useState } from "react";
import type { ActionTarget, StashItem } from "@core/api";
import { t } from "@shared/strings";
import { ARTWORK_SIZES } from "@shared/constants";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { executeAction } from "../store/actions";
import { usePresence } from "../lib/presence";
import { showToast } from "../lib/ui-toast";
import { fmtTime } from "../lib/format";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";
import { openContextMenu } from "./ContextMenu";

const count = (n: number) => (n === 1 ? t("stash.countOne") : t("stash.count", { count: n }));

/** Clicks on these don't close the popover: what it opened (the playlist picker, a menu, a dialog, toasts). */
const KEEP_OPEN = '[data-context-layer], [data-testid="toasts"], .overlay, .dialog';

export function StashChip() {
  const items = useApp((s) => s.stash);
  const open = useApp((s) => s.stashOpen);
  const setOpen = useApp((s) => s.setStashOpen);
  const has = items.length > 0;
  const chip = usePresence<true, HTMLDivElement>(has || undefined);
  const popover = usePresence<true, HTMLDivElement>((open && has) || undefined);
  const ref = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  // Emptied (cleared here or elsewhere): nothing left to show.
  useEffect(() => {
    if (!has && open) setOpen(false);
  }, [has, open, setOpen]);
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      const at = e.target as Element | null;
      if (ref.current?.contains(at as Node) || at?.closest?.(KEEP_OPEN)) return;
      setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open, setOpen]);
  // Opened from the shortcut or palette: focus the list.
  useEffect(() => {
    if (open) requestAnimationFrame(() => ref.current?.querySelector<HTMLElement>(".stash-row")?.focus());
  }, [open]);
  if (!chip.value) return null;
  const [first, second] = items;
  return (
    <div
      ref={ref}
      className="stash-anchor"
      onKeyDown={(e) => {
        if (e.key === "Escape" && open) {
          e.preventDefault();
          e.stopPropagation();
          setOpen(false);
          button.current?.focus();
        }
      }}
    >
      <div {...chip.exitProps} className={`stash-chip-wrap ${chip.motion}`}>
        <button
          ref={button}
          type="button"
          className={`stash-chip ${open ? "on" : ""}`}
          aria-label={t("stash.chip", { count: items.length })}
          aria-haspopup="dialog"
          aria-expanded={open}
          onClick={() => setOpen(!open)}
          data-testid="stash-chip"
        >
          <span className={`stash-fan ${second ? "two" : ""}`} aria-hidden="true">
            {second ? <Fanned key={second.entryId} item={second} back /> : null}
            {first ? <Fanned key={first.entryId} item={first} /> : null}
          </span>
          <span key={items.length} className="stash-count" data-testid="stash-count">{items.length}</span>
        </button>
      </div>
      {popover.value ? <StashPopover items={items} exitProps={popover.exitProps} motion={popover.motion} close={() => setOpen(false)} /> : null}
    </div>
  );
}

function Fanned({ item, back }: { item: StashItem; back?: boolean }) {
  return (
    <span className={`stash-card ${back ? "back" : "front"}`}>
      <Artwork id={item.track.coverArt ?? undefined} size={ARTWORK_SIZES.thumb} className="art" />
    </span>
  );
}

function StashPopover({ items, exitProps, motion, close }: { items: StashItem[]; exitProps: ReturnType<typeof usePresence<true, HTMLDivElement>>["exitProps"]; motion: string; close: () => void }) {
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [drag, setDrag] = useState<{ from: number; over: number } | undefined>();
  const live = useMemo(() => new Set(items.map((i) => i.entryId)), [items]);
  // Entries removed elsewhere drop out of the selection.
  const picked = items.filter((i) => selected.has(i.entryId));
  useEffect(() => {
    if ([...selected].some((id) => !live.has(id))) setSelected(new Set([...selected].filter((id) => live.has(id))));
  }, [live, selected]);
  const acting = picked.length ? picked : items;
  const ids = acting.map((i) => i.track.id);
  const target: ActionTarget = { type: "tracks", data: { ids } };
  const serverId = items[0]?.track.serverId ?? "";
  const play = (shuffle: boolean, from = 0, list = ids) =>
    bridge().dispatch({ type: "playTracks", data: { server_id: serverId, track_ids: list, start_index: from, label: t("stash.title"), shuffle } });
  const queue = (next: boolean) => {
    useApp.getState().runAction(next ? "playNext" : "playLater", target);
    const n = ids.length;
    showToast(next ? (n === 1 ? t("stash.addedNextOne") : t("stash.addedNext", { count: n })) : n === 1 ? t("stash.addedOne") : t("stash.added", { count: n }));
  };
  const remove = (entryIds: string[]) => bridge().dispatch({ type: "stashRemove", data: { entry_ids: entryIds } });
  const toggle = (id: string) => {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setSelected(next);
  };
  const drop = () => {
    if (drag && drag.from !== drag.over) {
      const entry = items[drag.from];
      const to = drag.over > drag.from ? drag.over - 1 : drag.over;
      if (entry) bridge().dispatch({ type: "stashMove", data: { entry_id: entry.entryId, to_index: to } });
    }
    setDrag(undefined);
  };
  return (
    <div {...exitProps} className={`popover stash-popover ${motion}`} role="dialog" aria-label={t("stash.title")} data-testid="stash-popover">
      <div className="stash-head">
        <div className="grow">
          <div className="stash-title">{t("stash.title")}</div>
          <div className="small muted" aria-live="polite">{picked.length ? t("stash.selected", { count: picked.length }) : count(items.length)}</div>
        </div>
        {picked.length ? <button type="button" className="btn sm ghost" onClick={() => setSelected(new Set())}>{t("stash.clearSelection")}</button> : null}
        <button type="button" className="btn sm ghost" onClick={() => bridge().dispatch({ type: "stashClear" })} data-testid="stash-clear">
          <Icon name="trash" size={16} />
          {t("stash.clear")}
        </button>
      </div>
      <div className="stash-actions" role="toolbar" aria-label={picked.length ? t("stash.selectionHint") : t("stash.title")}>
        <button type="button" className="btn primary sm" onClick={() => play(false)} data-testid="stash-play"><Icon name="play" size={18} />{t("stash.playAll")}</button>
        <button type="button" className="btn icon" aria-label={t("stash.shuffle")} title={t("stash.shuffle")} onClick={() => play(true)}><Icon name="shuffle" size={18} /></button>
        <button type="button" className="btn icon" aria-label={t("action.playNext")} title={t("action.playNext")} onClick={() => queue(true)} data-testid="stash-play-next"><Icon name="playNext" size={18} /></button>
        <button type="button" className="btn icon" aria-label={t("action.playLater")} title={t("action.playLater")} onClick={() => queue(false)} data-testid="stash-play-later"><Icon name="playLater" size={18} /></button>
        <button type="button" className="btn icon" aria-label={t("action.addToPlaylist")} title={t("action.addToPlaylist")} aria-haspopup="menu" onClick={(e) => void executeAction("addToPlaylist", target, { anchor: e.currentTarget })} data-testid="stash-add-to-playlist"><Icon name="playlistAdd" size={18} /></button>
        <button type="button" className="btn icon" aria-label={t("stash.newPlaylist")} title={t("stash.newPlaylist")} onClick={() => void executeAction("ui.newPlaylist", target)} data-testid="stash-new-playlist"><Icon name="plus" size={18} /></button>
        <button type="button" className="btn icon" aria-label={t("action.download")} title={t("action.download")} onClick={() => void executeAction("download", target)}><Icon name="download" size={18} /></button>
      </div>
      <ul className="plain-list stash-list" aria-label={t("stash.title")} onDragLeave={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node)) setDrag((d) => (d ? { ...d, over: d.from } : d)); }}>
        {items.map((item, i) => {
          const on = selected.has(item.entryId);
          const before = drag && drag.over === i && drag.from !== i && drag.from !== i - 1;
          const after = drag && i === items.length - 1 && drag.over === items.length && drag.from !== i;
          return (
            <li
              key={item.entryId}
              className={`stash-row ${on ? "selected" : ""} ${drag?.from === i ? "dragging" : ""} ${before ? "drop-before" : ""} ${after ? "drop-after" : ""}`}
              tabIndex={0}
              draggable
              data-testid="stash-row"
              onClick={() => toggle(item.entryId)}
              onDoubleClick={() => play(false, i, items.map((x) => x.track.id))}
              onKeyDown={(e) => {
                if (e.key === "Delete" || e.key === "Backspace") { e.preventDefault(); remove(picked.length && on ? picked.map((p) => p.entryId) : [item.entryId]); }
                else if (e.key === "Enter") { e.preventDefault(); play(false, i, items.map((x) => x.track.id)); }
                else if (e.key === " ") { e.preventDefault(); toggle(item.entryId); }
                else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                  e.preventDefault();
                  const sib = (e.key === "ArrowDown" ? e.currentTarget.nextElementSibling : e.currentTarget.previousElementSibling) as HTMLElement | null;
                  if (e.altKey) {
                    const to = e.key === "ArrowDown" ? i + 1 : i - 1;
                    if (to >= 0 && to < items.length) bridge().dispatch({ type: "stashMove", data: { entry_id: item.entryId, to_index: to } });
                  } else sib?.focus();
                }
              }}
              onContextMenu={(e) => void openContextMenu(e, { type: "tracks", data: { ids: on && picked.length ? picked.map((p) => p.track.id) : [item.track.id] } })}
              onDragStart={(e) => { e.dataTransfer.effectAllowed = "move"; e.dataTransfer.setData("text/plain", item.entryId); setDrag({ from: i, over: i }); }}
              onDragOver={(e) => {
                if (!drag) return;
                e.preventDefault();
                const r = e.currentTarget.getBoundingClientRect();
                const over = e.clientY > r.top + r.height / 2 ? i + 1 : i;
                if (over !== drag.over) setDrag({ ...drag, over });
              }}
              onDrop={(e) => { e.preventDefault(); drop(); }}
              onDragEnd={() => setDrag(undefined)}
            >
              <Icon name="grip" size={16} className="stash-grip" />
              <Artwork id={item.track.coverArt ?? undefined} size={ARTWORK_SIZES.thumb} className="art" />
              <span className="stash-text">
                <span className="truncate">{item.track.title}{on ? <span className="sr-only"> ({t("stash.picked")})</span> : null}</span>
                <span className="small muted truncate">{item.track.artist ?? ""}</span>
              </span>
              <span className="small faint">{item.track.durationMs ? fmtTime(item.track.durationMs) : ""}</span>
              <button
                type="button"
                className="btn icon sm stash-remove"
                aria-label={`${t("stash.remove")}: ${item.track.title}`}
                title={t("stash.remove")}
                onClick={(e) => { e.stopPropagation(); remove([item.entryId]); }}
                data-testid="stash-remove"
              >
                <Icon name="close" size={16} />
              </button>
            </li>
          );
        })}
      </ul>
      <button type="button" className="sr-only" onClick={close}>{t("misc.close")}</button>
    </div>
  );
}
