// The fullscreen "Now playing" view (F), docs/design.md "Desktop layout".
//
// On the blurred, artwork-coloured background (FluidBackground): the artwork,
// a controls card (title, artist, love / add to playlist / More, the
// rating, the wavy seek bar and the transport), the collapse and window
// fullscreen buttons top right, and a pill bottom right with the Connect
// device and the Lyrics / Queue / About toggles (choosing the one that is on
// goes back to the artwork alone), then the sleep timer and volume.
//
// Three layouts:
// - "columns" (wide windows, card covers): the rounded artwork with the card
//   under it, centred; with Lyrics, Queue or About the column moves left
//   (FLIP, a spring) and the view takes the right.
// - "immersive" (3:2 or wider, a cover lib/immersive.ts carries on): the
//   artwork full height, flush to the top and bottom, carried on past its
//   edges; the controls card floats over its bottom like glass. Centred and
//   carried on both ways when both its sides suit it, else on the left and
//   carried on to the right. With the artwork alone, the controls fade while
//   the mouse rests during playback (never while paused, focused or with a
//   menu open; a setting turns it off). The other views put the artwork on
//   the left (centred artwork slides there, its card with it, the left
//   continuation going with it off the edge), shorten the carried-on part,
//   which dissolves into the moving blur, and sit on that.
// - "stacked" (narrow or tall windows): the mobile layout, artwork (or the
//   view, with a thumbnail by the title) over the controls; a tall window's
//   immersive cover runs full width on top, carried on below.
//
// The view is remembered (store `nowPlayingMode`, the queue the first time).
// On open the artwork flies in from the player bar's. A modal dialog: focus
// moves in on open (the collapse button), Tab stays inside, Escape closes,
// and focus returns to what opened it. On close it stays mounted, inert,
// while it fades (lib/presence.ts). The window fullscreen button beside the
// collapse button toggles the window's own fullscreen; if it put the window
// into fullscreen, the window leaves it again as the close fade starts.
import { Fragment, useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type RefObject } from "react";
import { t } from "@shared/strings";
import { useApp, useSetting, type NowPlayingMode } from "../store/app";
import { executeAction } from "../store/actions";
import { bridge } from "../core/bridge";
import { FluidBackground } from "../components/FluidBackground";
import { Artwork, useArtwork } from "../components/Artwork";
import { Icon } from "../components/Icon";
import { WavySeek } from "../components/WavySeek";
import { QueuePanel } from "../components/QueuePanel";
import { LyricsPane } from "../components/NowPlayingSidebar";
import { AboutSong } from "../components/AboutSong";
import { ConnectPill } from "../components/ConnectPill";
import { ArtistChooser } from "../components/ArtistChooser";
import { useLyricsOptionsTrigger } from "../components/LyricsOptions";
import { Stars } from "../components/Stars";
import { Slider } from "../components/controls";
import { openMenuFromButton } from "../components/ContextMenu";
import { trapTab, useReturnFocus } from "../lib/focus";
import { usePresence, type Presence } from "../lib/presence";
import { useMediaQuery, usePrefersReducedMotion } from "../lib/media";
import { drawContinuation, isFlat, prepareScene, useScene, withTone, type Orientation, type Scene } from "../lib/immersive";
import { IDLE_HIDE_MS, useIdleHide } from "../lib/idle-hide";
import { extractBrightest } from "../lib/accent";
import { scrimAlpha } from "../lib/contrast";
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

/** The player's popovers, portalled into it, that keep its controls up while open. */
const POPOVERS = ":is(.artist-menu, .lyrics-options-pop, .np-volume-pop):not(.closing)";
/** The latest a new cover's scene has faded in over the last (np-scene-in takes 500ms). */
const SCENE_FADE_MS = 2000;
/** Room for the controls beside a full-height artwork: at least 3:2. */
const WIDE_IMMERSIVE = "(min-aspect-ratio: 3/2) and (min-height: 480px)";
/** Room for the controls under a full-width artwork: at most 2:3. */
const TALL_IMMERSIVE = "(max-aspect-ratio: 2/3)";
/** Room for the artwork column beside a view; anything narrower, shorter or taller than wide stacks. */
const COLUMNS = "(min-width: 1000px) and (min-height: 600px) and (min-aspect-ratio: 1/1)";

export type FullscreenLayout = "columns" | "immersive" | "stacked";

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
  const sleep = useApp((s) => s.sleepTimer);
  const mode = useApp((s) => s.nowPlayingMode);
  const setMode = useApp((s) => s.setNowPlayingMode);
  const setFullscreen = useApp((s) => s.setFullscreen);
  const navigate = useApp((s) => s.navigate);
  const openDialog = useApp((s) => s.openDialog);
  const covered = useApp((s) => !!s.dialog || s.paletteOpen);
  const menuOpen = useApp((s) => !!s.contextMenu);
  const windowFullscreen = useApp((s) => s.windowState.fullscreen);
  const showRating = useSetting(SK.displayPlayerRating, true);
  const reducedMotion = usePrefersReducedMotion();
  const closeRef = useRef<HTMLButtonElement>(null);
  const bigArt = useRef<HTMLDivElement>(null);
  const thumbArt = useRef<HTMLButtonElement>(null);
  const controls = useRef<HTMLDivElement>(null);
  const pill = useRef<HTMLDivElement>(null);
  /** Where the controls last rested (the columns layout moves them between modes). */
  const controlsAt = useRef<{ rect: DOMRect; mode: NowPlayingMode } | undefined>(undefined);
  /** Where the visible artwork last came to rest, and in which mode and layout. */
  const flight = useRef<{ rect: DOMRect; radius: number; mode: NowPlayingMode; shape: FullscreenLayout } | undefined>(undefined);
  useReturnFocus(!closing);
  useEffect(() => closeRef.current?.focus({ preventScroll: true }), []);
  const d = bridge().dispatch;
  const track = now?.track;
  const playing = transport.position.isPlaying;

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

  // Immersive artwork: a wide window puts the artwork at its full height, carried on to the
  // right (in every view) and, centred with the artwork alone, to the left too when that side
  // suits it; a tall one at its full width on top, carried on below (with the artwork alone).
  // Otherwise the artwork is a card.
  const preference = useSetting<ImmersiveArtwork>(SK.displayImmersiveArtwork, "automatic");
  const themeSetting = useSetting<"system" | "light" | "dark">(SK.displayTheme, "system");
  const systemDark = useMediaQuery("(prefers-color-scheme: dark)");
  const dark = themeSetting === "dark" || (themeSetting === "system" && systemDark);
  // The artwork, its layout and its continuation switch together, once the new cover's scene is
  // ready (the next track's is prepared ahead), and the new scene crossfades in over the last.
  const scene = useScene(track?.coverArt, preference, dark);
  const next = (queue.playingNext[0] ?? queue.upcoming[0])?.track.coverArt;
  useEffect(() => {
    if (next) void prepareScene(next, preference, dark);
  }, [next, preference, dark]);
  const layout = scene?.layout;
  const wide = useMediaQuery(WIDE_IMMERSIVE);
  const tall = useMediaQuery(TALL_IMMERSIVE);
  const columns = useMediaQuery(COLUMNS);
  const orientation: Orientation | undefined = wide ? "right" : tall && mode === "art" ? "bottom" : undefined;
  const immersiveOf = (s: Scene | undefined): { edge: ArtworkEdge; orientation: Orientation; left?: ArtworkEdge } | undefined => {
    const edge = s && orientation ? s.layout?.[orientation] : undefined;
    if (!edge || edge.style === "card") return undefined;
    // Layouts from an older core have no left edge: a card there.
    const left = orientation === "right" ? s?.layout?.left : undefined;
    if (!left || left.style === "card") return { edge, orientation: orientation as Orientation };
    // Centred, carried on both ways: one tone for the controls over both sides (the two
    // decisions where they agree, the app's theme where they do not, with the scrim it needs).
    const light = left.light === edge.light ? edge.light : !dark;
    return { edge: withTone(edge, light), orientation: orientation as Orientation, left: withTone(left, light) };
  };
  const immersive = immersiveOf(scene);
  const shape: FullscreenLayout = immersive?.orientation === "right" ? "immersive" : columns ? "columns" : "stacked";
  /** The artwork centred, carried on both ways (both sides suit it, with the artwork alone). */
  const centred = !!immersive?.left && mode === "art";
  /** Dark controls on light glass over a light continuation; they keep it when a view opens. */
  const lightGlass = !!immersive?.edge.light;
  /** The scenes on screen: the current one last, over the one it is fading in from. */
  const [shown, setShown] = useState<Scene[]>([]);
  if (scene ? shown.at(-1) !== scene : shown.length > 0) setShown(scene ? [...shown.slice(-1), scene] : []);
  // The one beneath goes when the new one has faded in (or, should no fade run, a little after).
  const settle = () => setShown((s) => s.slice(-1));
  useEffect(() => {
    if (shown.length < 2) return;
    const id = window.setTimeout(settle, SCENE_FADE_MS);
    return () => window.clearTimeout(id);
  }, [shown]);
  // The one before stays beneath only when it is laid out the same way (both immersive, or both a
  // card); otherwise the new scene fades in from the background on its own.
  const fading = shown.length > 1;
  const layers = shown.filter((s, i) => i === shown.length - 1 || !immersiveOf(s) === !immersive);
  const bigArtContent = layers.length ? (
    layers.map((s, i) => (s.url ? <img key={s.coverArt} className={`np-art-img np-scene${fading && i === layers.length - 1 ? " np-scene-in" : ""}`} src={s.url} alt="" draggable={false} onAnimationEnd={settle} /> : <div key={s.coverArt} className="np-art-img np-scene placeholder"><Icon name="music" size={48} /></div>))
  ) : (
    <div className="np-art-img placeholder"><Icon name="music" size={48} /></div>
  );
  const smallArt = track ? <Artwork id={track.coverArt} size={1000} className="np-art-img" /> : <div className="np-art-img placeholder"><Icon name="music" size={48} /></div>;
  /** With a view open, the stacked layout shows the artwork as a thumbnail by the title; the others keep it in its slot. */
  const thumbByTitle = shape === "stacked" && mode !== "art";

  const visibleArt = (): HTMLElement | null => (mode === "art" ? bigArt.current : thumbArt.current);
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
      if (barArt) from = { rect: barArt.getBoundingClientRect(), radius: radiusOf(barArt) };
    } else if (prev.mode !== mode && !(prev.shape === "immersive" && shape === "immersive")) {
      // (The immersive layout moves its artwork with a transition of its own, below.)
      from = prev;
    }
    const rect = from && !reducedMotion ? flyFrom(el, from.rect, from.radius, radius) : el.getBoundingClientRect();
    flight.current = { rect, radius, mode, shape };
  });
  // The columns layout moves the controls with the artwork when the mode changes: glide them there too.
  useLayoutEffect(() => {
    const el = controls.current;
    if (!el) return;
    const prev = controlsAt.current;
    if (el.getAnimations().length && prev?.mode === mode) return;
    const to = el.getBoundingClientRect();
    if (prev && prev.mode !== mode && !reducedMotion && shape !== "immersive") {
      const dx = prev.rect.left - to.left;
      const dy = prev.rect.top - to.top;
      if (Math.abs(dx) > 1 || Math.abs(dy) > 1) el.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], { duration: SPRING_SPATIAL.duration, easing: SPRING_SPATIAL.easing });
    }
    controlsAt.current = { rect: to, mode };
  });
  // The immersive layout slides its artwork, card and continuations between centred and left
  // (a CSS transition on --np-art-x); with reduced motion they fade in where they land instead.
  const wasCentred = useRef(centred);
  useLayoutEffect(() => {
    if (wasCentred.current === centred) return;
    wasCentred.current = centred;
    if (!reducedMotion || shape !== "immersive") return;
    for (const el of exitProps.ref.current?.querySelectorAll<HTMLElement>(".np-art-slot, .np-controls, .np-continuation") ?? []) {
      el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 250, easing: "ease-out" });
    }
  });

  // The glass under the controls: dark enough that white text reaches 4.5:1 over the cover's brightest colour.
  const coverUrl = useArtwork(track?.coverArt, 300);
  const [glass, setGlass] = useState(0.5);
  useEffect(() => {
    if (!coverUrl) return;
    let alive = true;
    void extractBrightest(coverUrl).then((c) => alive && c && setGlass(Math.min(0.72, scrimAlpha(c, 0.8, 4.6, 0.32))));
    return () => { alive = false; };
  }, [coverUrl]);

  // Idle: over the immersive artwork alone, while it plays, the controls fade when the mouse rests.
  const idleHide = useIdleHide();
  const [volumeOpen, setVolumeOpen] = useState(false);
  const canIdle = idleHide && shape === "immersive" && mode === "art" && playing && !menuOpen && !volumeOpen && !covered && !closing;
  const [idle, setIdle] = useState(false);
  const root = exitProps.ref;
  useEffect(() => {
    if (!canIdle) {
      setIdle(false);
      return;
    }
    let timer = 0;
    /** Whether the keyboard, not the mouse, was used last: then focus in the controls keeps them up. */
    let keyboard = false;
    const arm = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        if (keyboard && root.current?.querySelector(".np-chrome :focus")) return arm();
        // A popover of the player's own (the artist menu, the lyrics options, volume) keeps the
        // controls up while it is open, whatever has focus.
        if (root.current?.querySelector(POPOVERS)) return arm();
        setIdle(true);
      }, IDLE_HIDE_MS);
    };
    const wake = (e: Event) => {
      keyboard = e.type === "keydown";
      setIdle(false);
      arm();
    };
    arm();
    const events = ["mousemove", "pointerdown", "keydown", "wheel"] as const;
    for (const e of events) window.addEventListener(e, wake, { passive: true });
    return () => {
      window.clearTimeout(timer);
      for (const e of events) window.removeEventListener(e, wake);
    };
  }, [canIdle, root]);

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
  // A right-click (or the context-menu key, Shift+F10) on the pill's lyrics toggle opens the lyrics
  // options above it: nothing is added to the pill, so its buttons never move between views.
  const lyricsOptions = useLyricsOptionsTrigger("above");
  const repeatLabel = queue.repeat === "off" ? t("player.repeatOff") : queue.repeat === "all" ? t("player.repeatAll") : t("player.repeatOne");

  return (
    <div {...exitProps} className={`fullscreen np ${motion}`} data-mode={mode} data-layout={shape} data-immersive={immersive?.orientation} data-light={immersive?.edge.light && (mode === "art" || immersive.orientation === "bottom") ? "" : undefined} data-flat={immersive && isFlat(immersive.edge) ? "" : undefined} data-sides={immersive?.left ? "both" : undefined} data-centred={centred ? "" : undefined} data-left-flat={immersive?.left && isFlat(immersive.left) ? "" : undefined} data-top-light={immersive && layout?.topLight ? "" : undefined} data-idle={idle ? "" : undefined}
      style={{ ...MOTION, "--np-glass": glass } as CSSProperties} role="dialog" aria-modal="true" aria-labelledby="fs-title" inert={covered || closing} onKeyDown={onKey} data-testid="fullscreen-player">
      <FluidBackground coverArt={scene?.coverArt ?? track?.coverArt} />
      {layers.map((s) => {
        const im = immersiveOf(s);
        return im ? (
          <Fragment key={s.coverArt}>
            <ImmersiveContinuation img={s.img} edge={im.edge} orientation={im.orientation} controls={im.orientation === "right" ? pill : controls} split={!!im.left} />
            {im.left ? <ImmersiveContinuation img={s.img} edge={im.left} orientation="left" controls={pill} split /> : null}
          </Fragment>
        ) : null;
      })}
      <div className="np-fade" aria-hidden="true" />

      <header className="np-head np-chrome">
        <button type="button" className="np-icon-btn np-round" aria-pressed={windowFullscreen} aria-label={t("player.windowFullscreen")} title={windowFullscreen ? t("player.exitWindowFullscreen") : t("player.windowFullscreen")} onClick={toggleWindowFullscreen} data-testid="fs-window-fullscreen"><Icon name={windowFullscreen ? "fullscreenExit" : "fullscreen"} size={22} /></button>
        <button ref={closeRef} type="button" className="np-icon-btn np-round" aria-label={t("fullscreen.exit")} title={t("fullscreen.exit")} onClick={() => setFullscreen(false)} data-testid="fullscreen-exit"><Icon name="closeFullscreen" size={22} /></button>
      </header>

      <div className="np-stage">
        <div className="np-col">
          {mode === "art" ? (
            <div className="np-art-slot">
              <div ref={bigArt} className="np-art" data-testid="np-artwork" data-cover={layers.length === 1 ? scene?.coverArt : undefined}>{bigArtContent}</div>
            </div>
          ) : !thumbByTitle ? (
            <div className="np-art-slot">
              <button ref={thumbArt} type="button" className="np-art np-art-button" aria-label={t("nowPlaying.showArtwork")} title={t("nowPlaying.showArtwork")} onClick={() => setMode("art")} data-cover={layers.length === 1 ? scene?.coverArt : undefined} data-testid="np-thumb">{bigArtContent}</button>
            </div>
          ) : null}

          <div ref={controls} className={`np-controls np-chrome ${lightGlass ? "light-glass" : ""}`} role="group" aria-label={t("nowPlaying.controls")} data-testid="fs-controls">
            <div className="np-info">
              {thumbByTitle ? <button ref={thumbArt} type="button" className="np-thumb" aria-label={t("nowPlaying.showArtwork")} title={t("nowPlaying.showArtwork")} onClick={() => setMode("art")} data-testid="np-thumb">{smallArt}</button> : null}
              <div className="np-text info">
                <h2 className="t1" id="fs-title" title={track?.title}>{track?.title ?? t("player.nothingPlaying")}</h2>
                <div className="t2">{track ? <ArtistChooser track={track} className="artist-chooser" onNavigate={go} testId="fs-artist" /> : null}</div>
              </div>
              {track ? (
                <div className="np-actions">
                  <button type="button" className={`np-icon-btn np-round heart ${track.loved ? "on" : ""}`} aria-pressed={track.loved} aria-label={t("player.love")} title={track.loved ? t("player.unlove") : t("player.love")} onClick={() => d({ type: "setLoved", data: { targets: [{ type: "track", data: { id: track.id } }], loved: !track.loved } })} data-testid="np-love"><Icon name="heart" size={20} filled={track.loved} /></button>
                  <button type="button" className="np-icon-btn np-round" aria-label={t("action.addToPlaylist")} title={t("action.addToPlaylist")} onClick={() => void executeAction("addToPlaylist")} data-testid="np-add"><Icon name="playlistAdd" size={20} /></button>
                  <button type="button" className="np-icon-btn np-round" aria-label={t("misc.more")} title={t("misc.more")} aria-haspopup="menu" onClick={(e) => now && openMenuFromButton(e, { type: "queueItems", data: { keys: [now.item.key] } })} data-testid="np-more"><Icon name="more" size={20} /></button>
                </div>
              ) : null}
            </div>
            {track && showRating ? <div className="np-stars" data-testid="np-stars"><Stars value={track.rating} size={20} label={t("a11y.ratingOf", { title: track.title })} onChange={(r) => d({ type: "setRating", data: { targets: [{ type: "track", data: { id: track.id } }], rating: r } })} /></div> : null}

            <WavySeek durationMs={track?.durationMs} playing={playing && !transport.buffering} />

            <div className="np-transport">
              <button type="button" className={`np-tonal np-flank ${queue.shuffle ? "on" : ""}`} aria-pressed={queue.shuffle} aria-label={t("player.shuffle")} title={t("player.shuffle")} onClick={() => d({ type: "setShuffle", data: { enabled: !queue.shuffle } })} data-testid="fs-shuffle"><Icon name="shuffle" size={22} filled={queue.shuffle} /></button>
              <button type="button" className="np-skip" aria-label={t("player.previous")} title={t("player.previous")} onClick={() => d({ type: "previous" })} data-testid="fs-previous"><Icon name="previous" size={36} filled /></button>
              <button type="button" className={`np-play ${playing ? "playing" : "paused"}`} aria-label={playing ? t("player.pause") : t("player.play")} title={playing ? t("player.pause") : t("player.play")} onClick={() => d({ type: "togglePlay" })} data-testid="fs-play-pause">
                {transport.buffering ? <Icon name="spinner" className="spin" size={34} /> : <Icon name={playing ? "pause" : "play"} size={40} filled />}
              </button>
              <button type="button" className="np-skip" aria-label={t("player.next")} title={t("player.next")} onClick={() => d({ type: "next" })} data-testid="fs-next"><Icon name="next" size={36} filled /></button>
              <button type="button" className={`np-tonal np-flank ${queue.repeat !== "off" ? "on" : ""}`} aria-pressed={queue.repeat !== "off"} aria-label={repeatLabel} title={repeatLabel} onClick={() => d({ type: "setRepeat", data: { mode: queue.repeat === "off" ? "all" : queue.repeat === "all" ? "one" : "off" } })} data-testid="fs-repeat"><Icon name={queue.repeat === "one" ? "repeatOne" : "repeat"} size={22} filled={queue.repeat !== "off"} /></button>
            </div>
          </div>
        </div>

        {mode !== "art" ? (
          <section key={mode} className={`np-pane np-pane-${mode}`} aria-label={t(`nowPlaying.view.${mode}`)} data-testid={`np-pane-${mode}`}>
            {mode === "lyrics" ? <LyricsPane variant="large" /> : null}
            {mode === "queue" ? <QueuePanel large /> : null}
            {mode === "about" ? (
              // Scrolls when long: focusable so the keyboard can scroll it.
              <div className="np-about-scroll" tabIndex={0} role="group" aria-label={t("nowPlaying.details")}>
                <AboutSong variant="fullscreen" trackId={track?.id} onNavigate={go} />
              </div>
            ) : null}
          </section>
        ) : null}
      </div>

      <div ref={pill} className={`np-pill np-chrome ${lightGlass ? "light-glass" : ""}`} role="toolbar" aria-label={t("nowPlaying.viewsAndDevice")} data-testid="fs-pill">
        <ConnectPill testId="fs-connect" dropdown iconSize={20} />
        <span className="np-pill-divider" aria-hidden="true" />
        <div className="np-modes" role="group" aria-label={t("nowPlaying.views")}>
          {PANES.map((p) => (
            <button key={p.key} type="button" className={`np-mode ${mode === p.key ? "on" : ""}`} aria-pressed={mode === p.key} aria-label={t(`nowPlaying.view.${p.key}`)} title={t(`nowPlaying.view.${p.key}`)} onClick={() => setMode(mode === p.key ? "art" : p.key)} {...(p.key === "lyrics" ? lyricsOptions.trigger : {})} data-testid={`fs-mode-${p.key}`}>
              <Icon name={p.icon} size={22} filled={mode === p.key} />
            </button>
          ))}
        </div>
        <span className="np-pill-divider" aria-hidden="true" />
        <button type="button" className={`np-mode ${sleep ? "on" : ""}`} aria-pressed={!!sleep} aria-label={sleep ? t("nowPlaying.sleepTimerOn") : t("player.sleepTimer")} title={sleep ? t("nowPlaying.sleepTimerOn") : t("player.sleepTimer")} onClick={() => openDialog({ kind: "sleepTimer" })} data-testid="fs-sleep"><Icon name="sleep" size={22} filled={!!sleep} /></button>
        <NpVolume open={volumeOpen} setOpen={setVolumeOpen} />
      </div>
      {lyricsOptions.extra}
    </div>
  );
}

/**
 * The continuation past the immersive artwork's edge, drawn once per cover, layout and window size
 * (and wherever what sits on it, whose fade covers it, ends up: the pill beside a full-height
 * artwork, the controls under a full-width one).
 */
function ImmersiveContinuation({ img, edge, orientation, controls, split = false }: { img: HTMLImageElement | undefined; edge: ArtworkEdge; orientation: Orientation; controls: RefObject<HTMLDivElement | null>; split?: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ w: 0, h: 0, top: 0, bottom: 0 });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => {
      const box = el.getBoundingClientRect();
      const c = controls.current?.getBoundingClientRect();
      setSize({ w: el.clientWidth, h: el.clientHeight, top: c ? c.top - box.top : 0, bottom: c ? c.bottom - box.top : 0 });
    };
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    if (controls.current) ro.observe(controls.current);
    return () => ro.disconnect();
  }, [controls]);
  // Before paint: the scene's artwork is already decoded, so the continuation lands with it.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || !size.w || !size.h) return;
    const dpr = window.devicePixelRatio || 1;
    el.width = Math.round(size.w * dpr);
    el.height = Math.round(size.h * dpr);
    drawContinuation(el, img, edge, orientation, size.bottom > size.top ? { top: size.top * dpr, bottom: size.bottom * dpr } : undefined, { split });
  }, [img, edge, orientation, size, split]);
  return <canvas ref={ref} className="np-continuation" aria-hidden="true" data-testid={orientation === "left" ? "np-continuation-left" : "np-continuation"} data-side={orientation} data-style={edge.style} />;
}

/**
 * Volume in the pill: a button that opens a popover with mute and an M3
 * Expressive slider (a thick rounded track, a gap either side of a bar thumb)
 * over a native range input, so keys and assistive tech work as usual.
 */
function NpVolume({ open, setOpen }: { open: boolean; setOpen: (open: boolean) => void }) {
  const volume = useApp((s) => s.transport.volume);
  const [muted, setMuted] = useState<number | undefined>(undefined);
  const wrap = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const popRef = useRef<HTMLDivElement>(null);
  const pop = usePresence(open || undefined, popRef);
  const set = (v: number) => bridge().dispatch({ type: "setVolume", data: { volume: v } });
  const toggle = () => {
    if (muted !== undefined) { set(muted); setMuted(undefined); } else { setMuted(volume); set(0); }
  };
  useEffect(() => {
    if (!open) return;
    wrap.current?.querySelector<HTMLInputElement>('input[type="range"]')?.focus({ preventScroll: true });
    const down = (e: MouseEvent) => { if (!wrap.current?.contains(e.target as Node)) setOpen(false); };
    window.addEventListener("mousedown", down, true);
    return () => window.removeEventListener("mousedown", down, true);
  }, [open, setOpen]);
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
    <div ref={wrap} className="np-volume-wrap" onKeyDown={(e) => { if (e.key === "Escape" && open) { e.preventDefault(); e.stopPropagation(); setOpen(false); button.current?.focus(); } }}>
      <button ref={button} type="button" className={`np-mode ${open ? "on" : ""}`} aria-expanded={open} aria-haspopup="dialog" aria-label={t("nowPlaying.volume")} title={t("nowPlaying.volume")} onClick={() => setOpen(!open)} data-testid="fs-pill-volume"><Icon name={volume === 0 ? "mute" : "volume"} size={22} /></button>
      {pop.value ? (
        <div {...pop.exitProps} className={`np-volume-pop ${pop.motion}`} role="dialog" aria-label={t("nowPlaying.volume")} data-testid="fs-volume-popover">
          <button type="button" className="np-icon-btn sm" aria-label={t("player.mute")} aria-pressed={volume === 0} title={volume === 0 ? t("a11y.unmute") : t("player.mute")} onClick={toggle} data-testid="fs-mute"><Icon name={volume === 0 ? "mute" : "volume"} size={20} /></button>
          <Slider min={0} max={1} step={0.01} value={volume} aria-label={t("player.volume")} aria-valuetext={volumeValueText(volume)} onKeyDown={onKey} onChange={(v) => { setMuted(undefined); set(v); }} onWheel={(e) => set(Math.max(0, Math.min(1, Math.round((volume - Math.sign(e.deltaY) * 0.05) * 100) / 100)))} data-testid="fs-volume" />
        </div>
      ) : null}
    </div>
  );
}
