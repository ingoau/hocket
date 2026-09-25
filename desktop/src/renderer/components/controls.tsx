// Material 3 Expressive form controls, drawn by us rather than the platform:
//
// - Slider: a thick rounded track split by a gap around a bar-shaped handle,
//   a stop dot at the end, an optional value bubble. A transparent native
//   <input type="range"> lies over it, so the keyboard, assistive tech,
//   form semantics and tests behave exactly as with the plain element.
// - Switch: an M3 switch (outlined when off; filled with a white handle
//   carrying a check when on) over a transparent native checkbox (role
//   "switch"), for the same reasons.
// - Select: a select-only combobox (ARIA 1.2 pattern) with an M3 menu of
//   options, replacing the platform's <select> popup. It takes the same
//   <option> children and calls onChange with `{ target: { value } }`, so
//   call sites only swap the tag.
import { Children, isValidElement, useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties, type ReactElement, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Icon } from "./Icon";
import { usePresence } from "../lib/presence";

// ---- Slider ---------------------------------------------------------------------------------

export interface SliderProps {
  value: number;
  min?: number;
  max?: number;
  step?: number | "any";
  onChange: (value: number) => void;
  disabled?: boolean;
  /** `sm` for tight rows (the player bar), `md` elsewhere. */
  size?: "sm" | "md";
  /** Vertical sliders grow upwards (an equaliser's bands). */
  orientation?: "horizontal" | "vertical";
  /** Shown in a bubble over the handle while it is dragged or keyboard-focused. */
  bubble?: ReactNode;
  className?: string;
  style?: CSSProperties;
  "aria-label"?: string;
  "aria-valuetext"?: string;
  "data-testid"?: string;
  onKeyDown?: React.KeyboardEventHandler<HTMLInputElement>;
  onWheel?: React.WheelEventHandler<HTMLDivElement>;
}

export function Slider({ value, min = 0, max = 1, step, onChange, disabled, size = "md", orientation = "horizontal", bubble, className, style, onKeyDown, onWheel, ...aria }: SliderProps) {
  const f = max > min ? Math.max(0, Math.min(1, (value - min) / (max - min))) : 0;
  return (
    <div className={`m3-slider ${size} ${orientation === "vertical" ? "vertical" : ""} ${disabled ? "disabled" : ""} ${className ?? ""}`} style={{ ...style, "--f": f } as CSSProperties} onWheel={onWheel}>
      <span className="m3-slider-active" aria-hidden="true" />
      <span className="m3-slider-inactive" aria-hidden="true" />
      <span className="m3-slider-handle" aria-hidden="true">{bubble !== undefined ? <span className="m3-slider-bubble">{bubble}</span> : null}</span>
      <input type="range" className="m3-slider-input" min={min} max={max} step={step} value={value} disabled={disabled} onChange={(e) => onChange(Number(e.target.value))} onKeyDown={onKeyDown} aria-orientation={orientation === "vertical" ? "vertical" : undefined} {...aria} />
    </div>
  );
}

// ---- Switch ---------------------------------------------------------------------------------

export interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  id?: string;
  className?: string;
  "aria-label"?: string;
  "aria-describedby"?: string;
  "data-testid"?: string;
}

export function Switch({ checked, onChange, disabled, id, className, ...aria }: SwitchProps) {
  return (
    <span className={`m3-switch ${checked ? "on" : ""} ${disabled ? "disabled" : ""} ${className ?? ""}`}>
      <input type="checkbox" role="switch" className="m3-switch-input" id={id} checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} {...aria} />
      <span className="m3-switch-track" aria-hidden="true">
        <span className="m3-switch-handle">{checked ? <Icon name="check" size={16} /> : null}</span>
      </span>
    </span>
  );
}

// ---- Select ---------------------------------------------------------------------------------

interface Opt {
  value: string;
  label: ReactNode;
  text: string;
  disabled: boolean;
}

const textOf = (n: ReactNode): string => (typeof n === "string" || typeof n === "number" ? String(n) : Array.isArray(n) ? n.map(textOf).join("") : isValidElement<{ children?: ReactNode }>(n) ? textOf(n.props.children) : "");

function optionsOf(children: ReactNode): Opt[] {
  const out: Opt[] = [];
  Children.forEach(children, (c) => {
    if (!isValidElement(c)) return;
    const el = c as ReactElement<{ value?: string | number; children?: ReactNode; disabled?: boolean }>;
    if (el.type === "option") out.push({ value: String(el.props.value ?? textOf(el.props.children)), label: el.props.children, text: textOf(el.props.children), disabled: !!el.props.disabled });
    else if (el.props.children) out.push(...optionsOf(el.props.children));
  });
  return out;
}

export interface SelectProps {
  value: string | number;
  onChange: (e: { target: { value: string } }) => void;
  children: ReactNode;
  disabled?: boolean;
  className?: string;
  style?: CSSProperties;
  id?: string;
  "aria-label"?: string;
  "aria-labelledby"?: string;
  "data-testid"?: string;
}

/** A select-only combobox with an M3 menu. Keys: Enter/Space/Alt+↓/↑/↓ open; ↑/↓/Home/End/type-ahead move; Enter/Space pick; Escape/Tab close. */
export function Select({ value, onChange, children, disabled, className, style, id, ...aria }: SelectProps) {
  const options = optionsOf(children);
  const current = options.find((o) => o.value === String(value));
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [pos, setPos] = useState<{ left: number; top?: number; bottom?: number; minWidth: number; maxHeight: number } | undefined>(undefined);
  const button = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const typed = useRef({ text: "", at: 0 });
  const listId = useId();
  const optId = (i: number) => `${listId}-o${i}`;
  // The closed menu stays mounted, inert, while its exit plays.
  const menu = usePresence(open || undefined, list);

  const place = () => {
    const r = button.current?.getBoundingClientRect();
    if (!r) return;
    const below = window.innerHeight - r.bottom - 12;
    const above = r.top - 12;
    const up = below < 220 && above > below;
    setPos({ left: Math.max(8, Math.min(r.left, window.innerWidth - Math.max(r.width, 180) - 8)), minWidth: r.width, maxHeight: Math.min(360, up ? above : below), ...(up ? { bottom: window.innerHeight - r.top + 6 } : { top: r.bottom + 6 }) });
  };
  const openMenu = (at?: number) => {
    if (disabled || !options.length) return;
    place();
    const i = at ?? Math.max(0, options.findIndex((o) => o.value === String(value)));
    setActive(i);
    setOpen(true);
  };
  const close = (refocus = true) => {
    setOpen(false);
    if (refocus) button.current?.focus({ preventScroll: true });
  };
  const pick = (i: number) => {
    const o = options[i];
    if (!o || o.disabled) return;
    if (o.value !== String(value)) onChange({ target: { value: o.value } });
    close();
  };
  const move = (from: number, dir: 1 | -1) => {
    for (let k = 1; k <= options.length; k++) {
      const i = (from + dir * k + options.length) % options.length;
      if (!options[i]?.disabled) return i;
    }
    return from;
  };
  const typeahead = (key: string) => {
    const now = Date.now();
    typed.current = { text: (now - typed.current.at < 700 ? typed.current.text : "") + key.toLowerCase(), at: now };
    const start = open ? active : Math.max(0, options.findIndex((o) => o.value === String(value)));
    for (let k = 0; k < options.length; k++) {
      const i = (start + (typed.current.text.length === 1 ? 1 : 0) + k) % options.length;
      if (options[i]?.text.toLowerCase().startsWith(typed.current.text)) return i;
    }
    return undefined;
  };

  // Close on an outside pointer; follow the field on scroll and resize.
  useEffect(() => {
    if (!open) return;
    const down = (e: PointerEvent) => {
      const t = e.target as Node;
      if (!list.current?.contains(t) && !button.current?.contains(t)) close(false);
    };
    // Scrolling elsewhere (or a live page nudging its scroll position) keeps
    // the menu with its field; it only closes once the field leaves the window.
    let raf = 0;
    const follow = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => {
        const r = button.current?.getBoundingClientRect();
        if (!r || r.bottom < 0 || r.top > window.innerHeight) close(false);
        else place();
      });
    };
    const scroll = (e: Event) => { if (!list.current?.contains(e.target as Node)) follow(); };
    const resize = follow;
    document.addEventListener("pointerdown", down, true);
    window.addEventListener("scroll", scroll, true);
    window.addEventListener("resize", resize);
    return () => {
      document.removeEventListener("pointerdown", down, true);
      window.removeEventListener("scroll", scroll, true);
      window.removeEventListener("resize", resize);
      cancelAnimationFrame(raf);
    };
  }, [open]);
  useLayoutEffect(() => {
    if (open) list.current?.querySelector<HTMLElement>(`#${CSS.escape(optId(active))}`)?.scrollIntoView({ block: "nearest" });
  });

  const onKey = (e: React.KeyboardEvent) => {
    if (disabled) return;
    const k = e.key;
    let handled = true;
    if (!open) {
      if (k === "Enter" || k === " " || k === "ArrowDown" || k === "ArrowUp") openMenu();
      else if (k === "Home") openMenu(0);
      else if (k === "End") openMenu(options.length - 1);
      else if (k.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) { const i = typeahead(k); if (i !== undefined) openMenu(i); }
      else handled = false;
    } else if (k === "ArrowDown") setActive(move(active, 1));
    else if (k === "ArrowUp") setActive(e.altKey ? active : move(active, -1));
    else if (k === "Home") setActive(0);
    else if (k === "End") setActive(options.length - 1);
    else if (k === "PageDown") setActive(Math.min(options.length - 1, active + 8));
    else if (k === "PageUp") setActive(Math.max(0, active - 8));
    else if (k === "Enter" || (k === " " && !typed.current.text)) pick(active);
    else if (k === "Escape") close();
    else if (k === "Tab") { setOpen(false); handled = false; }
    else if (k.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) { const i = typeahead(k); if (i !== undefined) setActive(i); }
    else handled = false;
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  };

  return (
    <>
      <button ref={button} type="button" id={id} role="combobox" aria-haspopup="listbox" aria-expanded={open} aria-controls={open ? listId : undefined} aria-activedescendant={open ? optId(active) : undefined} disabled={disabled}
        className={`m3-select ${open ? "open" : ""} ${className ?? ""}`} style={style} data-value={String(value)} onClick={() => (open ? close() : openMenu())} onKeyDown={onKey} onBlur={(e) => { if (open && !list.current?.contains(e.relatedTarget as Node)) setOpen(false); }} {...aria}>
        <span className="m3-select-value">{current?.label ?? ""}</span>
        <Icon name="chevronDown" size={20} className="m3-select-chevron" />
      </button>
      {menu.value && pos ? createPortal(
        <div {...menu.exitProps} id={listId} role="listbox" className={`m3-menu m3-select-menu ${menu.closing ? "closing" : ""}`} aria-label={aria["aria-label"]} aria-labelledby={aria["aria-labelledby"]} style={{ left: pos.left, top: pos.top, bottom: pos.bottom, minWidth: pos.minWidth, maxHeight: pos.maxHeight }} data-testid={aria["data-testid"] ? `${aria["data-testid"]}-menu` : undefined}
          onMouseDown={(e) => e.preventDefault()}>
          {options.map((o, i) => (
            <div key={o.value} id={optId(i)} role="option" aria-selected={o.value === String(value)} aria-disabled={o.disabled || undefined} className={`m3-option ${i === active ? "active" : ""} ${o.value === String(value) ? "selected" : ""}`} data-value={o.value}
              onPointerEnter={() => setActive(i)} onClick={() => pick(i)}>
              <span className="m3-option-check" aria-hidden="true">{o.value === String(value) ? <Icon name="check" size={18} /> : null}</span>
              <span className="m3-option-label">{o.label}</span>
            </div>
          ))}
        </div>,
        document.body,
      ) : null}
    </>
  );
}
