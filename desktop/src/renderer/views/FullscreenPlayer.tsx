// The fullscreen "Now playing" view (F): a blurred, artwork-coloured
// background darkening towards the bottom; "Playing from <source>" and a
// collapse chevron; the big artwork; title and artist • album with love,
// add-to-playlist and more; the wavy seek bar; the transport; playback
// options; and Lyrics / Queue / About toggles.
//
// Choosing Lyrics, Queue or About replaces the artwork in place with that pane
// and the artwork flies (FLIP, a spring) into a thumbnail beside the title;
// choosing it again flies it back. The choice is remembered (store
// `nowPlayingMode`), so the player reopens the way it was left. On open the
// artwork flies in from the player bar's.
//
// A modal dialog: focus moves in on open (the collapse button), Tab stays
// inside, Escape closes, and focus returns to what opened it. On close it
// stays mounted, inert, while it fades (lib/presence.ts) and focus goes back.
//
// Opening it leaves the window as it is; the button beside the collapse
// chevron toggles the window's own fullscreen. If that button put the window
// into fullscreen, the window leaves it again as the close fade starts.
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { t } from "@shared/strings";
import type { Track } from "@core/api";
import { useApp, useSetting, type NowPlayingMode } from "../store/app";
import { useQuery } from "../store/queries";
import { executeAction } from "../store/actions";
import { bridge } from "../core/bridge";
import { FluidBackground } from "../components/FluidBackground";
import { Artwork } from "../components/Artwork";
import { Icon } from "../components/Icon";
import { WavySeek } from "../components/WavySeek";
import { QueuePanel } from "../components/QueuePanel";
import { LyricsPane } from "../components/RightPanel";
import { Stars } from "../components/Stars";
import { Slider } from "../components/controls";
import { openMenuFromButton } from "../components/ContextMenu";
import { fmtBytes, fmtDate, fmtTime } from "../lib/format";
import { trapTab, useReturnFocus } from "../lib/focus";
import { usePresence, type Presence } from "../lib/presence";
import { useMediaQuery, usePrefersReducedMotion } from "../lib/media";
import { drawContinuation, isFlat, loadImage, useArtworkLayout, type Orientation } from "../lib/immersive";
import { useArtwork } from "../components/Artwork";
import { SK } from "@shared/settings-keys";
import type { ArtworkEdge, ImmersiveArtwork } from "@core/api";
import { volumeValueText } from "../lib/a11y";
import { SPRING_EFFECTS, SPRING_FAST, SPRING_SPATIAL } from "../lib/spring";

type Pane = Exclude<NowPlayingMode, "art">;
const PANES: { key: Pane; icon: string }[] = [
  { key: "lyrics", icon: "lyrics" },
  { key: "queue", icon: "queue" },
  { key: "about", icon: "info" },
];
/** The artwork's corner radius as laid out (the stylesheet sizes it per layout). */
const radiusOf = (el: HTMLElement) => Number.parseFloat(getComputedStyle(el).borderTopLeftRadius) || 0;

/** The motion tokens the stylesheet uses, from lib/spring.ts. */
const MOTION = {
  "--spring-spatial": SPRING_SPATIAL.easing,
  "--spring-spatial-ms": `${SPRING_SPATIAL.duration}ms`,
  "--spring-fast": SPRING_FAST.easing,
  "--spring-fast-ms": `${SPRING_FAST.duration}ms`,
  "--spring-effects": SPRING_EFFECTS.easing,
  "--spring-effects-ms": `${SPRING_EFFECTS.duration}ms`,
} as CSSProperties;

/** Animate `el` from where `from` was to where it is now (FLIP), corners included. Returns where it lands. */
function flyFrom(el: HTMLElement, from: DOMRect, fromRadius: number, toRadius: number): DOMRect {
  const to = el.getBoundingClientRect();
  if (!to.width || !from.width) return to;
  const sx = from.width / to.width;
  const sy = from.height / to.height;
  if (Math.abs(from.left - to.left) < 1 && Math.abs(from.top - to.top) < 1 && Math.abs(sx - 1) < 0.01) return to;
  el.animate(
    [
      { transformOrigin: "0 0", transform: `translate(${from.left - to.left}px, ${from.top - to.top}px) scale(${sx}, ${sy})`, borderRadius: `${fromRadius / sx}px / ${fromRadius / sy}px` },
      { transformOrigin: "0 0", transform: "none", borderRadius: `${toRadius}px` },
    ],
    { duration: SPRING_SPATIAL.duration, easing: SPRING_SPATIAL.easing },
  );
  return to;
}

export function FullscreenPlayer() {
  const fullscreen = useApp((s) => s.fullscreen);
  const { value, closing, motion, exitProps } = usePresence<true, HTMLDivElement>(fullscreen || undefined);
  if (!value) return null;
  return <Player closing={closing} motion={motion} exitProps={exitProps} />;
}

type PlayerProps = Pick<Presence<true, HTMLDivElement>, "closing" | "motion" | "exitProps">;

function Player({ closing, motion, exitProps }: PlayerProps) {
  const now = useApp((s) => s.nowPlaying);
  const transport = useApp((s) => s.transport);
  const queue = useApp((s) => s.queue);
  const devices = useApp((s) => s.devices);
  const sleep = useApp((s) => s.sleepTimer);
  const mode = useApp((s) => s.nowPlayingMode);
  const setMode = useApp((s) => s.setNowPlayingMode);
  const setFullscreen = useApp((s) => s.setFullscreen);
  const navigate = useApp((s) => s.navigate);
  const openDialog = useApp((s) => s.openDialog);
  const covered = useApp((s) => !!s.dialog || s.paletteOpen);
  const windowFullscreen = useApp((s) => s.windowState.fullscreen);
  const reducedMotion = usePrefersReducedMotion();
  const closeRef = useRef<HTMLButtonElement>(null);
  const bigArt = useRef<HTMLDivElement>(null);
  const thumbArt = useRef<HTMLButtonElement>(null);
  const controls = useRef<HTMLDivElement>(null);
  /** Where the controls column last rested (the wide layout moves it between modes). */
  const controlsAt = useRef<{ rect: DOMRect; mode: NowPlayingMode } | undefined>(undefined);
  /** Where the visible artwork last came to rest, and in which mode. */
  const flight = useRef<{ rect: DOMRect; radius: number; mode: NowPlayingMode } | undefined>(undefined);
  useReturnFocus(!closing);
  useEffect(() => closeRef.current?.focus({ preventScroll: true }), []);
  const d = bridge().dispatch;
  const track = now?.track;
  const playing = transport.position.isPlaying;
  const owner = devices.find((x) => x.id === transport.lease.owner);
  const remote = owner && !owner.isSelf ? owner.name : undefined;
  const source = queue.contextLabel ?? track?.album;

  /** Whether the full screen button put the window into fullscreen, so closing takes it back out. */
  const madeFullscreen = useRef(false);
  const toggleWindowFullscreen = () => {
    madeFullscreen.current = !windowFullscreen;
    bridge().window.openFullscreen(!windowFullscreen);
  };
  useEffect(() => {
    if (closing) return;
    return () => {
      if (!madeFullscreen.current) return;
      madeFullscreen.current = false;
      if (useApp.getState().windowState.fullscreen) bridge().window.openFullscreen(false);
    };
  }, [closing]);

  const visibleArt = (): HTMLElement | null => (mode === "art" ? bigArt.current : thumbArt.current);
  const choose = (next: NowPlayingMode) => setMode(next);
  // After every commit: when the mode changed (a click, or L/Q from the
  // keyboard), fly the artwork from its last resting place into its new slot;
  // on open, fly it up from the player bar's. Then remember where it rests.
  useLayoutEffect(() => {
    const el = visibleArt();
    const prev = flight.current;
    if (!el) {
      flight.current = undefined;
      return;
    }
    const radius = radiusOf(el);
    if (el.getAnimations().length && prev?.mode === mode) return;
    let from: { rect: DOMRect; radius: number } | undefined;
    if (!prev) {
      const barArt = document.querySelector<HTMLElement>('[data-testid="player-bar"] .now .art');
      if (barArt) from = { rect: barArt.getBoundingClientRect(), radius: 4 };
    } else if (prev.mode !== mode && (prev.mode === "art" || mode === "art")) {
      from = prev;
    }
    const rect = from && !reducedMotion ? flyFrom(el, from.rect, from.radius, radius) : el.getBoundingClientRect();
    flight.current = { rect, radius, mode };
  });
  // The wide layout re-centres the controls column when the mode changes: glide it there too.
  useLayoutEffect(() => {
    const el = controls.current;
    if (!el) return;
    const prev = controlsAt.current;
    if (el.getAnimations().length && prev?.mode === mode) return;
    const to = el.getBoundingClientRect();
    if (prev && prev.mode !== mode && !reducedMotion) {
      const dx = prev.rect.left - to.left;
      const dy = prev.rect.top - to.top;
      if (Math.abs(dx) > 1 || Math.abs(dy) > 1) el.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], { duration: SPRING_SPATIAL.duration, easing: SPRING_SPATIAL.easing });
    }
    controlsAt.current = { rect: to, mode };
  });

  const onKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape" && !e.defaultPrevented) {
      e.preventDefault();
      e.stopPropagation();
      setFullscreen(false);
      return;
    }
    trapTab(e);
  };
  const go = (route: Parameters<typeof navigate>[0]) => {
    navigate(route);
    setFullscreen(false);
  };
  const repeatLabel = queue.repeat === "off" ? t("player.repeatOff") : queue.repeat === "all" ? t("player.repeatAll") : t("player.repeatOne");
  const art = track ? <Artwork id={track.coverArt} size={1000} className="np-art-img" /> : <div className="np-art-img placeholder"><Icon name="music" size={48} /></div>;
  // Immersive artwork: a wide window puts the artwork at its full height on the left,
  // carried on to the right; a tall one at its full width on top, carried on below.
  // In between (and in the other modes) the artwork is a card.
  const preference = useSetting<ImmersiveArtwork>(SK.displayImmersiveArtwork, "automatic");
  const layout = useArtworkLayout(track?.coverArt, preference);
  const wide = useMediaQuery(WIDE_IMMERSIVE);
  const tall = useMediaQuery(TALL_IMMERSIVE);
  const orientation: Orientation | undefined = mode === "art" && layout ? (wide ? "right" : tall ? "bottom" : undefined) : undefined;
  const edge = orientation ? layout?.[orientation] : undefined;
  const immersive = edge && edge.style !== "card" ? { edge, orientation: orientation as Orientation } : undefined;

  return (
    <div {...exitProps} className={`fullscreen np ${motion}`} data-mode={mode} data-immersive={immersive?.orientation} data-light={immersive?.edge.light ? "" : undefined} data-flat={immersive && isFlat(immersive.edge) ? "" : undefined} data-top-light={immersive && layout?.topLight ? "" : undefined} style={MOTION} role="dialog" aria-modal="true" aria-labelledby="fs-title" inert={covered || closing} onKeyDown={onKey} data-testid="fullscreen-player">
      <FluidBackground coverArt={track?.coverArt} />
      {immersive && track ? <ImmersiveContinuation coverArt={track.coverArt} edge={immersive.edge} orientation={immersive.orientation} /> : null}
      <div className="np-fade" aria-hidden="true" />
      <div className="np-stage">
        <header className="np-head">
          <div className="np-source">
            <span className="np-kicker">{t("nowPlaying.playingFrom")}</span>
            <span className="np-source-name" data-testid="np-source">{source ?? t("player.nothingPlaying")}</span>
          </div>
          <button type="button" className="np-icon-btn" aria-pressed={windowFullscreen} aria-label={t("player.windowFullscreen")} title={windowFullscreen ? t("player.exitWindowFullscreen") : t("player.windowFullscreen")} onClick={toggleWindowFullscreen} data-testid="fs-window-fullscreen"><Icon name={windowFullscreen ? "fullscreenExit" : "fullscreen"} size={24} /></button>
          <button ref={closeRef} type="button" className="np-icon-btn" aria-label={t("fullscreen.exit")} title={t("fullscreen.exit")} onClick={() => setFullscreen(false)} data-testid="fullscreen-exit"><Icon name="chevronDown" size={24} /></button>
        </header>

        <div className="np-main">
          {mode === "art" ? (
            <div className="np-art-slot">
              <div ref={bigArt} className="np-art" data-testid="np-artwork">{art}</div>
            </div>
          ) : (
            <section key={mode} className={`np-pane np-pane-${mode}`} aria-label={t(`nowPlaying.view.${mode}`)} data-testid={`np-pane-${mode}`}>
              {mode === "lyrics" ? <LyricsPane variant="large" /> : null}
              {mode === "queue" ? <QueuePanel large /> : null}
              {mode === "about" ? <About trackId={track?.id} onNavigate={go} /> : null}
            </section>
          )}
        </div>

        <div ref={controls} className="np-controls">
        <div className="np-info">
          {mode !== "art" ? (
            <button ref={thumbArt} type="button" className="np-thumb" aria-label={t("nowPlaying.showArtwork")} title={t("nowPlaying.showArtwork")} onClick={() => choose("art")} data-testid="np-thumb">{art}</button>
          ) : null}
          <div className="np-text info">
            <h2 className="t1" id="fs-title" title={track?.title}>{track?.title ?? t("player.nothingPlaying")}</h2>
            <div className="t2">
              {track ? (
                <>
                  {track.artistId ? <button type="button" className="np-link" onClick={() => go({ view: "artist", id: track.artistId })}>{track.artist ?? t("misc.unknownArtist")}</button> : <span>{track.artist ?? t("misc.unknownArtist")}</span>}
                  {track.album ? <><span className="np-dot" aria-hidden="true"> • </span>{track.albumId ? <button type="button" className="np-link" onClick={() => go({ view: "album", id: track.albumId })}>{track.album}</button> : <span>{track.album}</span>}</> : null}
                </>
              ) : null}
            </div>
          </div>
          {track ? (
            <div className="np-actions">
              <button type="button" className={`np-icon-btn heart ${track.loved ? "on" : ""}`} aria-pressed={track.loved} aria-label={t("player.love")} title={track.loved ? t("player.unlove") : t("player.love")} onClick={() => d({ type: "setLoved", data: { targets: [{ type: "track", data: { id: track.id } }], loved: !track.loved } })} data-testid="np-love"><Icon name="heart" size={22} filled={track.loved} /></button>
              <button type="button" className="np-icon-btn" aria-label={t("action.addToPlaylist")} title={t("action.addToPlaylist")} onClick={() => void executeAction("addToPlaylist")} data-testid="np-add"><Icon name="playlistAdd" size={22} /></button>
              <button type="button" className="np-icon-btn" aria-label={t("misc.more")} title={t("misc.more")} aria-haspopup="menu" onClick={(e) => now && openMenuFromButton(e, { type: "queueItems", data: { keys: [now.item.key] } })} data-testid="np-more"><Icon name="moreVert" size={22} /></button>
            </div>
          ) : null}
        </div>

        <WavySeek durationMs={track?.durationMs} playing={playing && !transport.buffering} />

        <div className="np-transport">
          <button type="button" className={`np-tonal np-flank ${queue.shuffle ? "on" : ""}`} aria-pressed={queue.shuffle} aria-label={t("player.shuffle")} title={t("player.shuffle")} onClick={() => d({ type: "setShuffle", data: { enabled: !queue.shuffle } })} data-testid="fs-shuffle"><Icon name="shuffle" size={20} filled={queue.shuffle} /></button>
          <button type="button" className="np-skip" aria-label={t("player.previous")} title={t("player.previous")} onClick={() => d({ type: "previous" })} data-testid="fs-previous"><Icon name="previous" size={38} filled /></button>
          <button type="button" className={`np-play ${playing ? "playing" : "paused"}`} aria-label={playing ? t("player.pause") : t("player.play")} title={playing ? t("player.pause") : t("player.play")} onClick={() => d({ type: "togglePlay" })} data-testid="fs-play-pause">
            {transport.buffering ? <Icon name="spinner" className="spin" size={34} /> : <Icon name={playing ? "pause" : "play"} size={42} filled />}
          </button>
          <button type="button" className="np-skip" aria-label={t("player.next")} title={t("player.next")} onClick={() => d({ type: "next" })} data-testid="fs-next"><Icon name="next" size={38} filled /></button>
          <button type="button" className={`np-tonal np-flank ${queue.repeat !== "off" ? "on" : ""}`} aria-pressed={queue.repeat !== "off"} aria-label={repeatLabel} title={repeatLabel} onClick={() => d({ type: "setRepeat", data: { mode: queue.repeat === "off" ? "all" : queue.repeat === "all" ? "one" : "off" } })} data-testid="fs-repeat"><Icon name={queue.repeat === "one" ? "repeatOne" : "repeat"} size={20} filled={queue.repeat !== "off"} /></button>
        </div>

        <div className="np-row2">
          <div className="np-options" role="toolbar" aria-label={t("nowPlaying.playbackOptions")}>
            <button type="button" className={`np-tonal ${queue.autoplay ? "on" : ""}`} aria-pressed={queue.autoplay} aria-label={t("player.autoplay")} title={t("player.autoplay")} onClick={() => d({ type: "setAutoplay", data: { enabled: !queue.autoplay } })} data-testid="fs-autoplay"><Icon name="autoplay" size={20} filled={queue.autoplay} /></button>
            <button type="button" className={`np-tonal ${remote ? "on" : ""}`} aria-label={remote ? t("player.playingOn", { device: remote }) : t("player.connect")} title={remote ? t("player.playingOn", { device: remote }) : t("player.connect")} onClick={() => void executeAction("ui.playOn")} data-testid="fs-connect"><Icon name="devices" size={20} filled={!!remote} /></button>
            <button type="button" className={`np-tonal ${sleep ? "on" : ""}`} aria-label={sleep ? t("nowPlaying.sleepTimerOn") : t("player.sleepTimer")} title={sleep ? t("nowPlaying.sleepTimerOn") : t("player.sleepTimer")} onClick={() => openDialog({ kind: "sleepTimer" })} data-testid="fs-sleep"><Icon name="sleep" size={20} filled={!!sleep} /></button>
          </div>
          <NpVolume />
        </div>

        <div className="np-modes" role="group" aria-label={t("nowPlaying.views")}>
          {PANES.map((p) => (
            <button key={p.key} type="button" className={`np-mode ${mode === p.key ? "on" : ""}`} aria-pressed={mode === p.key} onClick={() => choose(mode === p.key ? "art" : p.key)} data-testid={`fs-mode-${p.key}`}>
              <Icon name={p.icon} size={18} filled={mode === p.key} />
              <span>{t(`nowPlaying.view.${p.key}`)}</span>
            </button>
          ))}
        </div>
        </div>
      </div>
    </div>
  );
}

/** Room for the controls beside a full-height artwork: at least 3:2. */
const WIDE_IMMERSIVE = "(min-aspect-ratio: 3/2) and (min-height: 480px)";
/** Room for the controls under a full-width artwork: at most 2:3. */
const TALL_IMMERSIVE = "(max-aspect-ratio: 2/3)";

/** The continuation past the immersive artwork's edge, drawn once per cover, layout and window size. */
function ImmersiveContinuation({ coverArt, edge, orientation }: { coverArt: string | undefined; edge: ArtworkEdge; orientation: Orientation }) {
  const url = useArtwork(coverArt, 1000);
  const ref = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  useEffect(() => {
    const el = ref.current;
    if (!el || !size.w || !size.h) return;
    let alive = true;
    const dpr = window.devicePixelRatio || 1;
    void (url ? loadImage(url) : Promise.resolve(undefined)).then((img) => {
      if (!alive) return;
      el.width = Math.round(size.w * dpr);
      el.height = Math.round(size.h * dpr);
      drawContinuation(el, img, edge, orientation);
    });
    return () => {
      alive = false;
    };
  }, [url, edge, orientation, size]);
  return <canvas ref={ref} className="np-continuation" aria-hidden="true" data-testid="np-continuation" data-style={edge.style} />;
}

/** About: the song's details, its rating, and similar tracks. */
function About({ trackId, onNavigate }: { trackId: string | undefined; onNavigate: (route: Parameters<ReturnType<typeof useApp.getState>["navigate"]>[0]) => void }) {
  const { data } = useQuery(() => (trackId ? { type: "track", data: { id: trackId } } : null), "trackDetail", [trackId]);
  const tr = data as Track | undefined;
  const d = bridge().dispatch;
  if (!trackId) return <div className="empty">{t("player.nothingPlaying")}</div>;
  if (!tr) return <div className="empty" role="status">{t("misc.loading")}</div>;
  const format = [
    tr.suffix?.toUpperCase(),
    tr.bitRate ? t("about.bitRate", { kbps: tr.bitRate }) : undefined,
    tr.sampleRate ? t("about.sampleRate", { khz: Math.round(tr.sampleRate / 100) / 10 }) : undefined,
    tr.bitDepth ? t("about.bitDepth", { bits: tr.bitDepth }) : undefined,
    tr.channels ? t("about.channels", { n: tr.channels }) : undefined,
  ].filter(Boolean).join(" · ");
  const link = (label: string | undefined, route: Parameters<typeof onNavigate>[0] | undefined) => label && route ? <button type="button" className="np-link" onClick={() => onNavigate(route)}>{label}</button> : label;
  const rows: [string, React.ReactNode][] = [
    [t("about.album"), link(tr.album, tr.albumId ? { view: "album", id: tr.albumId } : undefined)],
    [t("about.artist"), link(tr.artist, tr.artistId ? { view: "artist", id: tr.artistId } : undefined)],
    [t("about.albumArtist"), tr.albumArtist && tr.albumArtist !== tr.artist ? tr.albumArtist : undefined],
    [t("about.year"), tr.year],
    [t("about.genre"), tr.genre],
    [t("about.track"), tr.trackNumber !== undefined ? t("about.trackOfDisc", { track: tr.trackNumber, disc: tr.discNumber ?? 1 }) : undefined],
    [t("about.duration"), fmtTime(tr.durationMs)],
    [t("about.format"), format || undefined],
    [t("about.playCount"), tr.playCount],
    [t("about.lastPlayed"), tr.lastPlayed ? fmtDate(tr.lastPlayed) : undefined],
    [t("about.added"), tr.created ? fmtDate(tr.created) : undefined],
    [t("about.replayGain"), tr.replayGain?.trackGainDb !== undefined ? `${tr.replayGain.trackGainDb.toFixed(1)} dB` : undefined],
    [t("about.bpm"), tr.sonic?.bpm],
    [t("about.key"), tr.sonic?.key],
    [t("about.size"), tr.sizeBytes ? fmtBytes(tr.sizeBytes) : undefined],
    [t("about.path"), tr.path],
    [t("about.comment"), tr.comment],
  ];
  return (
    <div className="np-about" data-testid="np-about">
      {/* Scrolls when long: focusable so the keyboard can scroll it. */}
      <div className="np-about-scroll" tabIndex={0} role="group" aria-label={t("nowPlaying.details")}>
        <div className="np-card">
          <h3 className="np-card-title">{t("nowPlaying.details")}</h3>
          <dl className="np-facts">
            {rows.filter(([, v]) => v !== undefined && v !== null && v !== "").map(([k, v]) => (
              <div key={k} className={`np-fact ${k === t("about.path") || k === t("about.comment") ? "wide" : ""}`}>
                <dt>{k}</dt>
                <dd className="selectable">{v}</dd>
              </div>
            ))}
            <div className="np-fact">
              <dt>{t("about.rating")}</dt>
              <dd><Stars value={tr.rating} size={18} label={t("a11y.ratingOf", { title: tr.title })} onChange={(r) => d({ type: "setRating", data: { targets: [{ type: "track", data: { id: tr.id } }], rating: r } })} /></dd>
            </div>
          </dl>
          {tr.path ? <button type="button" className="np-chip" onClick={() => bridge().shell.showItemInFolder(tr.path as string)}><Icon name="folder" size={16} />{t("action.showInFolder")}</button> : null}
        </div>
        <Related trackId={tr.id} />
      </div>
    </div>
  );
}

function Related({ trackId }: { trackId: string }) {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const { data, loading } = useQuery(() => ({ type: "related", data: { track_id: trackId, count: 12 } }), "related", [trackId], { static: true });
  const providerLabel = (p: string) => t(`related.provider.${p}` as never);
  const playNext = (id: string) => bridge().dispatch({ type: "playNext", data: { server_id: serverId, track_ids: [id] } });
  return (
    <div className="np-card">
      <h3 className="np-card-title">{t("nowPlaying.similar")}</h3>
      {!loading && !data?.length ? <p className="np-muted">{t("fullscreen.relatedEmpty")}</p> : (
        <ul className="plain-list" aria-label={t("nowPlaying.similar")} data-testid="related-list">
          {(data ?? []).map((r) => (
            <li key={r.track.id} className="related-row" onDoubleClick={() => playNext(r.track.id)}>
              <Artwork id={r.track.coverArt} size={64} className="art" />
              <div className="grow" style={{ minWidth: 0 }}>
                <div className="truncate">{r.track.title} <span className="np-muted">· {r.track.artist}</span></div>
                <div className="why truncate">{providerLabel(r.provider)}{r.score !== undefined ? ` · ${t("related.score", { score: Math.round(r.score * 100) })}` : ""} · {r.reason}</div>
              </div>
              <span className="np-muted xs">{fmtTime(r.track.durationMs)}</span>
              <button type="button" className="np-icon-btn sm" aria-label={`${t("action.playNext")}: ${r.track.title}`} title={t("action.playNext")} onClick={() => playNext(r.track.id)}><Icon name="playNext" size={16} /></button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/**
 * Volume, shown in the wide layout (the player bar is covered): an M3
 * Expressive slider — a thick rounded track, a gap either side of a bar
 * thumb — over a native range input, so keys and assistive tech work as usual.
 */
function NpVolume() {
  const volume = useApp((s) => s.transport.volume);
  const [muted, setMuted] = useState<number | undefined>(undefined);
  const set = (v: number) => bridge().dispatch({ type: "setVolume", data: { volume: v } });
  const toggle = () => {
    if (muted !== undefined) { set(muted); setMuted(undefined); } else { setMuted(volume); set(0); }
  };
  // Arrows move 5 %, Page keys 20 %, like the player bar's.
  const onKey = (e: React.KeyboardEvent) => {
    const step = e.key === "ArrowUp" || e.key === "ArrowRight" ? 0.05 : e.key === "ArrowDown" || e.key === "ArrowLeft" ? -0.05 : e.key === "PageUp" ? 0.2 : e.key === "PageDown" ? -0.2 : 0;
    if (!step || e.ctrlKey || e.metaKey || e.altKey) return;
    e.preventDefault();
    e.stopPropagation();
    setMuted(undefined);
    set(Math.round(Math.max(0, Math.min(1, volume + step)) * 100) / 100);
  };
  return (
    <div className="np-volume">
      <button type="button" className="np-icon-btn sm" aria-label={t("player.mute")} aria-pressed={volume === 0} title={volume === 0 ? t("a11y.unmute") : t("player.mute")} onClick={toggle} data-testid="fs-mute"><Icon name={volume === 0 ? "mute" : "volume"} size={18} /></button>
      <Slider min={0} max={1} step={0.01} value={volume} aria-label={t("player.volume")} aria-valuetext={volumeValueText(volume)} onKeyDown={onKey} onChange={(v) => { setMuted(undefined); set(v); }} onWheel={(e) => set(Math.max(0, Math.min(1, Math.round((volume - Math.sign(e.deltaY) * 0.05) * 100) / 100)))} data-testid="fs-volume" />
    </div>
  );
}
