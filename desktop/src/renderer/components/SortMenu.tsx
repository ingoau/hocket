// A library list's sort control: one tonal button naming the current sort and its direction
// ("Recently added ↓") that opens an anchored popover, the desktop counterpart of an M3 Expressive
// bottom sheet: a "Sort by" heading over the orders as a connected list (a leading icon each, a check
// on the chosen one), then the direction as a connected segmented pair worded for the order (A–Z,
// Oldest, Fewest…). A choice applies at once and the popover stays open for the other one.
// Keyboard: the arrows move (Left/Right between the directions), Home/End, Enter/Space choose,
// Escape or Tab close it and focus goes back to the button.
import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { t } from "@shared/strings";
import { directionLabels, sortOptions, type ListId, type ListSort } from "../lib/list-sort";
import { usePresence } from "../lib/presence";
import { Icon } from "./Icon";

const WIDTH = 264;

export function SortMenu({ list, value, onChange, testId }: { list: ListId; value: ListSort; onChange: (v: ListSort) => void; testId: string }) {
  const options = sortOptions(list);
  const current = options.find((o) => o.value === value.sort) ?? options[0]!;
  const [ascLabel, descLabel] = directionLabels(current.words);
  const dirLabel = t(value.descending ? descLabel : ascLabel);
  const [open, setOpen] = useState(false);
  const [at, setAt] = useState<{ right: number; top?: number; bottom?: number; maxHeight: number } | undefined>(undefined);
  const button = useRef<HTMLButtonElement>(null);
  const pop = useRef<HTMLDivElement>(null);
  const presence = usePresence(open ? at : undefined, pop);
  const shown = presence.value;
  const headingId = useId();
  const dirId = useId();

  const close = (refocus = true) => {
    setOpen(false);
    setAt(undefined);
    if (refocus) button.current?.focus({ preventScroll: true });
  };
  // Right-aligned with the button (it ends its header row): below it, or above where there's no room
  // below but more above; where neither side fits it scrolls within the room it has.
  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      const r = button.current?.getBoundingClientRect();
      if (!r) return;
      if (r.bottom < 0 || r.top > window.innerHeight) { close(false); return; }
      const right = Math.max(8, window.innerWidth - r.right);
      const height = 120 + 38 * options.length;
      const below = window.innerHeight - r.bottom - 14;
      const above = r.top - 14;
      setAt(below >= height || below >= above ? { right, top: r.bottom + 6, maxHeight: below } : { right, bottom: window.innerHeight - r.top + 6, maxHeight: above });
    };
    place();
    // It follows the button while the page scrolls under it, and closes once the button leaves the window.
    const scroll = (e: Event) => { if (!pop.current?.contains(e.target as Node)) place(); };
    window.addEventListener("resize", place);
    window.addEventListener("scroll", scroll, true);
    return () => {
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", scroll, true);
    };
  }, [open, options.length]);
  const placed = !!at;
  useEffect(() => {
    if (open && placed) pop.current?.querySelector<HTMLElement>('[role="menuitemradio"][aria-checked="true"]')?.focus({ preventScroll: true });
  }, [open, placed]);
  useEffect(() => {
    if (!open) return;
    const down = (e: PointerEvent) => {
      const target = e.target as Node;
      if (pop.current?.contains(target) || button.current?.contains(target)) return;
      close(false);
    };
    document.addEventListener("pointerdown", down, true);
    return () => document.removeEventListener("pointerdown", down, true);
  }, [open]);

  const onKey = (e: React.KeyboardEvent) => {
    const items = [...(pop.current?.querySelectorAll<HTMLElement>('[role="menuitemradio"]') ?? [])];
    const i = items.indexOf(document.activeElement as HTMLElement);
    const inDir = (el: HTMLElement | undefined) => !!el?.closest(".sort-dir");
    let to = -1;
    if (e.key === "ArrowDown") to = Math.min(items.length - 1, inDir(items[i]) ? i : i + 1);
    else if (e.key === "ArrowUp") to = inDir(items[i]) ? items.length - 3 : Math.max(0, i - 1);
    else if (e.key === "ArrowRight" && inDir(items[i])) to = items.length - 1;
    else if (e.key === "ArrowLeft" && inDir(items[i])) to = items.length - 2;
    else if (e.key === "Home") to = 0;
    else if (e.key === "End") to = items.length - 1;
    if (to >= 0) {
      e.preventDefault();
      e.stopPropagation();
      // Down from the last order lands on the chosen direction.
      if (e.key === "ArrowDown" && !inDir(items[i]) && inDir(items[to])) to = items.length - (value.descending ? 1 : 2);
      items[to]?.focus();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    } else if (e.key === "Tab") {
      e.preventDefault();
      close();
    } else if (e.key.startsWith("Arrow")) e.stopPropagation();
  };

  const choose = (next: ListSort) => {
    if (next.sort !== value.sort || next.descending !== value.descending) onChange(next);
  };
  return (
    <>
      <button ref={button} type="button" className={`btn tonal sort-button ${open ? "open" : ""}`} aria-haspopup="menu" aria-expanded={open}
        aria-label={t("sort.button", { sort: t(current.label), direction: dirLabel })}
        onClick={() => (open ? close(false) : setOpen(true))}
        onKeyDown={(e) => { if (e.key === "ArrowDown" && !open) { e.preventDefault(); e.stopPropagation(); setOpen(true); } }}
        data-testid={testId} data-sort={value.sort} data-descending={value.descending}>
        <span>{t(current.label)}</span>
        <Icon name={value.descending ? "arrowDown" : "arrowUp"} size={18} />
      </button>
      {shown ? createPortal(
        <div {...presence.exitProps} className={`menu sort-pop ${shown.bottom !== undefined ? "above" : "below"} ${presence.motion}`} role="menu" aria-labelledby={headingId} tabIndex={-1}
          style={{ right: shown.right, top: shown.top, bottom: shown.bottom, maxHeight: shown.maxHeight, width: WIDTH }} onKeyDown={onKey} data-testid={`${testId}-menu`}>
          <div role="group" aria-labelledby={headingId} className="sort-options">
            <div id={headingId} className="sort-heading" role="presentation">{t("sort.heading")}</div>
            {options.map((o) => {
              const on = o.value === current.value;
              return (
                <button key={o.value} type="button" role="menuitemradio" aria-checked={on} tabIndex={-1} className={`sort-row ${on ? "on" : ""}`}
                  onClick={() => choose({ sort: o.value, descending: value.descending })} data-testid="sort-option" data-value={o.value}>
                  <span className="sort-row-icon" aria-hidden="true"><Icon name={o.icon} size={18} filled={on} /></span>
                  <span className="sort-row-label">{t(o.label)}</span>
                  <span className="sort-row-check" aria-hidden="true">{on ? <Icon name="check" size={18} /> : null}</span>
                </button>
              );
            })}
          </div>
          <div role="group" aria-labelledby={dirId} className="sort-dir-block">
            <div id={dirId} className="sort-heading" role="presentation">{t("sort.direction")}</div>
            <div className="seg-group sort-dir" role="presentation">
              {([false, true] as const).map((desc) => (
                <button key={String(desc)} type="button" role="menuitemradio" aria-checked={value.descending === desc} tabIndex={-1} className={`seg ${value.descending === desc ? "on" : ""}`}
                  onClick={() => choose({ sort: value.sort, descending: desc })} data-testid={desc ? "sort-descending" : "sort-ascending"}>
                  <Icon name={desc ? "arrowDown" : "arrowUp"} size={16} />
                  {t(desc ? descLabel : ascLabel)}
                </button>
              ))}
            </div>
          </div>
        </div>,
        document.body,
      ) : null}
    </>
  );
}
