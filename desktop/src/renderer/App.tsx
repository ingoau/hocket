// Main window shell: server setup as the entire first screen, otherwise the
// sidebar / content / right panel / player bar layout.
import { useEffect, useRef, useState, type CSSProperties } from "react";
import type { Command } from "@core/api";
import { parseDeepLink } from "@shared/deep-link";
import { t } from "@shared/strings";
import { useApp, useSetting } from "./store/app";
import { useGlobalKeyboard } from "./store/keyboard";
import { bridge } from "./core/bridge";
import { extractAccent } from "./lib/accent";
import { resolveArtwork } from "./components/Artwork";
import { Sidebar } from "./components/Sidebar";
import { TopBar } from "./components/TopBar";
import { PlayerBar } from "./components/PlayerBar";
import { RightPanel } from "./components/RightPanel";
import { ContextMenu } from "./components/ContextMenu";
import { CommandPalette } from "./components/CommandPalette";
import { Toasts } from "./components/Toasts";
import { Dialogs } from "./components/Dialogs";
import { Router } from "./views/Router";
import { Setup } from "./views/Setup";
import { FullscreenPlayer } from "./views/FullscreenPlayer";
import { executeAction } from "./store/actions";
import { DEFAULT_ACCENT, SK } from "@shared/settings-keys";
import { accentTokens, type ThemeName } from "./lib/contrast";
import { nowPlayingAnnouncement } from "./lib/a11y";
import { NARROW, useMediaQuery } from "./lib/media";
import { Icon } from "./components/Icon";
import { isOfflineNotice, noticeText } from "./lib/notice";

export function App() {
  const ready = useApp((s) => s.ready);
  // The core probes before installing: ServersChanged only arrives once a server is real.
  const hasServer = useApp((s) => s.servers.length > 0);
  const coreKind = useApp((s) => s.meta?.coreKind);
  const network = useApp((s) => s.network);
  const panels = useApp((s) => s.panels);
  // A modal (dialog, palette, fullscreen player) makes the rest of the window inert:
  // no focus, no pointer, hidden from assistive tech.
  const modal = useApp((s) => !!s.dialog || s.paletteOpen || s.fullscreen);
  const narrow = useMediaQuery(NARROW);
  useTheme();
  useGlobalKeyboard(true);
  useDeepLinks();

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
      <div className="app-body" inert={modal} style={{ "--sidebar-w": `${panels.sidebarWidth}px`, "--right-w": panels.rightOpen && !narrow ? `${panels.rightWidth}px` : "0px" } as CSSProperties} data-testid="app-body">
        <Sidebar />
        <main className="content" id="main" tabIndex={-1} data-testid="content">
          <Router />
        </main>
        <RightPanel />
      </div>
      <PlayerBar inert={modal} />
      <NowPlayingAnnouncer />
      {/* Covers the whole window (sidebar, panels and player bar included). */}
      <FullscreenPlayer />
      <ContextMenu />
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

/** Theme (light/dark/system), accent colour setting and dynamic accent from artwork. */
export function useTheme() {
  const theme = useSetting<"system" | "light" | "dark">(SK.displayTheme, "system");
  const accentSetting = useSetting<string | null>(SK.displayAccent, null) || DEFAULT_ACCENT;
  const dynamic = useSetting(SK.displayDynamicColour, true);
  const cover = useApp((s) => s.nowPlaying?.track.coverArt);
  const accent = useApp((s) => s.accent);
  const setAccent = useApp((s) => s.setAccent);
  const [resolved, setResolved] = useState<ThemeName>("light");
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const name: ThemeName = theme === "system" ? (mq.matches ? "dark" : "light") : theme;
      document.documentElement.setAttribute("data-theme", name);
      setResolved(name);
    };
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);
  useEffect(() => {
    if (!dynamic || !cover) {
      setAccent(undefined);
      return;
    }
    let alive = true;
    void resolveArtwork(cover, 64).then((url) => url && extractAccent(url)).then((c) => alive && setAccent(c));
    return () => { alive = false; };
  }, [dynamic, cover, setAccent]);
  // Every accent-derived colour is contrast-checked for the theme (lib/contrast.ts).
  useEffect(() => {
    const tokens = accentTokens(accent ?? accentSetting, resolved);
    const style = document.documentElement.style;
    style.setProperty("--accent", tokens.accent);
    style.setProperty("--accent-fg", tokens.accentFg);
    style.setProperty("--accent-text", tokens.accentText);
    style.setProperty("--accent-inverse", tokens.accentInverse);
    style.setProperty("--accent-tint", tokens.accentTint);
    style.setProperty("--focus-ring", tokens.focusRing);
  }, [accent, accentSetting, resolved]);
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
