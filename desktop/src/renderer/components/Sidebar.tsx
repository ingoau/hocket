import { useEffect, useRef, useState } from "react";
import { t } from "@shared/strings";
import { NAV_VIEWS } from "@shared/keymap";
import { executeAction } from "../store/actions";
import { useApp, type ViewName } from "../store/app";
import { useActions, useQuery } from "../store/queries";
import { openContextMenu } from "./ContextMenu";
import { Icon } from "./Icon";
import { filterName } from "../lib/filters";
import { AVAILABLE_OFFLINE_FILTER_ID } from "@shared/constants";

export function Sidebar() {
  const route = useApp((s) => s.route);
  const navigate = useApp((s) => s.navigate);
  const serverId = useApp((s) => s.servers[0]?.id);
  const filters = useApp((s) => s.filters);
  const panels = useApp((s) => s.panels);
  const setPanels = useApp((s) => s.setPanels);
  const navActions = useActions("sidebar", { type: "none" });
  const { data: playlists } = useQuery(() => (serverId ? { type: "playlists", data: { server_id: serverId } } : null), "playlists", [serverId]);
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

  const fallback = ["navigateHome", "navigateAlbums", "navigateArtists", "navigatePlaylists", "navigateTracks", "navigateGenres", "navigateDownloads", "navigateFilters", "navigateStats"];
  const fallbackIcon: Record<string, string> = { navigateHome: "home", navigateAlbums: "album", navigateArtists: "artist", navigatePlaylists: "playlist", navigateTracks: "song", navigateGenres: "genre", navigateDownloads: "download", navigateFilters: "filter", navigateStats: "stats", navigateRecent: "restore", navigateSettings: "settings" };
  const items = (navActions.length ? navActions.map((a) => ({ id: a.id, label: a.label, icon: a.icon })) : fallback.map((id) => ({ id, label: t(`nav.${NAV_VIEWS[id] ?? "home"}` as never), icon: fallbackIcon[id] ?? "music" })))
    .filter((it) => it.id !== "navigateSettings")
    .map((it) => ({ ...it, view: NAV_VIEWS[it.id] as ViewName | undefined }));
  const isActive = (v: ViewName | undefined) => !!v && ((route.view === v && !(v === "downloads" && route.param === "offline")) || (v === "albums" && route.view === "album") || (v === "artists" && route.view === "artist") || (v === "playlists" && route.view === "playlist" && !route.param) || (v === "genres" && route.view === "genre") || (v === "filters" && route.view === "filter")));

  return (
    <nav className="sidebar" aria-label={t("nav.library")} data-testid="sidebar">
      <div className="sidebar-scroll">
        <div className="section-title">{t("nav.library")}</div>
        {items.map((it) => (
          <a key={it.id} href="#" className={`nav-item ${isActive(it.view) ? "active" : ""}`} aria-current={isActive(it.view) ? "page" : undefined} title={it.label} onClick={(e) => { e.preventDefault(); if (it.view) navigate({ view: it.view }); else void executeAction(it.id); }} data-testid={`nav-${it.view ?? it.id}`}>
            <Icon name={it.icon} size={15} />
            <span>{it.label}</span>
          </a>
        ))}
        {filters.length ? <div className="section-title">{t("nav.savedFilters")}</div> : null}
        {filters.map((f) => {
          // "Available offline" opens its track list (Downloads → Available offline), not the builder.
          const offline = f.id === AVAILABLE_OFFLINE_FILTER_ID;
          const active = offline ? route.view === "downloads" && route.param === "offline" : route.view === "filter" && route.id === f.id;
          const name = filterName(f);
          return (
            <a key={f.id} href="#" className={`nav-item ${active ? "active" : ""}`} aria-current={active ? "page" : undefined} title={name} onClick={(e) => { e.preventDefault(); navigate(offline ? { view: "downloads", param: "offline" } : { view: "filter", id: f.id }); }} data-testid={offline ? "nav-available-offline" : undefined}>
              <Icon name={offline ? "cached" : "filter"} size={15} />
              <span>{name}</span>
            </a>
          );
        })}
        {(playlists ?? []).length ? <div className="section-title">{t("nav.playlistsSection")}</div> : null}
        {(playlists ?? []).map((p) => (
          <a key={p.id} href="#" className={`nav-item ${route.view === "playlist" && route.id === p.id ? "active" : ""}`} aria-current={route.view === "playlist" && route.id === p.id ? "page" : undefined} onClick={(e) => { e.preventDefault(); navigate({ view: "playlist", id: p.id }); }} onContextMenu={(e) => void openContextMenu(e, { type: "playlists", data: { ids: [p.id] } })} title={p.isSmart ? `${p.name} · ${t("playlists.smart")}` : p.name}>
            <Icon name={p.isSmart ? "filter" : "playlist"} size={15} />
            <span>{p.name}</span>
          </a>
        ))}
      </div>
      {/* Pinned below the scrolling list, like a native source list's footer. */}
      <div className="sidebar-foot">
        <a href="#" className={`nav-item ${route.view === "settings" ? "active" : ""}`} aria-current={route.view === "settings" ? "page" : undefined} title={t("nav.settings")} onClick={(e) => { e.preventDefault(); navigate({ view: "settings" }); }} data-testid="nav-settings">
          <Icon name="settings" size={15} />
          <span>{t("nav.settings")}</span>
        </a>
      </div>
      <div className={`resize-handle ${dragging ? "dragging" : ""}`} onMouseDown={(e) => { e.preventDefault(); startX.current = e.clientX; startW.current = panels.sidebarWidth; setDragging(true); }} role="separator" aria-orientation="vertical" aria-label={t("a11y.resizeSidebar")} aria-valuenow={panels.sidebarWidth} aria-valuemin={160} aria-valuemax={360} tabIndex={0}
        onKeyDown={(e) => { const d = e.key === "ArrowRight" ? 16 : e.key === "ArrowLeft" ? -16 : 0; if (!d) return; e.preventDefault(); e.stopPropagation(); setPanels({ sidebarWidth: Math.max(160, Math.min(360, panels.sidebarWidth + d)) }); }}
        data-testid="sidebar-resize" />
    </nav>
  );
}
