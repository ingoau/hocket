// The queue view's header: the queue's name and the Autoplay toggle. Shared by
// the side panel and the fullscreen player. In the side panel the name opens
// the Queues page (SavedQueues); in the fullscreen player, which has no pages,
// it drops down the saved queues to restore one, each with its own menu.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { t } from "@shared/strings";
import { showSideView, useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { fmtRelative } from "../lib/format";
import { openMenuFromButton } from "./ContextMenu";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";
import { usePresence } from "../lib/presence";

export function QueueHeader({ large = false }: { large?: boolean }) {
  const autoplay = useApp((s) => s.queue.autoplay);
  const d = bridge().dispatch;
  return (
    <div className={`queue-header ${large ? "large" : ""}`} data-testid="queue-header">
      <QueueSelector large={large} />
      <button type="button" className={`autoplay-pill ${autoplay ? "on" : ""}`} aria-pressed={autoplay} title={t("player.autoplay")} onClick={() => d({ type: "setAutoplay", data: { enabled: !autoplay } })} data-testid="queue-autoplay">
        <Icon name="autoplay" size={18} />{t("player.autoplay")}
      </button>
    </div>
  );
}

function QueueSelector({ large }: { large: boolean }) {
  return large ? <QueueSelectorPopover /> : <QueuesLink />;
}

function QueuesLink() {
  const label = useApp((s) => s.queue.contextLabel) ?? t("queue.untitled");
  return (
    <button type="button" className="qs-button" aria-label={t("queue.openQueues", { name: label })} title={t("queue.recentTitle")} onClick={() => showSideView("recent")} data-drill="recent" data-testid="queue-selector">
      <span className="qs-label" data-testid="queue-selector-label">{label}</span>
      <Icon name="chevronRight" size={20} className="qs-chevron" />
    </button>
  );
}

function QueueSelectorPopover() {
  const label = useApp((s) => s.queue.contextLabel) ?? t("queue.untitled");
  const saved = useApp((s) => s.savedQueues);
  const menuOpen = useApp((s) => !!s.contextMenu);
  const [open, setOpen] = useState(false);
  const [at, setAt] = useState<{ left: number; top: number; width: number; maxHeight: number } | undefined>(undefined);
  const button = useRef<HTMLButtonElement>(null);
  const pop = useRef<HTMLDivElement>(null);
  const d = bridge().dispatch;
  const sorted = [...saved].sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.lastInteractedAt - a.lastInteractedAt);

  const close = (refocus = true) => {
    setOpen(false);
    setAt(undefined);
    if (refocus) button.current?.focus({ preventScroll: true });
  };
  // Under the button, as wide as the view allows; drawn in a portal so the scrolling pane never clips it.
  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      const r = button.current?.getBoundingClientRect();
      if (!r) return;
      const width = Math.min(360, window.innerWidth - 16);
      const left = Math.max(8, Math.min(r.left, window.innerWidth - width - 8));
      setAt({ left, top: r.bottom + 6, width, maxHeight: Math.max(160, window.innerHeight - r.bottom - 24) });
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [open]);
  // Once placed, focus moves to the first queue.
  const placed = !!at;
  useEffect(() => {
    if (open && placed) pop.current?.querySelector<HTMLElement>(".qs-item")?.focus({ preventScroll: true });
  }, [open, placed]);
  useEffect(() => {
    if (!open) return;
    const down = (e: MouseEvent) => {
      const target = e.target as Node;
      if (pop.current?.contains(target) || button.current?.contains(target)) return;
      // A row's own menu floats outside the popover: leave the popover open under it.
      if ((target as Element).closest?.('[data-testid="context-menu"]')) return;
      setOpen(false);
      setAt(undefined);
    };
    window.addEventListener("mousedown", down, true);
    return () => window.removeEventListener("mousedown", down, true);
  }, [open]);

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Escape" && !menuOpen) {
      e.preventDefault();
      e.stopPropagation();
      close();
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp" && e.key !== "Home" && e.key !== "End") return;
    const items = Array.from(pop.current?.querySelectorAll<HTMLElement>(".qs-item") ?? []);
    if (!items.length) return;
    e.preventDefault();
    e.stopPropagation();
    const i = items.indexOf(document.activeElement as HTMLElement);
    const next = e.key === "Home" ? 0 : e.key === "End" ? items.length - 1 : e.key === "ArrowDown" ? (i + 1) % items.length : (i - 1 + items.length) % items.length;
    items[next]?.focus();
  };
  const restore = (id: string) => {
    d({ type: "restoreSavedQueue", data: { id } });
    close();
  };

  // Closing, it stays mounted (inert) while its exit plays.
  const presence = usePresence(open ? at : undefined, pop);
  const shownAt = presence.value;
  // Inside the fullscreen player the popover stays in its dialog (focus is trapped there).
  const host = button.current?.closest<HTMLElement>('[role="dialog"]') ?? document.body;
  return (
    <>
      <button ref={button} type="button" className="qs-button large" aria-haspopup="dialog" aria-expanded={open} aria-label={`${t("queue.selector")}: ${label}`} onClick={() => (open ? close(false) : setOpen(true))}
        onKeyDown={(e) => { if (e.key === "Escape" && open) { e.preventDefault(); e.stopPropagation(); close(); } }} data-testid="queue-selector">
        <span className="qs-label" data-testid="queue-selector-label">{label}</span>
        <Icon name="chevronDown" size={28} className="qs-chevron" />
      </button>
      {shownAt ? createPortal(
        <div {...presence.exitProps} className={`qs-popover ${presence.motion}`} role="dialog" aria-label={t("queue.selector")} style={{ left: shownAt.left, top: shownAt.top, width: shownAt.width, maxHeight: shownAt.maxHeight }} onKeyDown={onKey} data-testid="queue-selector-menu">
          <div className="qs-current">
            <Icon name="check" size={18} />
            <div className="grow" style={{ minWidth: 0 }}>
              <div className="truncate qs-name">{label}</div>
              <div className="qs-sub">{t("queue.current")}</div>
            </div>
          </div>
          {sorted.length ? (
            <ul className="plain-list qs-list" aria-label={t("queue.recentTitle")}>
              {sorted.map((q) => (
                <li key={q.id} className="qs-row">
                  <button type="button" className="qs-item" onClick={() => restore(q.id)} title={t("savedQueues.restore")} data-testid="queue-selector-item">
                    <Artwork id={q.coverArt} size={64} className="art" />
                    <span className="grow" style={{ minWidth: 0 }}>
                      <span className="truncate qs-name">{q.label}</span>
                      <span className="qs-sub truncate">{q.pinned ? `${t("savedQueues.pinned")} · ` : ""}{t("savedQueues.tracks", { count: q.trackCount })} · {fmtRelative(q.lastInteractedAt)}</span>
                    </span>
                  </button>
                  <button type="button" className="qs-more" aria-haspopup="menu" aria-label={t("queue.savedOptions", { name: q.label })} title={t("misc.more")} onClick={(e) => openMenuFromButton(e, { type: "savedQueue", data: { id: q.id } }, { savedQueueId: q.id })} data-testid="queue-selector-more"><Icon name="moreVert" size={18} /></button>
                </li>
              ))}
            </ul>
          ) : <p className="qs-empty">{t("savedQueues.empty")}</p>}
        </div>,
        host,
      ) : null}
    </>
  );
}
