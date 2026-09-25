// Right panel: queue above, lyrics below, draggable divider remembered in
// localStorage, either side collapsible to give the other full height.
import { useEffect, useRef, useState } from "react";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { QueuePanel } from "./QueuePanel";
import { SavedQueues } from "./SavedQueues";
import { LyricsView } from "./LyricsView";
import { EmptyState } from "./EmptyState";
import { Icon } from "./Icon";
import { Tabs, tabPanelProps } from "./Tabs";
import { bridge } from "../core/bridge";
import { SK } from "@shared/settings-keys";
import { NARROW, useMediaQuery } from "../lib/media";

export function RightPanel() {
  const panels = useApp((s) => s.panels);
  const setPanels = useApp((s) => s.setPanels);
  const tab = useApp((s) => s.queueTab);
  const setTab = useApp((s) => s.setQueueTab);
  const [drag, setDrag] = useState(false);
  const [widthDrag, setWidthDrag] = useState<{ x: number; w: number } | undefined>(undefined);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!drag) return;
    const move = (e: MouseEvent) => {
      const r = ref.current?.getBoundingClientRect();
      if (!r) return;
      setPanels({ splitRatio: Math.max(0.15, Math.min(0.85, (e.clientY - r.top) / r.height)) });
    };
    const up = () => setDrag(false);
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    return () => { window.removeEventListener("mousemove", move); window.removeEventListener("mouseup", up); };
  }, [drag, setPanels]);
  useEffect(() => {
    if (!widthDrag) return;
    const move = (e: MouseEvent) => setPanels({ rightWidth: Math.max(260, Math.min(560, widthDrag.w - (e.clientX - widthDrag.x))) });
    const up = () => setWidthDrag(undefined);
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    return () => { window.removeEventListener("mousemove", move); window.removeEventListener("mouseup", up); };
  }, [widthDrag, setPanels]);

  const narrow = useMediaQuery(NARROW);
  const drawerOpen = useApp((s) => s.drawerOpen);
  const setDrawerOpen = useApp((s) => s.setDrawerOpen);
  if (narrow ? !drawerOpen : !panels.rightOpen) return null;
  const qc = panels.queueCollapsed;
  const lc = panels.lyricsCollapsed;
  const queueFlex = qc ? "0 0 32px" : lc ? "1 1 auto" : `${panels.splitRatio} 1 0`;
  const lyricsFlex = lc ? "0 0 32px" : qc ? "1 1 auto" : `${1 - panels.splitRatio} 1 0`;
  return (
    <aside ref={ref} id="side-panel" className={`right-panel ${narrow ? "drawer" : ""}`} aria-label={t("a11y.sidePanel")} data-testid="right-panel"
      onKeyDown={narrow ? (e) => { if (e.key === "Escape" && !e.defaultPrevented) { e.preventDefault(); e.stopPropagation(); setDrawerOpen(false); document.querySelector<HTMLElement>('[data-testid="toggle-side-panel"]')?.focus(); } } : undefined}>
      <div className="resize-handle" style={{ left: -3, right: "auto" }} onMouseDown={(e) => { e.preventDefault(); setWidthDrag({ x: e.clientX, w: panels.rightWidth }); }} role="separator" aria-orientation="vertical" aria-label={t("a11y.resizeSidePanel")} aria-valuenow={panels.rightWidth} aria-valuemin={260} aria-valuemax={560} tabIndex={0}
        onKeyDown={(e) => { const d = e.key === "ArrowLeft" ? 16 : e.key === "ArrowRight" ? -16 : 0; if (!d) return; e.preventDefault(); e.stopPropagation(); setPanels({ rightWidth: Math.max(260, Math.min(560, panels.rightWidth + d)) }); }} />
      <section className={`pane ${qc ? "collapsed" : ""}`} style={{ flex: queueFlex }} aria-label={t("queue.title")} data-testid="queue-pane">
        <div className="pane-head">
          <Tabs id="queue-tabs" selected={tab} onSelect={(k) => { setTab(k); if (qc) setPanels({ queueCollapsed: false }); }} tabs={[{ key: "queue", label: t("queue.title"), testId: "tab-queue" }, { key: "recent", label: t("queue.recent"), testId: "tab-recent" }]} />
          <span className="spacer" />
          {tab === "queue" && !qc ? <button type="button" className="btn icon sm" title={t("queue.clear")} aria-label={t("queue.clear")} onClick={() => bridge().dispatch({ type: "clearQueue" })}><Icon name="trash" size={13} /></button> : null}
          <button type="button" className="btn icon sm" title={qc ? t("queue.expand") : t("queue.collapse")} aria-label={qc ? t("queue.expand") : t("queue.collapse")} aria-expanded={!qc} onClick={() => setPanels({ queueCollapsed: !qc, lyricsCollapsed: !qc ? lc : false })} data-testid="collapse-queue"><Icon name={qc ? "chevronDown" : "chevronUp"} size={13} /></button>
        </div>
        {!qc ? <div className="tab-panel" {...tabPanelProps("queue-tabs", tab)}>{tab === "queue" ? <QueuePanel /> : <SavedQueues />}</div> : null}
      </section>
      {!qc && !lc ? <div className={`resize-handle h ${drag ? "dragging" : ""}`} onMouseDown={(e) => { e.preventDefault(); setDrag(true); }} role="separator" aria-orientation="horizontal" aria-label={t("a11y.resizeSplit")} aria-valuenow={Math.round(panels.splitRatio * 100)} aria-valuemin={15} aria-valuemax={85} tabIndex={0}
        onKeyDown={(e) => { const d = e.key === "ArrowDown" ? 0.05 : e.key === "ArrowUp" ? -0.05 : 0; if (!d) return; e.preventDefault(); e.stopPropagation(); setPanels({ splitRatio: Math.round(Math.max(0.15, Math.min(0.85, panels.splitRatio + d)) * 100) / 100 }); }}
        data-testid="panel-divider" /> : null}
      <section className={`pane ${lc ? "collapsed" : ""}`} style={{ flex: lyricsFlex }} aria-label={t("lyrics.title")} data-testid="lyrics-pane">
        <div className="pane-head">
          <h2 className="pane-title">{t("lyrics.title")}</h2>
          <span className="spacer" />
          <button type="button" className="btn icon sm" title={lc ? t("lyrics.expand") : t("lyrics.collapse")} aria-label={lc ? t("lyrics.expand") : t("lyrics.collapse")} aria-expanded={!lc} onClick={() => setPanels({ lyricsCollapsed: !lc, queueCollapsed: !lc ? qc : false })} data-testid="collapse-lyrics"><Icon name={lc ? "chevronUp" : "chevronDown"} size={13} /></button>
        </div>
        {!lc ? <LyricsPane /> : null}
      </section>
    </aside>
  );
}

export function LyricsPane({ variant = "compact" }: { variant?: "compact" | "large" }) {
  const now = useApp((s) => s.nowPlaying);
  const lyrics = useApp((s) => s.lyrics);
  const external = useApp((s) => s.settings[SK.lyricsExternalEnabled]?.value === "true");
  if (!now) return <EmptyState message={t("lyrics.nothingPlaying")} icon="lyrics" />;
  const current = lyrics && lyrics.trackId === now.track.id ? lyrics.lyrics : undefined;
  if (!current) return <EmptyState message={t("lyrics.none")} icon="lyrics" action={!external ? <span className="small faint">{t("lyrics.noneHint")}</span> : undefined} testId="lyrics-empty" />;
  return <LyricsView lyrics={current} variant={variant} />;
}
