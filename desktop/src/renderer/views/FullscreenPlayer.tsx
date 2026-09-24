// Fullscreen player (F): Kawarp background, large cover, transport, tabbed
// side panel Up Next / Related / Lyrics.
// A modal dialog: focus moves in on open (the close button), Tab stays inside,
// Escape closes, and focus returns to what opened it.
import { useEffect, useRef, useState } from "react";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { FluidBackground } from "../components/FluidBackground";
import { Artwork } from "../components/Artwork";
import { Icon } from "../components/Icon";
import { SeekBar } from "../components/PlayerBar";
import { QueuePanel } from "../components/QueuePanel";
import { LyricsPane } from "../components/RightPanel";
import { Heart, Stars } from "../components/Stars";
import { fmtTime } from "../lib/format";
import { Tabs, tabPanelProps } from "../components/Tabs";
import { trapTab, useReturnFocus } from "../lib/focus";

type Tab = "upNext" | "related" | "lyrics";

export function FullscreenPlayer() {
  const now = useApp((s) => s.nowPlaying);
  const transport = useApp((s) => s.transport);
  const queue = useApp((s) => s.queue);
  const setFullscreen = useApp((s) => s.setFullscreen);
  const covered = useApp((s) => !!s.dialog || s.paletteOpen);
  const [tab, setTab] = useState<Tab>("lyrics");
  const closeRef = useRef<HTMLButtonElement>(null);
  useReturnFocus(true);
  useEffect(() => closeRef.current?.focus({ preventScroll: true }), []);
  const d = bridge().dispatch;
  const track = now?.track;
  const playing = transport.position.isPlaying;

  useEffect(() => {
    bridge().window.openFullscreen(true);
    return () => bridge().window.openFullscreen(false);
  }, []);

  const providerLabel = (p: string) => t(`related.provider.${p}` as never);
  const onKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape" && !e.defaultPrevented) {
      e.preventDefault();
      e.stopPropagation();
      setFullscreen(false);
      return;
    }
    trapTab(e);
  };
  return (
    <div className="fullscreen fade-in" role="dialog" aria-modal="true" aria-labelledby="fs-title" inert={covered} onKeyDown={onKey} data-testid="fullscreen-player">
      <FluidBackground coverArt={track?.coverArt} />
      <button ref={closeRef} type="button" className="btn icon close" aria-label={t("fullscreen.exit")} title={t("fullscreen.exit")} onClick={() => setFullscreen(false)} data-testid="fullscreen-exit"><Icon name="close" size={20} /></button>
      <div className="left">
        <Artwork id={track?.coverArt} size={1000} className="cover" />
        <div className="info" style={{ width: "min(52vh, 100%)" }}>
          <h2 className="t1" id="fs-title">{track?.title ?? t("player.nothingPlaying")}</h2>
          <div className="t2">{track ? [track.artist, track.album].filter(Boolean).join(" · ") : ""}</div>
        </div>
        <div className="fs-transport">
          <SeekBar durationMs={track?.durationMs} />
          <div className="transport" style={{ justifyContent: "center", gap: 10 }}>
            {track ? <Heart on={track.loved} size={18} onToggle={() => d({ type: "setLoved", data: { targets: [{ type: "track", data: { id: track.id } }], loved: !track.loved } })} /> : null}
            <button type="button" className={`btn icon ${queue.shuffle ? "on" : ""}`} aria-label={t("player.shuffle")} aria-pressed={queue.shuffle} onClick={() => d({ type: "setShuffle", data: { enabled: !queue.shuffle } })}><Icon name="shuffle" size={18} /></button>
            <button type="button" className="btn icon" aria-label={t("player.previous")} onClick={() => d({ type: "previous" })}><Icon name="previous" size={22} style={{ fill: "currentColor" }} /></button>
            <button type="button" className="btn icon play" aria-label={playing ? t("player.pause") : t("player.play")} onClick={() => d({ type: "togglePlay" })} data-testid="fs-play-pause"><Icon name={playing ? "pause" : "play"} size={22} style={{ fill: "currentColor" }} /></button>
            <button type="button" className="btn icon" aria-label={t("player.next")} onClick={() => d({ type: "next" })}><Icon name="next" size={22} style={{ fill: "currentColor" }} /></button>
            <button type="button" className={`btn icon ${queue.repeat !== "off" ? "on" : ""}`} aria-label={queue.repeat === "off" ? t("player.repeatOff") : queue.repeat === "all" ? t("player.repeatAll") : t("player.repeatOne")} onClick={() => d({ type: "setRepeat", data: { mode: queue.repeat === "off" ? "all" : queue.repeat === "all" ? "one" : "off" } })}><Icon name={queue.repeat === "one" ? "repeatOne" : "repeat"} size={18} /></button>
            {track ? <Stars value={track.rating} size={16} label={t("a11y.ratingOf", { title: track.title })} onChange={(r) => d({ type: "setRating", data: { targets: [{ type: "track", data: { id: track.id } }], rating: r } })} /> : null}
          </div>
        </div>
      </div>
      <div className="side">
        <Tabs id="fs-tabs" selected={tab} onSelect={setTab} tabs={(["upNext", "related", "lyrics"] as Tab[]).map((k) => ({ key: k, label: t(`fullscreen.${k}` as never), testId: `fs-tab-${k}` }))} />
        <div className="panel" {...tabPanelProps("fs-tabs", tab)}>
          {tab === "upNext" ? <QueuePanel large /> : null}
          {tab === "related" ? <Related trackId={track?.id} providerLabel={providerLabel} /> : null}
          {tab === "lyrics" ? <LyricsPane variant="large" /> : null}
        </div>
      </div>
    </div>
  );
}

function Related({ trackId, providerLabel }: { trackId: string | undefined; providerLabel: (p: string) => string }) {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const { data, loading } = useQuery(() => (trackId ? { type: "related", data: { track_id: trackId, count: 20 } } : null), "related", [trackId], { static: true });
  if (!trackId) return <div className="empty">{t("player.nothingPlaying")}</div>;
  if (!loading && !data?.length) return <div className="empty">{t("fullscreen.relatedEmpty")}</div>;
  return (
    <ul className="plain-list" aria-label={t("fullscreen.related")} data-testid="related-list">
      {(data ?? []).map((r) => (
        <li key={r.track.id} className="related-row" onDoubleClick={() => bridge().dispatch({ type: "playNext", data: { server_id: serverId, track_ids: [r.track.id] } })}>
          <Artwork id={r.track.coverArt} size={64} className="art" />
          <div className="grow" style={{ minWidth: 0 }}>
            <div className="truncate">{r.track.title} <span className="faint">· {r.track.artist}</span></div>
            <div className="why truncate">{providerLabel(r.provider)}{r.score !== undefined ? ` · ${t("related.score", { score: Math.round(r.score * 100) })}` : ""} · {r.reason}</div>
          </div>
          <span className="faint xs">{fmtTime(r.track.durationMs)}</span>
          <button type="button" className="btn icon sm" aria-label={`${t("action.playNext")}: ${r.track.title}`} title={t("action.playNext")} onClick={() => bridge().dispatch({ type: "playNext", data: { server_id: serverId, track_ids: [r.track.id] } })}><Icon name="playNext" size={14} /></button>
        </li>
      ))}
    </ul>
  );
}
