// Main window shell: server setup as the entire first screen, otherwise the
// sidebar / content / right panel / player bar layout.
import { useEffect } from "react";
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

export function App() {
  const ready = useApp((s) => s.ready);
  // The core probes before installing: ServersChanged only arrives once a server is real.
  const hasServer = useApp((s) => s.servers.length > 0);
  const fullscreen = useApp((s) => s.fullscreen);
  const coreKind = useApp((s) => s.meta?.coreKind);
  const network = useApp((s) => s.network);
  useTheme();
  useGlobalKeyboard(true);
  useDeepLinks();

  if (!ready) return <div className="setup"><div className="muted">{t("app.loading")}</div></div>;
  if (!hasServer) return <Setup />;
  return (
    <div className="app" data-testid="app">
      <div>
        {coreKind === "fake" ? <div className="dev-banner" data-testid="dev-banner">{t("app.devBanner")}</div> : null}
        {network?.kind === "offline" ? <div className="offline-banner">{t("misc.offline")}</div> : null}
        <TopBar />
      </div>
      <div className="app-body">
        <Sidebar />
        <main className="content" data-testid="content">
          <Router />
          {fullscreen ? <FullscreenPlayer /> : null}
        </main>
        <RightPanel />
      </div>
      <PlayerBar />
      <ContextMenu />
      <CommandPalette />
      <Dialogs />
      <Toasts />
    </div>
  );
}

/** Theme (light/dark/system), accent colour setting and dynamic accent from artwork. */
export function useTheme() {
  const theme = useSetting<"system" | "light" | "dark">(SK.displayTheme, "system");
  const accentSetting = useSetting<string | null>(SK.displayAccent, null) || DEFAULT_ACCENT;
  const dynamic = useSetting(SK.displayDynamicColour, true);
  const cover = useApp((s) => s.nowPlaying?.track.coverArt);
  const accent = useApp((s) => s.accent);
  const setAccent = useApp((s) => s.setAccent);
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => document.documentElement.setAttribute("data-theme", theme === "system" ? (mq.matches ? "dark" : "light") : theme);
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
  useEffect(() => {
    document.documentElement.style.setProperty("--accent", accent ?? accentSetting);
  }, [accent, accentSetting]);
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
