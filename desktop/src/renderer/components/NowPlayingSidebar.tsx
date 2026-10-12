// The now playing side panel (docs/design.md, "Desktop layout"): one floating,
// scrolling pane. The overview shows the artwork, the title and artist with
// add to playlist and More, then three cards: Lyrics (the previous, current
// and next lines, following playback), Up next (the next three songs) and
// About (album, genre, plays, format). A cover whose bottom edge carries on
// (lib/immersive.ts, as on the phone's player) runs edge to edge at the top,
// its continuation behind the title and the cards; others are a rounded card.
// Each card's link drills in to its own
// view, with a back arrow: Lyrics (the synced lyrics), Queue (the queue
// selector, Autoplay and the timeline), About this song, and Recent queues.
// The player bar's lyrics and queue buttons open those views directly.
// Resizable by its left edge; below 900px it is a drawer over the content.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import type { ArtworkEdge, ImmersiveArtwork, QueueEntry, Track } from "@core/api";
import { t } from "@shared/strings";
import { SK } from "@shared/settings-keys";
import { showSideView, useApp, useSetting, type Route, type SideView } from "../store/app";
import { useQuery } from "../store/queries";
import { usePosition } from "../store/position";
import { bridge } from "../core/bridge";
import { executeAction } from "../store/actions";
import { fmtTime } from "../lib/format";
import { mapLyrics } from "../lib/lyrics-map";
import { NARROW, useMediaQuery, usePrefersReducedMotion } from "../lib/media";
import { usePresence } from "../lib/presence";
import { openContextMenu, openMenuFromButton } from "./ContextMenu";
import { QueuePanel } from "./QueuePanel";
import { SavedQueues } from "./SavedQueues";
import { LyricsView } from "./LyricsView";
import { EmptyState } from "./EmptyState";
import { Artwork } from "./Artwork";
import { AboutSong, formatLine } from "./AboutSong";
import { Icon } from "./Icon";
import { LyricsOptionsButton } from "./LyricsOptions";
import { drawContinuation, isFlat, prepareScene, useScene, withTone, type Scene } from "../lib/immersive";

const MIN_W = 300;
const MAX_W = 560;
/** Where each drill-in goes back to. */
const PARENT: Record<Exclude<SideView, "overview">, SideView> = { lyrics: "overview", queue: "overview", about: "overview", recent: "queue" };
const TITLES: Record<Exclude<SideView, "overview">, Parameters<typeof t>[0]> = { lyrics: "lyrics.title", queue: "queue.title", about: "nowPlaying.aboutSong", recent: "queue.recentTitle" };

export function NowPlayingSidebar() {
  const panels = useApp((s) => s.panels);
  const setPanels = useApp((s) => s.setPanels);
  const [widthDrag, setWidthDrag] = useState<{ x: number; w: number } | undefined>(undefined);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!widthDrag) return;
    const move = (e: MouseEvent) => setPanels({ rightWidth: Math.max(MIN_W, Math.min(MAX_W, widthDrag.w - (e.clientX - widthDrag.x))) });
    const up = () => setWidthDrag(undefined);
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    return () => { window.removeEventListener("mousemove", move); window.removeEventListener("mouseup", up); };
  }, [widthDrag, setPanels]);

  const narrow = useMediaQuery(NARROW);
  const drawerOpen = useApp((s) => s.drawerOpen);
  const setDrawerOpen = useApp((s) => s.setDrawerOpen);
  // The drawer withdraws before it unmounts; the docked panel is part of the
  // layout (the content pane widens the moment it closes), so it just goes.
  const open = narrow ? drawerOpen : panels.rightOpen;
  const presence = usePresence(open || undefined, ref, { instant: !narrow });
  if (!presence.value) return null;
  const view = panels.rightView;
  return (
    <aside {...presence.exitProps} id="side-panel" className={`right-panel ${narrow ? "drawer" : ""} ${presence.closing ? "closing" : ""}`} aria-label={t("a11y.sidePanel")} data-view={view} data-testid="right-panel"
      onKeyDown={narrow ? (e) => { if (e.key === "Escape" && !e.defaultPrevented) { e.preventDefault(); e.stopPropagation(); setDrawerOpen(false); document.querySelector<HTMLElement>('[data-testid="toggle-side-panel"]')?.focus(); } } : undefined}>
      <div className="resize-handle" style={{ left: -3, right: "auto" }} onMouseDown={(e) => { e.preventDefault(); setWidthDrag({ x: e.clientX, w: panels.rightWidth }); }} role="separator" aria-orientation="vertical" aria-label={t("a11y.resizeSidePanel")} aria-valuenow={panels.rightWidth} aria-valuemin={MIN_W} aria-valuemax={MAX_W} tabIndex={0}
        onKeyDown={(e) => { const d = e.key === "ArrowLeft" ? 16 : e.key === "ArrowRight" ? -16 : 0; if (!d) return; e.preventDefault(); e.stopPropagation(); setPanels({ rightWidth: Math.max(MIN_W, Math.min(MAX_W, panels.rightWidth + d)) }); }} data-testid="side-panel-resize" />
      <div className="side-pane">
        <SideViews view={view} />
      </div>
    </aside>
  );
}

/** The current view, sliding in from the right when drilling in and from the left when going back (M3 shared axis). */
const depth = (v: SideView) => (v === "overview" ? 0 : v === "recent" ? 2 : 1);

function SideViews({ view }: { view: SideView }) {
  const [shown, setShown] = useState<{ view: SideView; from?: SideView; direction?: "forward" | "back" }>({ view });
  if (shown.view !== view) setShown({ view, from: shown.view, direction: depth(view) >= depth(shown.view) ? "forward" : "back" });
  const ref = useRef<HTMLDivElement>(null);
  // Moving between views from inside the panel (a card's link, the back arrow), focus follows:
  // onto the back arrow going in, onto the link that went in coming back. Focus elsewhere (the
  // player bar's buttons) stays where it is.
  useLayoutEffect(() => {
    const from = shown.from;
    if (!from) return;
    const active = document.activeElement;
    if (active && active !== document.body && !ref.current?.parentElement?.contains(active)) return;
    const back = shown.direction === "back" ? ref.current?.querySelector<HTMLElement>(`[data-drill="${from}"]`) : undefined;
    (back ?? ref.current?.querySelector<HTMLElement>('[data-testid="sidebar-back"]'))?.focus({ preventScroll: true });
  }, [shown]);
  return (
    <div ref={ref} key={view} className={`side-view ${shown.direction ? `enter-${shown.direction}` : ""}`} data-testid={`sidebar-${view}`}>
      {view === "overview" ? <Overview /> : <DrillIn view={view} />}
    </div>
  );
}

function DrillIn({ view }: { view: Exclude<SideView, "overview"> }) {
  const now = useApp((s) => s.nowPlaying);
  const navigate = useApp((s) => s.navigate);
  return (
    <section className={`side-drill side-drill-${view}`} aria-labelledby={`side-title-${view}`}>
      <div className="side-drill-head">
        <button type="button" className="side-icon-btn" aria-label={t("nowPlaying.back")} title={t("nowPlaying.back")} onClick={() => showSideView(PARENT[view])} data-testid="sidebar-back"><Icon name="back" size={22} /></button>
        <h2 className="side-drill-title" id={`side-title-${view}`}>{t(TITLES[view])}</h2>
        <span className="spacer" />
        {view === "lyrics" ? <LyricsOptionsButton className="side-icon-btn" placement="below" /> : null}
        {view === "queue" ? <button type="button" className="side-icon-btn" title={t("queue.clear")} aria-label={t("queue.clear")} onClick={() => bridge().dispatch({ type: "clearQueue" })} data-testid="queue-clear"><Icon name="trash" size={20} /></button> : null}
      </div>
      <div className="side-drill-body">
        {view === "lyrics" ? <LyricsPane /> : null}
        {view === "queue" ? <QueuePanel /> : null}
        {view === "recent" ? <SavedQueues /> : null}
        {view === "about" ? <div className="side-scroll"><AboutSong variant="side" trackId={now?.track.id} onNavigate={(r: Route) => navigate(r)} /></div> : null}
      </div>
    </section>
  );
}

/** Muted text's opacity over the continuation (--fg-muted there): the scrim is sized for it. */
const MUTED_ALPHA = 0.8;

function Overview() {
  const now = useApp((s) => s.nowPlaying);
  const navigate = useApp((s) => s.navigate);
  const track = now?.track;
  const scene = useOverviewScene(track?.coverArt);
  const overview = useRef<HTMLDivElement>(null);
  const immersive = !!scene.edge;
  // The continuation fades back into the pane below the Lyrics card (or the title, without one):
  // its offset, kept on the overview for the stylesheet as the content's height changes.
  useLayoutEffect(() => {
    const el = overview.current;
    if (!el || !immersive) return;
    const place = () => {
      const anchor = el.querySelector<HTMLElement>('[data-card="lyrics"]') ?? el.querySelector<HTMLElement>(".side-title-row");
      if (anchor) el.style.setProperty("--side-fade-at", `${anchor.offsetTop + anchor.offsetHeight}px`);
    };
    place();
    const ro = new ResizeObserver(place);
    ro.observe(el);
    return () => ro.disconnect();
  }, [immersive]);
  if (!track || !now) return <div className="side-scroll"><EmptyState message={t("player.nothingPlaying")} icon="music" action={<span className="small muted">{t("player.nothingPlayingHint")}</span>} testId="sidebar-empty" /></div>;
  const { layers, edge, fading, settle } = scene;
  const style = edge ? ({ "--side-fill": fillOf(edge) } as CSSProperties) : undefined;
  return (
    <div className="side-scroll" data-testid="sidebar-scroll">
      <div ref={overview} className={`side-overview ${edge ? "immersive" : ""}`} data-tone={edge ? (edge.light ? "light" : "dark") : undefined} data-flat={edge && isFlat(edge) ? "" : undefined} style={style} data-testid="side-overview">
      {edge ? layers.map((l, i) => (l.edge ? <SideContinuation key={l.scene.coverArt} img={l.scene.img} edge={l.edge} entering={fading && i === layers.length - 1} /> : null)) : null}
      <div className="side-art" data-testid="sidebar-art" data-immersive={edge ? edge.style : undefined}>
        {layers.length ? layers.map((l, i) => (l.scene.url ? <img key={l.scene.coverArt} src={l.scene.url} alt="" draggable={false} className={fading && i === layers.length - 1 ? "fade-in-art" : ""} onAnimationEnd={settle} /> : <div key={l.scene.coverArt} className="placeholder"><Icon name="music" size={48} /></div>)) : <div className="placeholder"><Icon name="music" size={48} /></div>}
      </div>
      <div className="side-title-row">
        <div className="side-titles">
          <h2 className="side-title" title={track.title}>{track.albumId ? <button type="button" className="side-text-link" onClick={() => navigate({ view: "album", id: track.albumId as string })}>{track.title}</button> : track.title}</h2>
          <div className="side-artist" title={track.artist}>{track.artistId ? <button type="button" className="side-text-link" onClick={() => navigate({ view: "artist", id: track.artistId as string })}>{track.artist ?? t("misc.unknownArtist")}</button> : (track.artist ?? t("misc.unknownArtist"))}</div>
        </div>
        <button type="button" className="side-icon-btn" aria-label={t("action.addToPlaylist")} title={t("action.addToPlaylist")} onClick={() => void executeAction("addToPlaylist")} data-testid="sidebar-add"><Icon name="playlistAdd" size={22} /></button>
        <button type="button" className="side-icon-btn" aria-label={t("misc.more")} title={t("misc.more")} aria-haspopup="menu" onClick={(e) => openMenuFromButton(e, { type: "queueItems", data: { keys: [now.item.key] } })} data-testid="sidebar-more"><Icon name="more" size={22} /></button>
      </div>
      <LyricsCard trackId={track.id} durationMs={track.durationMs} />
      <UpNextCard />
      <AboutCard trackId={track.id} />
      </div>
    </div>
  );
}

/** What the continuation carries on in past its canvas: the edge's base colour under its scrim, as `r, g, b`. */
function fillOf(edge: ArtworkEdge): string {
  const under = edge.light ? 255 : 0;
  return [16, 8, 0].map((shift) => Math.round(((edge.baseColor >> shift) & 255) * (1 - edge.scrim) + under * edge.scrim)).join(", ");
}

/** The latest a new cover's scene has faded in over the last (side-art-in is quicker). */
const SCENE_FADE_MS = 2000;

/**
 * The overview's artwork, one scene per cover (lib/immersive.ts): its layout's bottom edge decides
 * between edge to edge (with its continuation, in the tone the text follows) and a rounded card.
 * The next cover's scene is prepared ahead; a new one fades in over the last, which stays beneath
 * only when laid out the same way.
 */
function useOverviewScene(coverArt: string | undefined) {
  const preference = useSetting<ImmersiveArtwork>(SK.displayImmersiveArtwork, "automatic");
  const themeSetting = useSetting<"system" | "light" | "dark">(SK.displayTheme, "system");
  const systemDark = useMediaQuery("(prefers-color-scheme: dark)");
  const dark = themeSetting === "dark" || (themeSetting === "system" && systemDark);
  const scene = useScene(coverArt, preference, dark);
  const next = useApp((s) => (s.queue.playingNext[0] ?? s.queue.upcoming[0])?.track.coverArt);
  useEffect(() => {
    if (next) void prepareScene(next, preference, dark);
  }, [next, preference, dark]);
  const edgeOf = (s: Scene | undefined): ArtworkEdge | undefined => {
    const e = s?.layout?.bottom;
    return e && e.style !== "card" ? withTone(e, e.light, MUTED_ALPHA) : undefined;
  };
  const [shown, setShown] = useState<Scene[]>([]);
  if (scene ? shown.at(-1) !== scene : shown.length > 0) setShown(scene ? [...shown.slice(-1), scene] : []);
  const settle = () => setShown((s) => s.slice(-1));
  useEffect(() => {
    if (shown.length < 2) return;
    const id = window.setTimeout(settle, SCENE_FADE_MS);
    return () => window.clearTimeout(id);
  }, [shown]);
  const edge = edgeOf(scene);
  const layers = shown.filter((s, i) => i === shown.length - 1 || !edgeOf(s) === !edge).map((s) => ({ scene: s, edge: edgeOf(s) }));
  return { layers, edge, fading: layers.length > 1, settle };
}

/**
 * The continuation under an edge-to-edge artwork: a canvas the artwork's width and twice its
 * height (the artwork, then as long again carried on), drawn once per cover and width; past it
 * the overview carries on in the edge's base colour, and both fade into the pane below the
 * Lyrics card (CSS).
 */
function SideContinuation({ img, edge, entering }: { img: HTMLImageElement | undefined; edge: ArtworkEdge; entering: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || !width) return;
    const dpr = window.devicePixelRatio || 1;
    el.width = Math.round(width * dpr);
    el.height = Math.round(width * 2 * dpr);
    drawContinuation(el, img, edge, "bottom", undefined, { bottomFade: false });
  }, [img, edge, width]);
  return <canvas ref={ref} className={`side-continuation ${entering ? "fade-in-art" : ""}`} aria-hidden="true" data-testid="sidebar-continuation" data-style={edge.style} />;
}

function Card({ id, title, link, onLink, children, testId, drill }: { id: string; title: string; link: string; onLink: () => void; children: React.ReactNode; testId: string; drill: SideView }) {
  return (
    <section className="side-card" aria-labelledby={`side-card-${id}`} data-card={id} data-testid={testId}>
      <div className="side-card-head">
        <h3 className="side-card-title" id={`side-card-${id}`}>{title}</h3>
        <button type="button" className="side-card-link" onClick={onLink} data-drill={drill} data-testid={`${testId}-link`}>{link}</button>
      </div>
      {children}
    </section>
  );
}

/** The previous, current and next lines, following playback (static for unsynced lyrics). Hidden when the song has none. */
function LyricsCard({ trackId, durationMs }: { trackId: string; durationMs: number | undefined }) {
  const lyrics = useApp((s) => (s.lyrics && s.lyrics.trackId === trackId ? s.lyrics.lyrics : undefined));
  const mapped = useMemo(() => (lyrics ? mapLyrics(lyrics) : undefined), [lyrics]);
  const lines = useMemo(() => mapped?.lines.filter((l) => !l.isBG && l.words.some((w) => w.word.trim())) ?? [], [mapped]);
  if (!mapped || !lines.length) return null;
  return (
    <Card id="lyrics" title={t("lyrics.title")} link={t("nowPlaying.showLyrics")} onLink={() => showSideView("lyrics")} drill="lyrics" testId="sidebar-lyrics-card">
      {mapped.synced ? <SyncedPeek lines={lines.map((l) => ({ text: l.words.map((w) => w.word).join(""), start: l.startTime }))} durationMs={durationMs} /> : (
        <ol className="lyrics-peek" aria-label={t("lyrics.title")}>
          {lines.slice(0, 3).map((l, i) => <li key={i} className="peek-line">{l.words.map((w) => w.word).join("")}</li>)}
        </ol>
      )}
    </Card>
  );
}

function SyncedPeek({ lines, durationMs }: { lines: { text: string; start: number }[]; durationMs: number | undefined }) {
  const stamp = useApp((s) => s.transport.position);
  const offset = useApp((s) => s.connection.clockOffsetMs);
  const pos = usePosition(stamp, durationMs, offset);
  const reduced = usePrefersReducedMotion();
  let active = -1;
  for (let i = 0; i < lines.length; i++) {
    if ((lines[i]?.start ?? 0) <= pos) active = i;
    else break;
  }
  // Before the first line: the first two, still to come.
  const shown = active < 0 ? [undefined, 0, 1] : [active - 1, active, active + 1];
  return (
    <ol className={`lyrics-peek synced ${reduced ? "" : "moving"}`} aria-label={t("lyrics.title")} data-active={active}>
      {shown.map((i, slot) => {
        const line = i !== undefined && i >= 0 ? lines[i] : undefined;
        const current = slot === 1 && active >= 0;
        return <li key={i ?? `none-${slot}`} className={`peek-line ${current ? "current" : ""}`} aria-current={current ? "true" : undefined}>{line?.text ?? " "}</li>;
      })}
    </ol>
  );
}

function UpNextCard() {
  const queue = useApp((s) => s.queue);
  const next: QueueEntry[] = [...queue.playingNext, ...queue.upcoming].slice(0, 3);
  const d = bridge().dispatch;
  return (
    <Card id="upnext" title={t("nowPlaying.upNext")} link={t("nowPlaying.openQueue")} onLink={() => showSideView("queue")} drill="queue" testId="sidebar-upnext-card">
      {next.length ? (
        <ul className="plain-list upnext-list">
          {next.map((e) => (
            <li key={e.item.key} className="upnext-row" onDoubleClick={() => d({ type: "jumpToQueueItem", data: { key: e.item.key } })} onContextMenu={(ev) => void openContextMenu(ev, { type: "queueItems", data: { keys: [e.item.key] } })} data-testid="sidebar-upnext-row">
              <Artwork id={e.track.coverArt} size={100} className="art" />
              <div className="text">
                <div className="t1">{e.track.title}</div>
                <div className="t2">{e.item.source.type === "autoplay" ? t("queue.autoplayFrom", { reason: e.item.source.data.reason }) : e.track.artist}</div>
              </div>
              <span className="dur">{fmtTime(e.track.durationMs)}</span>
            </li>
          ))}
        </ul>
      ) : <p className="side-card-empty">{t("nowPlaying.nothingNext")}</p>}
    </Card>
  );
}

function AboutCard({ trackId }: { trackId: string }) {
  const { data } = useQuery(() => ({ type: "track", data: { id: trackId } }), "trackDetail", [trackId]);
  const tr = data as Track | undefined;
  const facts: [string, string | number | undefined][] = tr ? [
    [t("about.album"), tr.album],
    [t("about.genre"), tr.genre],
    [t("about.playCount"), tr.playCount],
    [t("about.format"), formatLine(tr, true) || undefined],
  ] : [];
  return (
    <Card id="about" title={t("nowPlaying.view.about")} link={t("nowPlaying.moreAbout")} onLink={() => showSideView("about")} drill="about" testId="sidebar-about-card">
      <dl className="side-facts">
        {facts.filter(([, v]) => v !== undefined && v !== "").map(([k, v]) => (
          <div key={k} className="side-fact"><dt>{k}</dt><dd>{v}</dd></div>
        ))}
      </dl>
    </Card>
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
