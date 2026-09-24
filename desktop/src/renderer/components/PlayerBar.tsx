// Persistent player bar: artwork, title/artist, transport, seek bar with hover
// time, volume, shuffle/repeat/autoplay, rating/love, Connect, panel toggles,
// inline notice line and the dormant resume offer.
import { useCallback, useEffect, useRef, useState } from "react";
import type { RepeatMode } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { usePosition } from "../store/position";
import { bridge } from "../core/bridge";
import { executeAction } from "../store/actions";
import { fmtTime } from "../lib/format";
import { seekKeyTarget, seekValueText, volumeValueText } from "../lib/a11y";
import { openContextMenu } from "./ContextMenu";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";
import { Heart, Stars } from "./Stars";

export function PlayerBar() {
  const now = useApp((s) => s.nowPlaying);
  const transport = useApp((s) => s.transport);
  const queue = useApp((s) => s.queue);
  const notice = useApp((s) => s.playerNotice);
  const resume = useApp((s) => s.resumeOffer);
  const devices = useApp((s) => s.devices);
  const panels = useApp((s) => s.panels);
  const setPanels = useApp((s) => s.setPanels);
  const fullscreen = useApp((s) => s.fullscreen);
  const setFullscreen = useApp((s) => s.setFullscreen);
  const navigate = useApp((s) => s.navigate);
  const sleep = useApp((s) => s.sleepTimer);
  const batterySaver = useApp((s) => s.batterySaver);
  const d = bridge().dispatch;
  const track = now?.track;
  const playing = transport.position.isPlaying;
  const owner = devices.find((x) => x.id === transport.lease.owner);
  const remote = owner && !owner.isSelf ? owner.name : undefined;

  return (
    <div className="player" role="region" aria-label={t("misc.nowPlaying")} data-testid="player-bar">
      {notice ? <div className="notice"><Icon name="warn" size={12} /> {notice}</div> : resume ? (
        <div className="notice resume" data-testid="resume-offer">
          <span>{t("player.resume", { device: resume.deviceName, track: resume.track.title })}</span>
          <button type="button" className="btn primary" onClick={() => d({ type: "resumeHere" })}>{t("player.resumeAction")}</button>
          <button type="button" className="btn icon sm" aria-label={t("player.resumeDismiss")} onClick={() => d({ type: "dismissResumeOffer" })}><Icon name="close" size={12} /></button>
        </div>
      ) : remote ? <div className="notice"><Icon name="devices" size={12} /> {t("player.playingOn", { device: remote })}</div> : null}

      <div className="now" onContextMenu={track ? (e) => void openContextMenu(e, { type: "queueItems", data: { keys: [now.item.key] } }) : undefined}>
        {track ? <Artwork id={track.coverArt} size={64} className="art" /> : <div className="art placeholder"><Icon name="music" /></div>}
        <div className="text">
          {track ? (
            <>
              <div className="title" title={track.title}><a href="#" onClick={(e) => { e.preventDefault(); if (track.albumId) navigate({ view: "album", id: track.albumId }); }}>{track.title}</a></div>
              <div className="artist" title={track.artist}><a href="#" onClick={(e) => { e.preventDefault(); if (track.artistId) navigate({ view: "artist", id: track.artistId }); }}>{track.artist ?? t("misc.unknownArtist")}</a>{track.album ? <span className="faint"> · {track.album}</span> : null}</div>
            </>
          ) : (
            <>
              <div className="title muted">{t("player.nothingPlaying")}</div>
              <div className="artist">{t("player.nothingPlayingHint")}</div>
            </>
          )}
        </div>
        {track ? (
          <div className="row" style={{ gap: 2, flex: "0 0 auto" }}>
            <Heart on={track.loved} onToggle={() => d({ type: "setLoved", data: { targets: [{ type: "track", data: { id: track.id } }], loved: !track.loved } })} />
            <Stars value={track.rating} size={12} label={t("a11y.ratingOf", { title: track.title })} onChange={(r) => d({ type: "setRating", data: { targets: [{ type: "track", data: { id: track.id } }], rating: r } })} />
          </div>
        ) : null}
      </div>

      <div className="center">
        <div className="transport">
          <button type="button" className={`btn icon ${queue.shuffle ? "on" : ""}`} aria-pressed={queue.shuffle} aria-label={t("player.shuffle")} title={t("player.shuffle")} onClick={() => d({ type: "setShuffle", data: { enabled: !queue.shuffle } })} data-testid="shuffle"><Icon name="shuffle" size={15} /></button>
          <button type="button" className="btn icon" aria-label={t("player.previous")} title={t("player.previous")} onClick={() => d({ type: "previous" })} data-testid="previous"><Icon name="previous" size={16} style={{ fill: "currentColor" }} /></button>
          <button type="button" className="btn icon play" aria-label={playing ? t("player.pause") : t("player.play")} title={playing ? t("player.pause") : t("player.play")} onClick={() => d({ type: "togglePlay" })} data-testid="play-pause">{transport.buffering ? <Icon name="spinner" className="spin" size={18} /> : <Icon name={playing ? "pause" : "play"} size={18} style={{ fill: "currentColor" }} />}</button>
          <button type="button" className="btn icon" aria-label={t("player.next")} title={t("player.next")} onClick={() => d({ type: "next" })} data-testid="next"><Icon name="next" size={16} style={{ fill: "currentColor" }} /></button>
          <RepeatButton mode={queue.repeat} />
          <button type="button" className={`btn icon ${queue.autoplay ? "on" : ""}`} aria-pressed={queue.autoplay} aria-label={t("player.autoplay")} title={t("player.autoplay")} onClick={() => d({ type: "setAutoplay", data: { enabled: !queue.autoplay } })}><Icon name="autoplay" size={15} /></button>
        </div>
        <SeekBar durationMs={track?.durationMs} />
      </div>

      <div className="right">
        {sleep ? <span className="badge" title={t("player.sleepTimer")}><Icon name="sleep" size={11} /></span> : null}
        {batterySaver ? <span className="badge" title={t("player.batterySaver")}><Icon name="battery" size={11} /></span> : null}
        <Volume />
        <button type="button" className={`btn icon ${remote ? "on" : ""}`} aria-label={t("player.connect")} title={t("player.connect")} onClick={() => void executeAction("ui.playOn")} data-testid="connect-button"><Icon name="devices" size={15} /></button>
        <button type="button" className={`btn icon ${panels.rightOpen && !panels.queueCollapsed ? "on" : ""}`} aria-pressed={panels.rightOpen && !panels.queueCollapsed} aria-label={t("player.queue")} title={`${t("player.queue")} (Q)`} onClick={() => void executeAction("ui.queue")} data-testid="toggle-queue"><Icon name="queue" size={15} /></button>
        <button type="button" className={`btn icon ${panels.rightOpen && !panels.lyricsCollapsed ? "on" : ""}`} aria-pressed={panels.rightOpen && !panels.lyricsCollapsed} aria-label={t("player.lyrics")} title={`${t("player.lyrics")} (L)`} onClick={() => void executeAction("ui.lyrics")} data-testid="toggle-lyrics"><Icon name="lyrics" size={15} /></button>
        <button type="button" className={`btn icon ${fullscreen ? "on" : ""}`} aria-label={t("player.fullscreen")} title={`${t("player.fullscreen")} (F)`} onClick={() => setFullscreen(!fullscreen)} data-testid="toggle-fullscreen"><Icon name="fullscreen" size={15} /></button>
        <button type="button" className="btn icon" aria-label={t("player.miniPlayer")} title={`${t("player.miniPlayer")} (M)`} onClick={() => bridge().window.openMiniPlayer()}><Icon name="mini" size={15} /></button>
        <button type="button" className="btn icon" aria-label={panels.rightOpen ? t("a11y.hideSidePanel") : t("a11y.showSidePanel")} aria-expanded={panels.rightOpen} title={panels.rightOpen ? t("a11y.hideSidePanel") : t("a11y.showSidePanel")} onClick={() => setPanels({ rightOpen: !panels.rightOpen })} data-testid="toggle-side-panel"><Icon name={panels.rightOpen ? "chevronRight" : "chevronLeft"} size={15} /></button>
      </div>
    </div>
  );
}

function RepeatButton({ mode }: { mode: RepeatMode }) {
  const next: Record<RepeatMode, RepeatMode> = { off: "all", all: "one", one: "off" };
  const label = mode === "off" ? t("player.repeatOff") : mode === "all" ? t("player.repeatAll") : t("player.repeatOne");
  return (
    <button type="button" className={`btn icon ${mode !== "off" ? "on" : ""}`} aria-label={label} title={label} onClick={() => bridge().dispatch({ type: "setRepeat", data: { mode: next[mode] } })} data-testid="repeat">
      <Icon name={mode === "one" ? "repeatOne" : "repeat"} size={15} />
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

function Volume() {
  const volume = useApp((s) => s.transport.volume);
  const [muted, setMuted] = useState<number | undefined>(undefined);
  const set = (v: number) => bridge().dispatch({ type: "setVolume", data: { volume: v } });
  const toggle = () => {
    if (muted !== undefined) { set(muted); setMuted(undefined); } else { setMuted(volume); set(0); }
  };
  // Arrows move 5 %, Page keys 20 %; the native 1 % step stays for the pointer.
  const onKey = (e: React.KeyboardEvent) => {
    const step = e.key === "ArrowUp" || e.key === "ArrowRight" ? 0.05 : e.key === "ArrowDown" || e.key === "ArrowLeft" ? -0.05 : e.key === "PageUp" ? 0.2 : e.key === "PageDown" ? -0.2 : 0;
    if (!step || e.ctrlKey || e.metaKey || e.altKey) return;
    e.preventDefault();
    e.stopPropagation();
    setMuted(undefined);
    set(Math.round(Math.max(0, Math.min(1, volume + step)) * 100) / 100);
  };
  return (
    <div className="volume">
      <button type="button" className="btn icon sm" aria-label={t("player.mute")} aria-pressed={volume === 0} title={volume === 0 ? t("a11y.unmute") : t("player.mute")} onClick={toggle} data-testid="mute"><Icon name={volume === 0 ? "mute" : "volume"} size={14} /></button>
      <input type="range" min={0} max={1} step={0.01} value={volume} aria-label={t("player.volume")} aria-valuetext={volumeValueText(volume)} onKeyDown={onKey} onChange={(e) => { setMuted(undefined); set(Number(e.target.value)); }} onWheel={(e) => set(Math.max(0, Math.min(1, volume - Math.sign(e.deltaY) * 0.05)))} data-testid="volume" />
    </div>
  );
}
