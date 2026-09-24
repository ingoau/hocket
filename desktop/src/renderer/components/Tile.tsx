// Artwork tiles outside the virtualised grid (home shelves, an artist's
// albums, search results): a list with one Tab stop, arrow keys between tiles
// (lib/roving.ts), Enter opens, Shift+F10 opens the tile's menu. The hover
// play button is a pointer shortcut and not a Tab stop (the menu has Play).
import { useRef, type ReactNode } from "react";
import { t } from "@shared/strings";
import { useRoving } from "../lib/roving";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";

export function TileList({ label, className = "", children, testId }: { label: string; className?: string; children: ReactNode; testId?: string }) {
  const ref = useRef<HTMLUListElement>(null);
  const roving = useRoving(ref);
  return (
    <ul ref={ref} className={`plain-list ${className}`} aria-label={label} {...roving} data-testid={testId}>
      {children}
    </ul>
  );
}

export function Tile({ title, subtitle, coverArt, round, onOpen, onActivate, onPlay, onContextMenu, testId }: {
  title: string;
  subtitle?: string;
  coverArt?: string;
  round?: boolean;
  /** Single click (and Enter). */
  onOpen?: () => void;
  /** Double click, and Enter when there is no onOpen. */
  onActivate?: () => void;
  onPlay?: () => void;
  onContextMenu?: (e: React.MouseEvent) => void;
  testId?: string;
}) {
  return (
    <li className="tile" onContextMenu={onContextMenu} data-testid={testId}>
      <button
        type="button"
        className="tile-main"
        data-roving
        aria-label={subtitle ? `${title}, ${subtitle}` : title}
        // A keyboard "click" has detail 0: Enter/Space activate even when the pointer needs a double click.
        onClick={(e) => { if (onOpen) onOpen(); else if (e.detail === 0) onActivate?.(); }}
        onDoubleClick={onActivate ?? onOpen}
      >
        <Artwork id={coverArt} size={300} round={round} />
        <span className="t1" title={title}>{title}</span>
        {subtitle ? <span className="t2" title={subtitle}>{subtitle}</span> : null}
      </button>
      {onPlay ? <button type="button" className="play" tabIndex={-1} aria-label={t("a11y.playItem", { title })} onClick={(e) => { e.stopPropagation(); onPlay(); }}><Icon name="play" size={16} style={{ fill: "currentColor" }} /></button> : null}
    </li>
  );
}
