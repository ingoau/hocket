import { useEffect, useRef, useState } from "react";
import { t } from "@shared/strings";
import { useApp, type ViewName } from "../store/app";
import { useActions, useQuery } from "../store/queries";
import { openContextMenu } from "./ContextMenu";
import { Icon } from "./Icon";

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

  const items = navActions.length ? navActions.map((a) => ({ id: a.id.replace("nav.", "") as ViewName, label: a.label, icon: a.icon })) : (["home", "albums", "artists", "playlists", "songs", "genres", "downloads", "filters", "stats"] as ViewName[]).map((v) => ({ id: v, label: t(`nav.${v}` as never), icon: v === "home" ? "home" : v === "albums" ? "album" : v === "artists" ? "artist" : v === "playlists" ? "playlist" : v === "songs" ? "song" : v === "genres" ? "genre" : v === "downloads" ? "download" : v === "filters" ? "filter" : "stats" }));
  const isActive = (v: ViewName) => route.view === v || (v === "albums" && route.view === "album") || (v === "artists" && route.view === "artist") || (v === "playlists" && route.view === "playlist" && !route.param) || (v === "genres" && route.view === "genre") || (v === "filters" && route.view === "filter");

  return (
    <nav className="sidebar" aria-label={t("nav.library")} style={{ width: panels.sidebarWidth }} data-testid="sidebar">
      <div className="sidebar-scroll">
        <div className="section-title">{t("nav.library")}</div>
        {items.map((it) => (
          <a key={it.id} href="#" className={`nav-item ${isActive(it.id) ? "active" : ""}`} aria-current={isActive(it.id) ? "page" : undefined} onClick={(e) => { e.preventDefault(); navigate({ view: it.id }); }} data-testid={`nav-${it.id}`}>
            <Icon name={it.icon} size={15} />
            <span>{it.label}</span>
          </a>
        ))}
        {filters.length ? <div className="section-title">{t("nav.savedFilters")}</div> : null}
        {filters.map((f) => (
          <a key={f.id} href="#" className={`nav-item ${route.view === "filter" && route.id === f.id ? "active" : ""}`} onClick={(e) => { e.preventDefault(); navigate({ view: "filter", id: f.id }); }}>
            <Icon name="filter" size={15} />
            <span>{f.name}</span>
          </a>
        ))}
        {(playlists ?? []).length ? <div className="section-title">{t("nav.playlistsSection")}</div> : null}
        {(playlists ?? []).map((p) => (
          <a key={p.id} href="#" className={`nav-item ${route.view === "playlist" && route.id === p.id ? "active" : ""}`} onClick={(e) => { e.preventDefault(); navigate({ view: "playlist", id: p.id }); }} onContextMenu={(e) => void openContextMenu(e, { type: "playlists", data: { ids: [p.id] } })} title={p.isSmart ? t("playlists.smart") : undefined}>
            <Icon name={p.isSmart ? "filter" : "playlist"} size={15} />
            <span>{p.name}</span>
          </a>
        ))}
        <div className="divider" />
        <a href="#" className={`nav-item ${route.view === "settings" ? "active" : ""}`} onClick={(e) => { e.preventDefault(); navigate({ view: "settings" }); }} data-testid="nav-settings">
          <Icon name="settings" size={15} />
          <span>{t("nav.settings")}</span>
        </a>
      </div>
      <div className={`resize-handle ${dragging ? "dragging" : ""}`} onMouseDown={(e) => { startX.current = e.clientX; startW.current = panels.sidebarWidth; setDragging(true); }} role="separator" aria-orientation="vertical" aria-label="Resize sidebar" />
    </nav>
  );
}
