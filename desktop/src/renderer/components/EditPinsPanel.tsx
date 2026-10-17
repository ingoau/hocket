// Edit pins: a side panel beside the sidebar (the phone's pin edit mode), opened from a pin's menu or
// the sidebar's pins. The pins as one connected group of tiles, two across: drag one (or pick it up
// with its handle and the arrow keys) to reorder, × to unpin (with Undo), and the chip to choose what
// a click does. It all edits a draft; Done saves it to `library.pins`, Cancel or Escape drops it.
import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useReturnFocus } from "../lib/focus";
import { showToast } from "../lib/ui-toast";
import { currentPins, savePins } from "../lib/pin-menus";
import { usePins, useResolvedPins } from "../lib/use-pins";
import { choosesTap, pinTap, removePin, restorePin, type LibraryPin } from "../lib/library-pins";
import { cycleTap, draftChanged, draftToSave, moveShownBy, pinKey, placeBeside, tileRadius } from "../lib/pin-editor";
import { Icon } from "./Icon";
import { PinThumb } from "./Sidebar";

export function EditPinsPanel() {
  const open = useApp((s) => s.editingPins);
  return open ? <PinsEditor /> : null;
}

/** Arrow keys while a tile is picked up: along the row, or a row up or down. */
const STEPS: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -2, ArrowDown: 2 };

type FocusPart = "handle" | "unpin" | "heading";

function PinsEditor() {
  const setEditing = useApp((s) => s.setEditingPins);
  const stored = usePins();
  const resolved = useResolvedPins();
  // What was stored when editing began, and the draft every change goes to.
  const [original] = useState(stored);
  const [draft, setDraft] = useState(stored);
  const draftRef = useRef(draft);
  draftRef.current = draft;
  // How editing ended, for an Unpinned toast's Undo pressed after the panel has closed.
  const ended = useRef<"open" | "saved" | "cancelled">("open");
  const heading = useRef<HTMLHeadingElement>(null);
  const handles = useRef(new Map<string, HTMLButtonElement>());
  const unpins = useRef(new Map<string, HTMLButtonElement>());
  const focusNext = useRef<{ key: string; part: FocusPart } | undefined>(undefined);
  // A tile picked up from the keyboard, and the draft from before it was, for Escape.
  const [lifted, setLifted] = useState<{ key: string; from: LibraryPin[] } | undefined>(undefined);
  // A tile being dragged: the draft follows the pointer, and goes back if it isn't dropped on the grid.
  const [dragKey, setDragKey] = useState<string | undefined>(undefined);
  const drag = useRef<{ from: LibraryPin[]; dropped: boolean } | undefined>(undefined);
  const [said, setSaid] = useState("");
  const headingId = useId();
  const keysId = useId();

  // Opening moves focus into the panel; closing returns it to what had it (or the pin, below).
  useReturnFocus(true);
  useEffect(() => heading.current?.focus(), []);
  // After a move or an unpin, focus stays on the tile (React may have moved it) or goes to a neighbour.
  useLayoutEffect(() => {
    const next = focusNext.current;
    if (!next) return;
    focusNext.current = undefined;
    const el = next.part === "heading" ? heading.current : (next.part === "handle" ? handles : unpins).current.get(next.key);
    if (el && document.activeElement !== el) el.focus();
  });

  const byKey = useMemo(() => new Map((resolved ?? []).map((rp) => [pinKey(rp.pin), rp])), [resolved]);
  const shown = draft.filter((p) => byKey.has(pinKey(p)));
  const titleOf = (p: LibraryPin) => byKey.get(pinKey(p))?.title ?? p.id;
  const position = (list: readonly LibraryPin[], p: LibraryPin) => {
    const visible = list.filter((x) => byKey.has(pinKey(x)));
    return { pos: visible.findIndex((x) => pinKey(x) === pinKey(p)) + 1, n: visible.length };
  };

  const close = (save: boolean) => {
    if (save && draftChanged(original, draftRef.current)) savePins(draftToSave(original, draftRef.current, currentPins()));
    ended.current = save ? "saved" : "cancelled";
    // Opened from a pin's menu: focus goes back to that pin, wherever it is now (unless it was unpinned).
    const from = useApp.getState().editPinsFrom;
    setEditing(false);
    if (from) requestAnimationFrame(() => {
      const active = document.activeElement;
      if (active && active !== document.body && active.isConnected) return;
      document.querySelector<HTMLElement>(`[data-testid="sidebar-pin"][data-pin="${CSS.escape(from)}"]`)?.focus({ preventScroll: true });
    });
  };

  const unpin = (p: LibraryPin) => {
    const title = titleOf(p);
    const i = shown.findIndex((x) => pinKey(x) === pinKey(p));
    const rest = shown.filter((x) => pinKey(x) !== pinKey(p));
    const neighbour = rest[Math.min(i, rest.length - 1)];
    focusNext.current = neighbour ? { key: pinKey(neighbour), part: "unpin" } : { key: "", part: "heading" };
    const { pins, at } = removePin(draftRef.current, p);
    setDraft(pins);
    if (lifted?.key === pinKey(p)) setLifted(undefined);
    // Undo puts it back where it was: in the draft while editing, in the Library once Done saved it.
    showToast(t("library.unpinned", { title }), () => {
      if (ended.current === "open") setDraft((d) => restorePin(d, p, at));
      else if (ended.current === "saved") savePins(restorePin(currentPins(), p, at));
    });
  };

  const toggleLift = (p: LibraryPin) => {
    const key = pinKey(p);
    if (lifted?.key === key) {
      setLifted(undefined);
      setSaid(t("library.editPins.dropped", { title: titleOf(p), ...position(draft, p) }));
    } else {
      setLifted({ key, from: draft });
      setSaid(t("library.editPins.picked", { title: titleOf(p), ...position(draft, p) }));
    }
  };

  const onHandleKey = (e: React.KeyboardEvent, p: LibraryPin) => {
    const key = pinKey(p);
    if (lifted?.key !== key) return;
    const by = STEPS[e.key];
    if (by) {
      e.preventDefault();
      e.stopPropagation();
      const next = moveShownBy(draft, p, by, shown);
      focusNext.current = { key, part: "handle" };
      setDraft(next);
      setSaid(t("library.editPins.moved", { title: titleOf(p), ...position(next, p) }));
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      focusNext.current = { key, part: "handle" };
      setDraft(lifted.from);
      setLifted(undefined);
      setSaid(t("library.editPins.putBack", { title: titleOf(p), ...position(lifted.from, p) }));
    } else if (e.key === "Tab") {
      setLifted(undefined);
    }
  };

  const n = shown.length;
  return (
    <section
      className="customise-panel pins-panel"
      role="dialog"
      aria-labelledby={headingId}
      onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); close(false); } }}
      data-testid="edit-pins-panel"
    >
      <div className="customise-head pins-head">
        <button type="button" className="btn icon" aria-label={t("library.editPins.cancel")} onClick={() => close(false)} data-testid="edit-pins-cancel"><Icon name="close" size={18} /></button>
        <h2 className="card-title grow" id={headingId} tabIndex={-1} ref={heading}>{t("library.editPins")}</h2>
        <button type="button" className="btn primary pins-done" onClick={() => close(true)} data-testid="edit-pins-done"><Icon name="check" size={16} /> {t("library.editPins.done")}</button>
      </div>
      <p className="pins-hint"><Icon name="info" size={16} /><span>{t("library.editPins.hint")}</span></p>
      <span id={keysId} hidden>{t("library.editPins.reorderKeys")}</span>
      {resolved ? (
        <ul
          className="plain-list pins-grid"
          aria-label={t("library.editPins.list")}
          // Anywhere on the grid (the gaps too) is a drop; elsewhere the drag is undone on its end.
          onDragOver={(e) => { if (drag.current) e.preventDefault(); }}
          onDrop={(e) => { if (!drag.current) return; e.preventDefault(); drag.current.dropped = true; }}
          data-testid="edit-pins-grid"
        >
          {shown.map((p, i) => {
            const key = pinKey(p);
            const rp = byKey.get(key);
            if (!rp) return null;
            const title = rp.title;
            const tap = pinTap(p);
            const up = lifted?.key === key || dragKey === key;
            return (
              <li
                key={key}
                className={`pin-tile ${up ? "lifted" : ""}`}
                style={{ borderRadius: tileRadius(i, n) }}
                draggable
                onDragStart={(e) => {
                  e.dataTransfer.effectAllowed = "move";
                  e.dataTransfer.setData("text/plain", key);
                  drag.current = { from: draft, dropped: false };
                  setLifted(undefined);
                  setDragKey(key);
                }}
                onDragOver={(e) => {
                  if (!drag.current || !dragKey) return;
                  e.preventDefault();
                  e.dataTransfer.dropEffect = "move";
                  if (dragKey === key) return;
                  // The dragged tile takes this one's place, so the pointer is over it again and nothing jitters.
                  const from = shown.findIndex((x) => pinKey(x) === dragKey);
                  const moving = draft.find((x) => pinKey(x) === dragKey);
                  if (moving) setDraft(placeBeside(draft, moving, p, from < i));
                }}
                onDragEnd={() => {
                  if (drag.current && !drag.current.dropped) setDraft(drag.current.from);
                  drag.current = undefined;
                  setDragKey(undefined);
                }}
                data-testid="edit-pin"
                data-key={key}
              >
                <div className="pin-tile-top">
                  <button
                    type="button"
                    className="btn icon pin-tile-handle"
                    aria-label={t("library.editPins.reorder", { title })}
                    aria-describedby={keysId}
                    aria-pressed={lifted?.key === key}
                    ref={(el) => { if (el) handles.current.set(key, el); else handles.current.delete(key); }}
                    onClick={() => toggleLift(p)}
                    onKeyDown={(e) => onHandleKey(e, p)}
                    // Focus leaving for something else drops it where it is (a move re-focuses it, with no target).
                    onBlur={(e) => { if (lifted?.key === key && e.relatedTarget) setLifted(undefined); }}
                    data-testid="edit-pin-handle"
                  >
                    <Icon name="grip" size={20} />
                  </button>
                  <button
                    type="button"
                    className="pin-tile-unpin"
                    aria-label={t("library.editPins.unpin", { title })}
                    ref={(el) => { if (el) unpins.current.set(key, el); else unpins.current.delete(key); }}
                    onClick={() => unpin(p)}
                    data-testid="edit-pin-unpin"
                  >
                    <span className="pin-tile-x"><Icon name="close" size={14} /></span>
                  </button>
                </div>
                <PinThumb rp={rp} large />
                <span className="pin-tile-title" title={title}>{title}</span>
                <span className="pin-tile-kind">{t(`library.kind.${rp.kind}` as never)}</span>
                {choosesTap(p.kind) ? (
                  <button type="button" className={`pin-chip ${tap}`} aria-label={t(`library.chipLabel.${tap}` as never, { title })} onClick={() => setDraft((d) => cycleTap(d, p))} data-testid="edit-pin-chip" data-tap={tap}>
                    <Icon name={tap === "open" ? "arrowOutward" : tap} size={15} />
                    {t(`library.chip.${tap}` as never)}
                    <Icon name="chevronDown" size={14} />
                  </button>
                ) : (
                  // A song always plays: said, not offered.
                  <span className="pin-chip-fixed"><Icon name="play" size={14} />{t("library.chip.play")}</span>
                )}
              </li>
            );
          })}
        </ul>
      ) : null}
      {resolved ? <p className="pins-foot small muted" data-testid="edit-pins-count">{t(n === 1 ? "library.editPins.count.one" : "library.editPins.count.other", { n })}</p> : null}
      <div className="sr-only" role="status" aria-live="polite">{said}</div>
    </section>
  );
}
