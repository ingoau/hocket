// Persistent player bar (docs/design.md, "Desktop layout"). Left: artwork,
// title and artist, the rating (unless Settings turns it off), add to playlist
// and More (which carries love). Centre: shuffle, previous, the expressive
// play/pause, next, repeat, and the wavy seek bar. Right: the sleep and
// battery badges, volume, then the side panel, lyrics and queue toggles, the
// Connect pill (the device's name only while another device plays), the mini
// player and the fullscreen player. Above it, the inline notice line. (Another
// device's dormant resume offer is not shown: play resumes where it was.)
import { useCallback, useEffect, useRef, useState } from "react";
import type { RepeatMode } from "@core/api";
import { t } from "@shared/strings";
import { SK } from "@shared/settings-keys";
import { toggleSidePanel, useApp, useSetting } from "../store/app";
import { usePosition } from "../store/position";
import { bridge } from "../core/bridge";
import { executeAction } from "../store/actions";
import { fmtTime } from "../lib/format";
import { seekKeyTarget, seekValueText } from "../lib/a11y";
import { useVolumeControl } from "../lib/volume";
import { NARROW, useMediaQuery } from "../lib/media";
import { openContextMenu, openMenuFromButton } from "./ContextMenu";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";
import { isOfflineNotice, noticeText } from "../lib/notice";
import { ArtistChooser } from "./ArtistChooser";
import { useLyricsOptionsTrigger } from "./LyricsOptions";
import { Stars } from "./Stars";
import { Slider } from "./controls";
import { WavySeek } from "./WavySeek";
import { ConnectPill } from "./ConnectPill";

export function PlayerBar({ inert = false }: { inert?: boolean }) {
  const now = useApp((s) => s.nowPlaying);
  const transport = useApp((s) => s.transport);
  const queue = useApp((s) => s.queue);
  // The offline notices get the app-wide banner (App.tsx) instead.
  const notice = useApp((s) => (isOfflineNotice(s.playerNotice) ? undefined : noticeText(s.playerNotice)));
  const panels = useApp((s) => s.panels);
  const drawerOpen = useApp((s) => s.drawerOpen);
  const narrow = useMediaQuery(NARROW);
  const sideOpen = narrow ? drawerOpen : panels.rightOpen;
  const fullscreen = useApp((s) => s.fullscreen);
  const setFullscreen = useApp((s) => s.setFullscreen);
  const navigate = useApp((s) => s.navigate);
  const sleep = useApp((s) => s.sleepTimer);
  const batterySaver = useApp((s) => s.batterySaver);
  const showRating = useSetting(SK.displayPlayerRating, true);
  const d = bridge().dispatch;
  const track = now?.track;
  const playing = transport.position.isPlaying;
  const lyricsOn = sideOpen && panels.rightView === "lyrics";
  // A right-click (or the context-menu key, Shift+F10) on the lyrics toggle opens the lyrics options.
  const lyricsOptions = useLyricsOptionsTrigger("above");
  const queueOn = sideOpen && (panels.rightView === "queue" || panels.rightView === "recent");

  return (
    <div className="player" role="region" aria-label={t("misc.nowPlaying")} inert={inert} data-testid="player-bar">
      {notice ? <div className="notice"><Icon name="warn" size={12} /> {notice}</div> : null}

      <div className="now" onContextMenu={track ? (e) => void openContextMenu(e, { type: "queueItems", data: { keys: [now.item.key] } }) : undefined}>
        {track ? <Artwork id={track.coverArt} size={128} className="art" /> : <div className="art placeholder"><Icon name="music" /></div>}
        <div className="text">
          {track ? (
            <>
              <div className="title" title={track.title}><a href="#" onClick={(e) => { e.preventDefault(); if (track.albumId) navigate({ view: "album", id: track.albumId }); }}>{track.title}</a></div>
              <div className="artist"><ArtistChooser track={track} className="artist-chooser" onNavigate={navigate} testId="bar-artist" /></div>
              {showRating ? <Stars value={track.rating} size={14} label={t("a11y.ratingOf", { title: track.title })} onChange={(r) => d({ type: "setRating", data: { targets: [{ type: "track", data: { id: track.id } }], rating: r } })} /> : null}
            </>
          ) : (
            <>
              <div className="title muted">{t("player.nothingPlaying")}</div>
              <div className="artist">{t("player.nothingPlayingHint")}</div>
            </>
          )}
        </div>
        {track ? (
          <div className="now-actions">
            <button type="button" className="btn icon" aria-label={t("action.addToPlaylist")} title={t("action.addToPlaylist")} onClick={(e) => void executeAction("addToPlaylist", undefined, { anchor: e.currentTarget })} aria-haspopup="menu" data-testid="bar-add"><Icon name="playlistAdd" size={20} /></button>
            <button type="button" className="btn icon" aria-label={t("misc.more")} title={t("misc.more")} aria-haspopup="menu" onClick={(e) => openMenuFromButton(e, { type: "queueItems", data: { keys: [now.item.key] } })} data-testid="bar-more"><Icon name="more" size={20} /></button>
          </div>
        ) : null}
      </div>

      <div className="center">
        <div className="transport">
          <button type="button" className={`btn icon toggle ${queue.shuffle ? "on" : ""}`} aria-pressed={queue.shuffle} aria-label={t("player.shuffle")} title={t("player.shuffle")} onClick={() => d({ type: "setShuffle", data: { enabled: !queue.shuffle } })} data-testid="shuffle"><Icon name="shuffle" size={20} /></button>
          <button type="button" className="btn icon" aria-label={t("player.previous")} title={t("player.previous")} onClick={() => d({ type: "previous" })} data-testid="previous"><Icon name="previous" size={24} filled /></button>
          <button type="button" className={`btn icon play ${playing ? "playing" : "paused"}`} aria-label={playing ? t("player.pause") : t("player.play")} title={playing ? t("player.pause") : t("player.play")} onClick={() => d({ type: "togglePlay" })} data-testid="play-pause">{transport.buffering ? <Icon name="spinner" className="spin" size={22} /> : <Icon name={playing ? "pause" : "play"} size={24} filled />}</button>
          <button type="button" className="btn icon" aria-label={t("player.next")} title={t("player.next")} onClick={() => d({ type: "next" })} data-testid="next"><Icon name="next" size={24} filled /></button>
          <RepeatButton mode={queue.repeat} />
        </div>
        <WavySeek variant="bar" durationMs={track?.durationMs} playing={playing && !transport.buffering} />
      </div>

      <div className="right">
        {sleep ? <span className="badge"><Icon name="sleep" size={11} title={t("player.sleepTimer")} /></span> : null}
        {batterySaver ? <span className="badge"><Icon name="battery" size={11} title={t("player.batterySaver")} /></span> : null}
        <Volume />
        <button type="button" className={`btn icon toggle ${sideOpen ? "on" : ""}`} aria-label={sideOpen ? t("a11y.hideSidePanel") : t("a11y.showSidePanel")} aria-expanded={sideOpen} aria-controls="side-panel" title={sideOpen ? t("a11y.hideSidePanel") : t("a11y.showSidePanel")} onClick={() => toggleSidePanel()} data-testid="toggle-side-panel"><Icon name="sidePanel" size={20} /></button>
        <button type="button" className={`btn icon toggle ${lyricsOn ? "on" : ""}`} aria-pressed={lyricsOn} aria-label={t("player.lyrics")} title={`${t("player.lyrics")} (L)`} onClick={() => void executeAction("ui.lyrics")} {...lyricsOptions.trigger} data-testid="toggle-lyrics"><Icon name="lyrics" size={20} filled={lyricsOn} /></button>
        {lyricsOptions.extra}
        <button type="button" className={`btn icon toggle ${queueOn ? "on" : ""}`} aria-pressed={queueOn} aria-label={t("player.queue")} title={`${t("player.queue")} (Q)`} onClick={() => void executeAction("ui.queue")} data-testid="toggle-queue"><Icon name="queue" size={20} filled={queueOn} /></button>
        <ConnectPill testId="connect-button" />
        <button type="button" className="btn icon" aria-label={t("player.miniPlayer")} title={`${t("player.miniPlayer")} (M)`} onClick={() => bridge().window.openMiniPlayer()} data-testid="toggle-mini"><Icon name="mini" size={20} /></button>
        <button type="button" className={`btn icon ${fullscreen ? "on" : ""}`} aria-label={t("player.fullscreen")} title={`${t("player.fullscreen")} (F)`} onClick={() => setFullscreen(!fullscreen)} data-testid="toggle-fullscreen"><Icon name="openInFull" size={20} /></button>
      </div>
    </div>
  );
}

function RepeatButton({ mode }: { mode: RepeatMode }) {
  const next: Record<RepeatMode, RepeatMode> = { off: "all", all: "one", one: "off" };
  const label = mode === "off" ? t("player.repeatOff") : mode === "all" ? t("player.repeatAll") : t("player.repeatOne");
  return (
    <button type="button" className={`btn icon toggle ${mode !== "off" ? "on" : ""}`} aria-pressed={mode !== "off"} aria-label={label} title={label} onClick={() => bridge().dispatch({ type: "setRepeat", data: { mode: next[mode] } })} data-testid="repeat">
      <Icon name={mode === "one" ? "repeatOne" : "repeat"} size={20} />
    </button>
  );
}

export function SeekBar({ durationMs, compact = false }: { durationMs: number | undefined; compact?: boolean }) {
  const stamp = useApp((s) => s.transport.position);
  const offset = useApp((s) => s.connection.clockOffsetMs);
  const pos = usePosition(stamp, durationMs, offset);
  const [hover, setHover] = useState<number | undefined>(undefined);
  const [drag, setDrag] = useState<number | undefined>(undefined);
  const bar = useRef<HTMLDivElement>(null);
  const dur = durationMs ?? 0;
  const frac = (x: number) => {
    const r = bar.current?.getBoundingClientRect();
    if (!r || !r.width) return 0;
    return Math.max(0, Math.min(1, (x - r.left) / r.width));
  };
  const commit = useCallback((f: number) => bridge().dispatch({ type: "seekTo", data: { position_ms: Math.round(f * dur) } }), [dur]);
  useEffect(() => {
    if (drag === undefined) return;
    const move = (e: MouseEvent) => setDrag(frac(e.clientX));
    const up = (e: MouseEvent) => { commit(frac(e.clientX)); setDrag(undefined); };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    return () => { window.removeEventListener("mousemove", move); window.removeEventListener("mouseup", up); };
  }, [drag !== undefined, commit]);
  const shown = drag !== undefined ? drag * dur : pos;
  const pct = dur ? (shown / dur) * 100 : 0;
  // Whole seconds only, so a focused slider is not re-announced every frame.
  const seconds = Math.floor(shown / 1000);
  const onKey = (e: React.KeyboardEvent) => {
    if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
    const to = seekKeyTarget(e.key, shown, dur);
    if (to === undefined) return;
    e.preventDefault();
    e.stopPropagation();
    bridge().dispatch({ type: "seekTo", data: { position_ms: Math.round(to) } });
  };
  return (
    <div className={`seek ${compact ? "compact" : ""}`} data-testid="seek">
      <span data-testid="seek-position" aria-hidden="true">{fmtTime(shown)}</span>
      <div ref={bar} className={`bar ${drag !== undefined ? "dragging" : ""}`} role="slider" aria-label={t("player.seek")} aria-valuemin={0} aria-valuemax={Math.floor(dur / 1000)} aria-valuenow={seconds} aria-valuetext={seekValueText(seconds * 1000, dur)} aria-disabled={!dur || undefined} tabIndex={dur ? 0 : -1}
        onKeyDown={onKey}
        data-testid="seek-slider"
        onMouseDown={(e) => { if (!dur) return; e.preventDefault(); (e.currentTarget as HTMLElement).focus({ preventScroll: true }); setDrag(frac(e.clientX)); }}
        onMouseMove={(e) => setHover(frac(e.clientX))}
        onMouseLeave={() => setHover(undefined)}>
        <div className="track">
          <div className="fill" style={{ width: `${pct}%` }} />
          <div className="knob" style={{ left: `${pct}%` }} />
        </div>
        {hover !== undefined && dur ? <div className="hover-time" style={{ left: `${hover * 100}%` }}>{fmtTime(hover * dur)}</div> : null}
      </div>
      <span aria-hidden="true">{fmtTime(dur)}</span>
    </div>
  );
}

/** The volume of the device playing (this one, or the one holding transport). */
function Volume() {
  const v = useVolumeControl();
  const silent = v.muted || v.shown === 0;
  return (
    <div className="volume" title={v.label}>
      <button type="button" className="btn icon" aria-label={t("player.mute")} aria-pressed={v.muted} title={v.muted ? t("a11y.unmute") : t("player.mute")} disabled={v.unknown} onClick={v.toggleMute} data-testid="mute"><Icon name={silent ? "mute" : "volume"} size={20} /></button>
      <Slider size="sm" min={0} max={1} step={0.01} value={v.shown} disabled={v.unknown} aria-label={v.label} aria-valuetext={v.valueText} onKeyDown={v.onKeyDown} onChange={v.set} onWheel={v.onWheel} data-testid="volume" />
    </div>
  );
}
