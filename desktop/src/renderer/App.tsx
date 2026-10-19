// Main window shell: server setup as the entire first screen, otherwise the
// sidebar / content / right panel / player bar layout.
import { lazy, Suspense, useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import type { Command } from "@core/api";
import { parseDeepLink } from "@shared/deep-link";
import { t } from "@shared/strings";
import { useApp } from "./store/app";
import { useGlobalKeyboard } from "./store/keyboard";
import { bridge } from "./core/bridge";
import { Sidebar } from "./components/Sidebar";
import { EditPinsPanel } from "./components/EditPinsPanel";
import { TopBar } from "./components/TopBar";
import { PlayerBar } from "./components/PlayerBar";
import { NowPlayingSidebar } from "./components/NowPlayingSidebar";
import { PlaylistPicker } from "./components/PlaylistPicker";
import { ContextMenu } from "./components/ContextMenu";
import { CommandPalette } from "./components/CommandPalette";
import { Toasts } from "./components/Toasts";
import { Dialogs } from "./components/Dialogs";
import { Router } from "./views/Router";
import { Setup } from "./views/Setup";
import { executeAction } from "./store/actions";
import { nowPlayingAnnouncement } from "./lib/a11y";
import { NARROW, useMediaQuery } from "./lib/media";
import { Icon } from "./components/Icon";
import { isOfflineNotice, noticeText } from "./lib/notice";
import { useQueueDropReveal } from "./lib/queue-drag";
import { useTheme } from "./lib/theme";
import { handOnFocus } from "./lib/focus";
import { loadAmll } from "./lib/amll";
import { ErrorBoundary } from "./components/ErrorBoundary";

// One retry: React.lazy keeps a failed import for good, which would leave fullscreen broken.
const loadFullscreen = () => import("./views/FullscreenPlayer").then((m) => ({ default: m.FullscreenPlayer }));
const FullscreenPlayer = lazy(() => loadFullscreen().catch(loadFullscreen));

export function App() {
  const ready = useApp((s) => s.ready);
  // The core probes before installing: ServersChanged only arrives once a server is real.
  const hasServer = useApp((s) => s.servers.length > 0);
  const coreKind = useApp((s) => s.meta?.coreKind);
  const network = useApp((s) => s.network);
  const panels = useApp((s) => s.panels);
  const queueDropReveal = useApp((s) => s.queueDropReveal);
  // A modal (dialog, palette, fullscreen player) makes the rest of the window inert:
  // no focus, no pointer, hidden from assistive tech.
  const fullscreen = useApp((s) => s.fullscreen);
  const modal = useApp((s) => !!s.dialog || s.paletteOpen || s.fullscreen);
  const narrow = useMediaQuery(NARROW);
  const loadedFs = useFullscreenChunk(ready, fullscreen);
  useLyricPlayerPreload();
  // The fullscreen player makes the rest of the window inert: if it fails it closes (the
  // window works again) and the next open starts afresh.
  const [fsFailures, setFsFailures] = useState(0);
  const onFullscreenError = () => {
    setFsFailures((n) => n + 1);
    useApp.getState().setFullscreen(false);
  };
  useTheme();
  useGlobalKeyboard(true);
  useDeepLinks();
  useQueueDropReveal();

  if (!ready) return <div className="setup" role="main"><div className="muted" role="status">{t("app.loading")}</div></div>;
  if (!hasServer) return <Setup />;
  const skip = (e: React.MouseEvent) => {
    e.preventDefault();
    document.getElementById("main")?.focus();
  };
  return (
    <div className={`app ${narrow ? "narrow" : ""}`} data-testid="app">
      <header className="app-header" inert={modal}>
        <a href="#main" className="skip-link" onClick={skip} data-testid="skip-link">{t("a11y.skipToContent")}</a>
        {coreKind === "fake" ? <div className="dev-banner" data-testid="dev-banner">{t("app.devBanner")}</div> : null}
        <OfflineBanner offline={network?.kind === "offline"} />
        <TopBar />
      </header>
        <div className="app-body" inert={modal} style={{ "--sidebar-w": `${panels.sidebarWidth}px`, "--right-w": (panels.rightOpen || queueDropReveal) && !narrow ? `${panels.rightWidth}px` : "0px" } as CSSProperties} data-testid="app-body">
        <Sidebar />
        <main className="content" id="main" tabIndex={-1} data-testid="content">
          {/* Edit pins opens at the content's leading edge, beside the sidebar it edits. */}
          <EditPinsPanel />
          <Router />
        </main>
        <ErrorBoundary name="side panel"><NowPlayingSidebar /></ErrorBoundary>
      </div>
      <PlayerBar inert={modal} />
      <NowPlayingAnnouncer />
      {/* Covers the whole window (sidebar, panels and player bar included). Kept mounted after first open so the close fade still runs. */}
      {loadedFs ? <ErrorBoundary key={fsFailures} name="fullscreen player" fallback={null} onError={onFullscreenError}><Suspense fallback={null}><FullscreenPlayer /></Suspense></ErrorBoundary> : null}
      <ContextMenu />
      <PlaylistPicker />
      <CommandPalette />
      <Dialogs />
      <Toasts />
    </div>
  );
}

/**
 * Offline: the core skips what isn't downloaded or fully cached and says so
 * with a PlayerNotice; show it (or plain "Offline") with the way to the list
 * of what plays. The live region stays mounted so a new notice is announced.
 */
function OfflineBanner({ offline }: { offline: boolean }) {
  const notice = useApp((s) => (isOfflineNotice(s.playerNotice) ? noticeText(s.playerNotice) : undefined));
  const navigate = useApp((s) => s.navigate);
  const show = offline || !!notice;
  return (
    <div role="status" className="offline-live" data-testid="offline-live">
      {show ? (
        <div className="offline-banner" data-testid="offline-banner">
          <Icon name="offline" size={12} />
          <span data-testid="offline-banner-text">{notice ?? t("misc.offline")}</span>
          <button type="button" className="link-button" onClick={() => navigate({ view: "downloads", param: "offline" })} data-testid="offline-banner-show">{t("misc.offlineShow")}</button>
        </div>
      ) : null}
    </div>
  );
}

/**
 * A polite live region that announces the track when it changes, and nothing
 * else (never position ticks, lyrics lines or buffering).
 */
function NowPlayingAnnouncer() {
  const id = useApp((s) => s.nowPlaying?.track.id);
  const title = useApp((s) => s.nowPlaying?.track.title);
  const artist = useApp((s) => s.nowPlaying?.track.artist);
  const [text, setText] = useState("");
  // What was already playing when the window opened is not a change: stay quiet.
  const last = useRef<string | undefined>(id);
  useEffect(() => {
    if (id === last.current) return;
    last.current = id;
    if (id) setText(nowPlayingAnnouncement(title, artist));
  }, [id, title, artist]);
  return <div className="sr-only" role="status" aria-live="polite" aria-atomic="true" data-testid="now-playing-announcer">{text}</div>;
}

/**
 * Whether to mount the lazily loaded fullscreen player: once the window is idle after startup
 * (so it is normally loaded before the first open), or at once when it opens first.
 */
function useFullscreenChunk(ready: boolean, fullscreen: boolean): boolean {
  const [load, setLoad] = useState(false);
  useEffect(() => {
    if (load || !ready) return;
    const id = requestIdleCallback(() => setLoad(true), { timeout: 5000 });
    return () => cancelIdleCallback(id);
  }, [ready, load]);
  // Opened before the chunk arrived: the shell turns inert (and drops focus) before the player
  // mounts, so hand it the opener now for the player to return focus to on close.
  useLayoutEffect(() => {
    if (!fullscreen || load) return;
    const a = document.activeElement;
    // From a menu, the menu has already handed its trigger on.
    if (a instanceof HTMLElement && a !== document.body && !a.closest("[role=menu]")) handOnFocus(a);
    setLoad(true);
  }, [fullscreen, load]);
  return load || fullscreen;
}

/** Synced lyrics have arrived for what is playing: load the lyric player when idle, ahead of a lyrics view. */
function useLyricPlayerPreload(): void {
  const synced = useApp((s) => !!s.lyrics?.lyrics && s.lyrics.lyrics.tier !== "unsynced");
  useEffect(() => {
    if (!synced) return;
    const id = requestIdleCallback(() => void loadAmll().catch(() => undefined), { timeout: 3000 });
    return () => cancelIdleCallback(id);
  }, [synced]);
}

/** A `hocket://track/<id>` link becomes a toast with a Play-next action when the id is one of the current server's tracks. */
async function confirmDeepLinkTrack(id: string): Promise<void> {
  const b = bridge();
  const serverId = useApp.getState().servers[0]?.id;
  if (!serverId) return;
  try {
    const r = await b.query({ type: "track", data: { id } });
    if (r.type !== "trackDetail" || !r.data || r.data.serverId !== serverId) return;
    const command: Command = { type: "playNext", data: { server_id: serverId, track_ids: [id] } };
    useApp.getState().applyEvent({ type: "toast", data: { toast: { id: `deeplink-${id}`, message: t("toast.deepLinkTrack", { title: r.data.title }), actionLabel: t("toast.deepLinkPlay"), actionCommand: JSON.stringify(command), durationMs: 8000 } } });
  } catch (err) {
    console.warn("deep link track lookup failed", err);
  }
}

function useDeepLinks() {
  const navigate = useApp((s) => s.navigate);
  useEffect(() => {
    const b = bridge();
    const offLink = b.onDeepLink((url) => {
      // Main already validated and canonicalised the link.
      const link = parseDeepLink(url);
      if (!link || link.kind === "open") return;
      if (link.kind === "track") {
        // Never mutate the queue on an external request: confirm with a
        // toast, and only for a track the current server knows.
        void confirmDeepLinkTrack(link.id);
        return;
      }
      navigate({ view: link.kind, id: link.id });
    });
    const offAction = b.onUiAction((id) => void executeAction(id));
    return () => { offLink(); offAction(); };
  }, [navigate]);
}
