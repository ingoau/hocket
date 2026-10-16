import { useEffect, useRef, useState } from "react";
import { t } from "@shared/strings";
import { NAV_VIEWS, sidebarPlaces } from "@shared/keymap";
import { executeAction } from "../store/actions";
import { useApp, type ViewName } from "../store/app";
import { useActions } from "../store/queries";
import { openContextMenu } from "./ContextMenu";
import { Icon } from "./Icon";
import { openItemsMenu } from "./ContextMenu";
import { Artwork } from "./Artwork";
import { useResolvedPins, performPin, type ResolvedPin } from "../lib/use-pins";
import { pinMenuItems } from "../lib/pin-menus";
import { pinTap } from "../lib/library-pins";

export function Sidebar() {
  const route = useApp((s) => s.route);
  const navigate = useApp((s) => s.navigate);
  const pins = useResolvedPins();
  const panels = useApp((s) => s.panels);
  const setPanels = useApp((s) => s.setPanels);
  const navActions = useActions("sidebar", { type: "none" });
  const [dragging, setDragging] = useState(false);
  const startX = useRef(0);
  const startW = useRef(0);

  useEffect(() => {
    if (!dragging) return;
    const move = (e: MouseEvent) => setPanels({ sidebarWidth: Math.max(160, Math.min(360, startW.current + e.clientX - startX.current)) });
    const up = () => setDragging(false);
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    return () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
  }, [dragging, setPanels]);

  const fallback = ["navigateHome", "navigateLibrary"];
  const fallbackIcon: Record<string, string> = { navigateHome: "home", navigateLibrary: "library", navigateAlbums: "album", navigateArtists: "artist", navigatePlaylists: "playlist", navigateTracks: "song", navigateGenres: "genre", navigateDownloads: "download", navigateFilters: "filter", navigateStats: "stats", navigateRecent: "restore", navigateSettings: "settings" };
  const items = sidebarPlaces(navActions.length ? navActions.map((a) => ({ id: a.id, label: a.label, icon: a.icon })) : fallback.map((id) => ({ id, label: t(`nav.${NAV_VIEWS[id] ?? "home"}` as never), icon: fallbackIcon[id] ?? "music" })))
    .map((it) => ({ ...it, view: it.view as ViewName | undefined }));
  const libraryViews: ViewName[] = ["library", "albums", "artists", "playlists", "songs", "genres", "filters", "downloads", "stats"];
  const pinnedOpen = (pins ?? []).some((rp) => isPinActive(rp));
  const isActive = (v: ViewName | undefined) => !!v && ((route.view === v && !(v === "downloads" && route.param === "offline")) || (v === "library" && !pinnedOpen && (libraryViews.includes(route.view) || ["album", "artist", "playlist", "genre", "filter"].includes(route.view)) && !items.some((it) => it.view === route.view)) || (v === "albums" && route.view === "album") || (v === "artists" && route.view === "artist") || (v === "playlists" && route.view === "playlist" && !route.param) || (v === "genres" && route.view === "genre") || (v === "filters" && route.view === "filter"));
  function isPinActive(rp: ResolvedPin): boolean {
    const view = rp.kind === "track" ? undefined : rp.kind;
    return !!view && route.view === view && route.id === rp.pin.id;
  }

  return (
    <nav className="sidebar" aria-label={t("nav.library")} data-testid="sidebar">
      <div className="sidebar-scroll">
        <div className="section-title">{t("nav.library")}</div>
        {items.map((it) => (
          <a key={it.id} href="#" className={`nav-item ${isActive(it.view) ? "active" : ""}`} aria-current={isActive(it.view) ? "page" : undefined} title={it.label} onClick={(e) => { e.preventDefault(); if (it.view) navigate({ view: it.view }); else void executeAction(it.id); }} data-testid={`nav-${it.view ?? it.id}`}>
            <Icon name={it.icon} size={18} filled={isActive(it.view)} />
            <span>{it.label}</span>
          </a>
        ))}
        <div className="section-title sidebar-pins-title">{t("library.pinned")}</div>
        {pins && !pins.length ? <p className="sidebar-pins-empty small muted">{t("library.pinsEmpty")}</p> : null}
        {(pins ?? []).map((rp) => {
          const active = isPinActive(rp);
          const tap = pinTap(rp.pin);
          const hint = tap === "play" ? t("library.pinPlays") : tap === "shuffle" ? t("library.pinShuffles") : t("library.pinOpens");
          return (
            <a
              key={`${rp.pin.kind}:${rp.pin.id}`}
              href="#"
              className={`nav-item pin-item ${active ? "active" : ""}`}
              aria-current={active ? "page" : undefined}
              title={`${rp.title} · ${t(`library.kind.${rp.kind}` as never)} · ${hint}`}
              onClick={(e) => { e.preventDefault(); performPin(rp); }}
              onContextMenu={(e) => {
                const items = pinMenuItems(rp.pin, rp.title, !rp.target);
                if (rp.target) void openContextMenu(e, rp.target, undefined, items);
                else openItemsMenu(e, items);
              }}
              data-testid="sidebar-pin"
            >
              <PinThumb rp={rp} />
              <span className="pin-text"><span className="pin-title">{rp.title}</span><span className="pin-kind">{t(`library.kind.${rp.kind}` as never)}</span></span>
              {tap !== "open" && rp.kind !== "track" ? <Icon name={tap} size={14} /> : null}
            </a>
          );
        })}
      </div>
      {/* Pinned below the scrolling list, like a native source list's footer. */}
      <div className="sidebar-foot">
        <a href="#" className={`nav-item ${route.view === "settings" ? "active" : ""}`} aria-current={route.view === "settings" ? "page" : undefined} title={t("nav.settings")} onClick={(e) => { e.preventDefault(); navigate({ view: "settings" }); }} data-testid="nav-settings">
          <Icon name="settings" size={18} filled={route.view === "settings"} />
          <span>{t("nav.settings")}</span>
        </a>
      </div>
      <div className={`resize-handle ${dragging ? "dragging" : ""}`} onMouseDown={(e) => { e.preventDefault(); startX.current = e.clientX; startW.current = panels.sidebarWidth; setDragging(true); }} role="separator" aria-orientation="vertical" aria-label={t("a11y.resizeSidebar")} aria-valuenow={panels.sidebarWidth} aria-valuemin={160} aria-valuemax={360} tabIndex={0}
        onKeyDown={(e) => { const d = e.key === "ArrowRight" ? 16 : e.key === "ArrowLeft" ? -16 : 0; if (!d) return; e.preventDefault(); e.stopPropagation(); setPanels({ sidebarWidth: Math.max(160, Math.min(360, panels.sidebarWidth + d)) }); }}
        data-testid="sidebar-resize" />
    </nav>
  );
}

/** A pin's artwork in its kind's shape; genres and filters (no cover) draw their icon. */
function PinThumb({ rp }: { rp: ResolvedPin }) {
  if (rp.kind === "genre" || rp.kind === "filter") return <span className={`pin-thumb icon-thumb ${rp.kind}`} aria-hidden="true"><Icon name={rp.kind} size={16} /></span>;
  return <span className={`pin-thumb ${rp.kind}`} aria-hidden="true"><Artwork id={rp.coverArt} size={64} round={rp.kind === "artist"} /></span>;
}
