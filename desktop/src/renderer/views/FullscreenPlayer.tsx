// Fullscreen player (F): Kawarp background, large cover, transport, tabbed
// side panel Up Next / Related / Lyrics.
import { useEffect, useState } from "react";
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

type Tab = "upNext" | "related" | "lyrics";

export function FullscreenPlayer() {
  const now = useApp((s) => s.nowPlaying);
  const transport = useApp((s) => s.transport);
  const queue = useApp((s) => s.queue);
  const setFullscreen = useApp((s) => s.setFullscreen);
  const [tab, setTab] = useState<Tab>("lyrics");
  const d = bridge().dispatch;
  const track = now?.track;
  const playing = transport.position.isPlaying;

  useEffect(() => {
    bridge().window.openFullscreen(true);
    return () => bridge().window.openFullscreen(false);
  }, []);

  const providerLabel = (p: string) => t(`related.provider.${p}` as never);
  return (
    <div className="fullscreen fade-in" role="region" aria-label={t("player.fullscreen")} data-testid="fullscreen-player">
      <FluidBackground coverArt={track?.coverArt} />
      <div className="bg-shade" />
      <button type="button" className="btn icon close" aria-label={t("fullscreen.exit")} onClick={() => setFullscreen(false)} data-testid="fullscreen-exit"><Icon name="close" size={20} /></button>
      <div className="left">
        <Artwork id={track?.coverArt} size={1000} className="cover" />
        <div className="info" style={{ width: "min(52vh, 100%)" }}>
          <div className="t1">{track?.title ?? t("player.nothingPlaying")}</div>
          <div className="t2">{track ? [track.artist, track.album].filter(Boolean).join(" · ") : ""}</div>
        </div>
        <div className="fs-transport">
          <SeekBar durationMs={track?.durationMs} />
          <div className="transport" style={{ justifyContent: "center", gap: 10 }}>
            {track ? <Heart on={track.loved} size={18} onToggle={() => d({ type: "setLoved", data: { targets: [{ type: "track", data: { id: track.id } }], loved: !track.loved } })} /> : null}
            <button type="button" className={`btn icon ${queue.shuffle ? "on" : ""}`} aria-label={t("player.shuffle")} onClick={() => d({ type: "setShuffle", data: { enabled: !queue.shuffle } })}><Icon name="shuffle" size={18} /></button>
            <button type="button" className="btn icon" aria-label={t("player.previous")} onClick={() => d({ type: "previous" })}><Icon name="previous" size={22} style={{ fill: "currentColor" }} /></button>
            <button type="button" className="btn icon play" aria-label={playing ? t("player.pause") : t("player.play")} onClick={() => d({ type: "togglePlay" })} data-testid="fs-play-pause"><Icon name={playing ? "pause" : "play"} size={22} style={{ fill: "currentColor" }} /></button>
            <button type="button" className="btn icon" aria-label={t("player.next")} onClick={() => d({ type: "next" })}><Icon name="next" size={22} style={{ fill: "currentColor" }} /></button>
            <button type="button" className={`btn icon ${queue.repeat !== "off" ? "on" : ""}`} aria-label={t("player.repeat")} onClick={() => d({ type: "setRepeat", data: { mode: queue.repeat === "off" ? "all" : queue.repeat === "all" ? "one" : "off" } })}><Icon name={queue.repeat === "one" ? "repeatOne" : "repeat"} size={18} /></button>
            {track ? <Stars value={track.rating} size={16} onChange={(r) => d({ type: "setRating", data: { targets: [{ type: "track", data: { id: track.id } }], rating: r } })} /> : null}
          </div>
        </div>
      </div>
      <div className="side">
        <div className="tabs" role="tablist">
          {(["upNext", "related", "lyrics"] as Tab[]).map((k) => (
            <button key={k} type="button" role="tab" aria-selected={tab === k} className={`tab ${tab === k ? "active" : ""}`} onClick={() => setTab(k)} data-testid={`fs-tab-${k}`}>{t(`fullscreen.${k}` as never)}</button>
          ))}
        </div>
        <div className="panel">
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
    <div data-testid="related-list">
      {(data ?? []).map((r) => (
        <div key={r.track.id} className="related-row" onDoubleClick={() => bridge().dispatch({ type: "playNext", data: { server_id: serverId, track_ids: [r.track.id] } })}>
          <Artwork id={r.track.coverArt} size={64} className="art" />
          <div className="grow" style={{ minWidth: 0 }}>
            <div className="truncate">{r.track.title} <span className="faint">· {r.track.artist}</span></div>
            <div className="why truncate">{providerLabel(r.provider)}{r.score !== undefined ? ` · ${t("related.score", { score: Math.round(r.score * 100) })}` : ""} · {r.reason}</div>
          </div>
          <span className="faint xs">{fmtTime(r.track.durationMs)}</span>
          <button type="button" className="btn icon sm" aria-label={t("action.playNext")} title={t("action.playNext")} onClick={() => bridge().dispatch({ type: "playNext", data: { server_id: serverId, track_ids: [r.track.id] } })}><Icon name="playNext" size={14} /></button>
        </div>
      ))}
    </div>
  );
}
