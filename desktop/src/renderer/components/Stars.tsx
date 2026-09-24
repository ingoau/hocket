import { useRef, useState } from "react";
import { Icon } from "./Icon";
import { t } from "@shared/strings";
import { ratingText } from "../lib/a11y";

/**
 * Star rating as an ARIA radio group ("Rating for <title>", each star "3 stars").
 * One Tab stop (the checked star, or the first), arrows move and rate, Space
 * or Enter on a star rates it (again on the current rating clears it), Delete
 * or 0 clears. Inside grid rows `tabbable` is false: the row is the Tab stop
 * and the 0–5 keys rate the row.
 */
export function Stars({ value, onChange, size = 14, label, tabbable = true }: { value: number; onChange?: (rating: number) => void; size?: number; label?: string; tabbable?: boolean }) {
  const [hover, setHover] = useState<number | undefined>(undefined);
  const shown = hover ?? value;
  const ref = useRef<HTMLSpanElement>(null);
  const name = label ?? t("player.rate");
  if (!onChange) {
    return (
      <span className="stars" role="img" aria-label={`${name}: ${ratingText(value)}`}>
        {[1, 2, 3, 4, 5].map((n) => <span key={n} className={`star ${n <= value ? "on" : ""}`}><Icon name="star" size={size} style={{ fill: n <= value ? "currentColor" : "none" }} /></span>)}
      </span>
    );
  }
  const focusStar = (n: number) => ref.current?.querySelector<HTMLElement>(`[data-star="${n}"]`)?.focus();
  const onKey = (e: React.KeyboardEvent, n: number) => {
    let next: number | undefined;
    if (e.key === "ArrowRight" || e.key === "ArrowUp") next = Math.min(5, n + 1);
    else if (e.key === "ArrowLeft" || e.key === "ArrowDown") next = Math.max(1, n - 1);
    else if (e.key === "Home") next = 1;
    else if (e.key === "End") next = 5;
    else if (e.key === "Delete" || e.key === "Backspace" || e.key === "0") {
      e.preventDefault();
      e.stopPropagation();
      onChange(0);
      return;
    }
    if (next === undefined) return;
    e.preventDefault();
    e.stopPropagation();
    onChange(next);
    focusStar(next);
  };
  const tabStop = value >= 1 && value <= 5 ? value : 1;
  return (
    <span ref={ref} className="stars" role="radiogroup" aria-label={name} onMouseLeave={() => setHover(undefined)}>
      {[1, 2, 3, 4, 5].map((n) => (
        <button
          type="button"
          key={n}
          data-star={n}
          className={`star ${n <= shown ? "on" : ""}`}
          role="radio"
          aria-checked={n === value}
          aria-label={ratingText(n)}
          tabIndex={tabbable && n === tabStop ? 0 : -1}
          onMouseEnter={() => setHover(n)}
          onKeyDown={(e) => onKey(e, n)}
          onClick={(e) => { e.stopPropagation(); onChange(n === value ? 0 : n); }}
        >
          <Icon name="star" size={size} style={{ fill: n <= shown ? "currentColor" : "none" }} />
        </button>
      ))}
    </span>
  );
}

export function Heart({ on, onToggle, size = 16, tabbable = true }: { on: boolean; onToggle?: () => void; size?: number; tabbable?: boolean }) {
  return (
    <button type="button" className={`btn icon sm heart ${on ? "on" : ""}`} aria-pressed={on} aria-label={t("player.love")} tabIndex={tabbable ? undefined : -1} onClick={(e) => { e.stopPropagation(); onToggle?.(); }} title={on ? t("player.unlove") : t("player.love")}>
      <Icon name="heart" size={size} style={{ fill: on ? "currentColor" : "none" }} />
    </button>
  );
}
