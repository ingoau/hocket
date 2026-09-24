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
import { bridge } from "../core/bridge";
import { SK } from "@shared/settings-keys";

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

  if (!panels.rightOpen) return null;
  const qc = panels.queueCollapsed;
  const lc = panels.lyricsCollapsed;
  const queueFlex = qc ? "0 0 32px" : lc ? "1 1 auto" : `${panels.splitRatio} 1 0`;
  const lyricsFlex = lc ? "0 0 32px" : qc ? "1 1 auto" : `${1 - panels.splitRatio} 1 0`;
  return (
    <aside ref={ref} className="right-panel" data-testid="right-panel">
      <div className="resize-handle" style={{ left: -3, right: "auto" }} onMouseDown={(e) => { e.preventDefault(); setWidthDrag({ x: e.clientX, w: panels.rightWidth }); }} role="separator" aria-orientation="vertical" aria-label="Resize side panel" />
      <section className={`pane ${qc ? "collapsed" : ""}`} style={{ flex: queueFlex }} aria-label={t("queue.title")} data-testid="queue-pane">
        <div className="pane-head">
          <div className="tabs" role="tablist">
            <button type="button" role="tab" aria-selected={tab === "queue"} className={`tab ${tab === "queue" ? "active" : ""}`} onClick={() => { setTab("queue"); if (qc) setPanels({ queueCollapsed: false }); }} data-testid="tab-queue">{t("queue.title")}</button>
            <button type="button" role="tab" aria-selected={tab === "recent"} className={`tab ${tab === "recent" ? "active" : ""}`} onClick={() => { setTab("recent"); if (qc) setPanels({ queueCollapsed: false }); }} data-testid="tab-recent">{t("queue.recent")}</button>
          </div>
          <span className="spacer" />
          {tab === "queue" && !qc ? <button type="button" className="btn icon sm" title={t("queue.clear")} aria-label={t("queue.clear")} onClick={() => bridge().dispatch({ type: "clearQueue" })}><Icon name="trash" size={13} /></button> : null}
          <button type="button" className="btn icon sm" title={qc ? t("queue.expand") : t("queue.collapse")} aria-label={qc ? t("queue.expand") : t("queue.collapse")} onClick={() => setPanels({ queueCollapsed: !qc, lyricsCollapsed: !qc ? lc : false })} data-testid="collapse-queue"><Icon name={qc ? "chevronDown" : "chevronUp"} size={13} /></button>
        </div>
        {!qc ? (tab === "queue" ? <QueuePanel /> : <SavedQueues />) : null}
      </section>
      {!qc && !lc ? <div className={`resize-handle h ${drag ? "dragging" : ""}`} onMouseDown={(e) => { e.preventDefault(); setDrag(true); }} role="separator" aria-orientation="horizontal" aria-label="Resize queue and lyrics" aria-valuenow={Math.round(panels.splitRatio * 100)} data-testid="panel-divider" /> : null}
      <section className={`pane ${lc ? "collapsed" : ""}`} style={{ flex: lyricsFlex }} aria-label={t("lyrics.title")} data-testid="lyrics-pane">
        <div className="pane-head">
          <span className="small" style={{ fontWeight: 600 }}>{t("lyrics.title")}</span>
          <span className="spacer" />
          <button type="button" className="btn icon sm" title={lc ? t("lyrics.expand") : t("lyrics.collapse")} aria-label={lc ? t("lyrics.expand") : t("lyrics.collapse")} onClick={() => setPanels({ lyricsCollapsed: !lc, queueCollapsed: !lc ? qc : false })} data-testid="collapse-lyrics"><Icon name={lc ? "chevronUp" : "chevronDown"} size={13} /></button>
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
  if (!now) return <EmptyState message={t("lyrics.nothingPlaying")} />;
  const current = lyrics && lyrics.trackId === now.track.id ? lyrics.lyrics : undefined;
  if (!current) return <EmptyState message={t("lyrics.none")} action={!external ? <span className="small faint">{t("lyrics.noneHint")}</span> : undefined} testId="lyrics-empty" />;
  return <LyricsView lyrics={current} variant={variant} />;
}
