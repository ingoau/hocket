import { useState } from "react";
import { Icon } from "./Icon";
import { t } from "@shared/strings";

export function Stars({ value, onChange, size = 14 }: { value: number; onChange?: (rating: number) => void; size?: number }) {
  const [hover, setHover] = useState<number | undefined>(undefined);
  const shown = hover ?? value;
  return (
    <span className="stars" role={onChange ? "radiogroup" : undefined} aria-label={t("player.rate")} onMouseLeave={() => setHover(undefined)}>
      {[1, 2, 3, 4, 5].map((n) => (
        <span
          key={n}
          className={`star ${n <= shown ? "on" : ""}`}
          role={onChange ? "radio" : undefined}
          aria-checked={onChange ? n === value : undefined}
          onMouseEnter={onChange ? () => setHover(n) : undefined}
          onClick={onChange ? (e) => { e.stopPropagation(); onChange(n === value ? 0 : n); } : undefined}
          style={{ display: "inline-flex", cursor: onChange ? "pointer" : "default" }}
        >
          <Icon name="star" size={size} style={{ fill: n <= shown ? "currentColor" : "none" }} />
        </span>
      ))}
    </span>
  );
}

export function Heart({ on, onToggle, size = 16 }: { on: boolean; onToggle?: () => void; size?: number }) {
  return (
    <button type="button" className={`btn icon sm heart ${on ? "on" : ""}`} aria-pressed={on} aria-label={on ? t("player.unlove") : t("player.love")} onClick={(e) => { e.stopPropagation(); onToggle?.(); }} title={on ? t("player.unlove") : t("player.love")}>
      <Icon name="heart" size={size} style={{ fill: on ? "currentColor" : "none" }} />
    </button>
  );
}
