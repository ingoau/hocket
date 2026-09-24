// Always-on-top mini player: artwork thumbnail, title/artist, transport,
// progress. No queue, no lyrics.
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { useTheme } from "../App";
import { Artwork } from "../components/Artwork";
import { Icon } from "../components/Icon";
import { SeekBar } from "../components/PlayerBar";
import { Heart } from "../components/Stars";

export function MiniApp() {
  const now = useApp((s) => s.nowPlaying);
  const transport = useApp((s) => s.transport);
  const win = useApp((s) => s.windowState);
  useTheme();
  const d = bridge().dispatch;
  const track = now?.track;
  const playing = transport.position.isPlaying;
  return (
    <main className="mini" aria-label={t("mini.title")} data-testid="mini-player">
      <div className="art-wrap"><Artwork id={track?.coverArt} size={300} className="art" /></div>
      <div className="body">
        <h1 className="title">{track?.title ?? t("player.nothingPlaying")}</h1>
        <div className="artist">{track?.artist ?? ""}</div>
        <SeekBar durationMs={track?.durationMs} compact />
        <div className="controls">
          <button type="button" className="btn icon sm" aria-label={t("player.previous")} onClick={() => d({ type: "previous" })}><Icon name="previous" size={14} filled /></button>
          <button type="button" className="btn icon sm" aria-label={playing ? t("player.pause") : t("player.play")} onClick={() => d({ type: "togglePlay" })}><Icon name={playing ? "pause" : "play"} size={16} filled /></button>
          <button type="button" className="btn icon sm" aria-label={t("player.next")} onClick={() => d({ type: "next" })}><Icon name="next" size={14} filled /></button>
          {track ? <Heart on={track.loved} size={13} onToggle={() => d({ type: "setLoved", data: { targets: [{ type: "track", data: { id: track.id } }], loved: !track.loved } })} /> : null}
          <span className="spacer" />
          <button type="button" className={`btn icon sm ${win.alwaysOnTop ? "on" : ""}`} aria-pressed={win.alwaysOnTop} aria-label={t("mini.pin")} title={t("mini.pin")} onClick={() => bridge().window.setAlwaysOnTop(!win.alwaysOnTop)}><Icon name="pin" size={13} /></button>
          <button type="button" className="btn icon sm" aria-label={t("mini.back")} title={t("mini.back")} onClick={() => bridge().window.closeMiniPlayer()}><Icon name="fullscreen" size={13} /></button>
          <button type="button" className="btn icon sm" aria-label={t("misc.close")} onClick={() => bridge().window.close()}><Icon name="close" size={13} /></button>
        </div>
      </div>
    </main>
  );
}
