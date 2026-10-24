// The context menu's Share flyout: a Navidrome public link, a Song.link search page, and the audio
// file. Each row closes the menu and does the share. Open in Genius is a row of the menu itself.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ActionTarget } from "@core/api";
import { t } from "@shared/strings";
import { copyNavidromeShare, copySongLink, shareAudioFile, shareChoices } from "../lib/share";
import type { MenuFacts } from "../lib/menu-model";
import { Icon } from "./Icon";

interface Props {
  facts: MenuFacts;
  target: ActionTarget;
  anchor: { left: number; right: number; top: number };
  closing: boolean;
  motion: string;
  onBack: () => void;
  onClose: (then?: () => void) => void;
}

export function ShareSubmenu({ facts, target, anchor, closing, motion, onBack, onClose }: Props) {
  const choices = shareChoices(facts);
  const rows: { id: string; label: string; icon: string; run: () => void }[] = [];
  if (choices.navidrome) rows.push({ id: "navidrome", label: t("menu.shareNavidrome"), icon: "share", run: () => void copyNavidromeShare(target, facts) });
  if (choices.songLink) rows.push({ id: "songLink", label: t("menu.shareSongLink"), icon: "song", run: () => void copySongLink(target, facts) });
  if (choices.file) rows.push({ id: "file", label: t("menu.shareFile"), icon: "download", run: () => void shareAudioFile(facts.trackIds?.[0]) });
  const [active, setActive] = useState(0);
  const [pos, setPos] = useState<{ x: number; y: number } | undefined>(undefined);
  const ref = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (!ref.current || pos) return;
    const w = ref.current.offsetWidth;
    const h = ref.current.offsetHeight;
    const x = anchor.right - 4 + w <= window.innerWidth - 6 ? anchor.right - 4 : Math.max(4, anchor.left - w + 4);
    setPos({ x, y: Math.max(4, Math.min(anchor.top - 6, window.innerHeight - h - 6)) });
  }, [pos, anchor]);

  useEffect(() => {
    if (pos) ref.current?.focus({ preventScroll: true });
  }, [pos]);

  const run = (i: number) => {
    const row = rows[i];
    if (!row) return;
    onClose(row.run);
  };

  return (
    <div
      ref={ref}
      className={`menu cm cm-sub ${motion} ${closing ? "closing" : ""}`}
      role="menu"
      aria-label={t("menu.share")}
      tabIndex={-1}
      style={pos ? { left: pos.x, top: pos.y } : { left: -9999, top: 0 }}
      data-context-layer=""
      data-testid="share-submenu"
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape" || e.key === "ArrowLeft") {
          e.preventDefault();
          onBack();
          return;
        }
        if (e.key === "ArrowDown" || e.key === "ArrowUp") {
          e.preventDefault();
          setActive((i) => (e.key === "ArrowDown" ? (i + 1) % rows.length : (i + rows.length - 1) % rows.length));
          return;
        }
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          run(active);
        }
      }}
    >
      {rows.map((row, i) => (
        <div
          key={row.id}
          role="menuitem"
          className={`mi ${active === i ? "active" : ""}`}
          data-action={`share-${row.id}`}
          onMouseMove={() => setActive(i)}
          onClick={() => run(i)}
        >
          <span className="cm-lead cm-icon" aria-hidden="true"><Icon name={row.icon} size={15} /></span>
          <span className="grow truncate">{row.label}</span>
        </div>
      ))}
    </div>
  );
}
