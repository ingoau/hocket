import { useEffect, useMemo, useRef, useState } from "react";
import { t } from "@shared/strings";
import { NAV_VIEWS, sidebarPlaces } from "@shared/keymap";
import { executeAction } from "../store/actions";
import { SIDEBAR_MAX, SIDEBAR_MIN, useApp, type ViewName } from "../store/app";
import { useActions } from "../store/queries";
import { openContextMenu } from "./ContextMenu";
import { Icon } from "./Icon";
import { openItemsMenu } from "./ContextMenu";
import { Artwork } from "./Artwork";
import { useResolvedPins, performPin, type ResolvedPin } from "../lib/use-pins";
import { editPinsItem, pinMenuItems } from "../lib/pin-menus";
import { pinTap } from "../lib/library-pins";
import { isSidebarItemActive } from "../lib/sidebar-active";
import { NARROW, useMediaQuery } from "../lib/media";
import { SidebarLibrary, useLibraryEntries } from "./SidebarLibrary";

const clampWidth = (w: number) => Math.max(SIDEBAR_MIN, Math.min(SIDEBAR_MAX, w));

// The sidebar, one scrolling column: the places (the customisable sidebar actions), the pins, then
// Your library (SidebarLibrary). Settings stays at the foot. Collapsed (the top bar's first button,
// Mod+B), or in a narrow window, it is a rail: icons and covers, the names as tooltips.
export function Sidebar() {
  const route = useApp((s) => s.route);
  const navigate = useApp((s) => s.navigate);
  const pins = useResolvedPins();
  const narrow = useMediaQuery(NARROW);
  const panels = useApp((s) => s.panels);
  const setPanels = useApp((s) => s.setPanels);
  const navActions = useActions("sidebar", { type: "none" });
  const [dragging, setDragging] = useState(false);
  const startX = useRef(0);
  const startW = useRef(0);

  useEffect(() => {
    if (!dragging) return;
    const move = (e: MouseEvent) => setPanels({ sidebarWidth: clampWidth(startW.current + e.clientX - startX.current) });
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
  const rail = narrow || panels.sidebarCollapsed;
  // What Your library leaves out: the pins straight from the setting, so nothing doubles up while they resolve.
  const pinnedKeys = useMemo(() => new Set((pins ?? []).map((rp) => `${rp.pin.kind}:${rp.pin.id}`)), [pins]);
  const library = useLibraryEntries();
  // A pin or a Your library item that is open is the highlight; its list page's place isn't as well.
  const libraryOpen = !!route.id && !pinnedKeys.has(`${route.view}:${route.id}`) && library.some((e) => e.kind === route.view && e.id === route.id);
  const pinnedOpen = (pins ?? []).some((rp) => isPinActive(rp)) || libraryOpen;
  const itemViews = items.map((it) => it.view);
  const isActive = (v: ViewName | undefined) => isSidebarItemActive(v, route, itemViews, pinnedOpen);
  function isPinActive(rp: ResolvedPin): boolean {
    const view = rp.kind === "track" ? undefined : rp.kind;
    return !!view && route.view === view && route.id === rp.pin.id;
  }

  return (
    <nav className={`sidebar ${rail ? "rail" : ""}`} aria-label={t("nav.library")} data-testid="sidebar" data-rail={rail || undefined}>
      <div className="sidebar-scroll" onContextMenu={(e) => { if (!e.defaultPrevented && pins?.length && (e.target === e.currentTarget || (e.target as Element).closest(".sidebar-pins"))) openItemsMenu(e, [editPinsItem()]); }}>
        <div className="sidebar-places">
          {items.map((it) => (
            <a key={it.id} href="#" className={`nav-item ${isActive(it.view) ? "active" : ""}`} aria-current={isActive(it.view) ? "page" : undefined} title={it.label} onClick={(e) => { e.preventDefault(); if (it.view) navigate({ view: it.view }); else void executeAction(it.id); }} data-testid={`nav-${it.view ?? it.id}`}>
              <Icon name={it.icon} size={18} filled={isActive(it.view)} />
              <span>{it.label}</span>
            </a>
          ))}
        </div>
        {/* No visible "Pinned" title (the pins' thumbnails say it); a gap, and the name for screen readers.
            Right-clicking around and below the pins (not on one: that has its own menu) offers Edit pins. */}
        <div className="sidebar-pins" data-testid="sidebar-pins">
          <div className="sidebar-pins-gap" role="presentation" />
          <div className="sr-only">{t("library.pinned")}</div>
          {pins && !pins.length && !rail ? <p className="sidebar-pins-empty small muted">{t("library.pinsEmpty")}</p> : null}
          {(pins ?? []).map((rp) => {
            const active = isPinActive(rp);
            // A song always plays; the rest do what their pin says.
            const tap = rp.kind === "track" ? "play" : pinTap(rp.pin);
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
                  const items = pinMenuItems(rp.pin, rp.title, !rp.target, (pins ?? []).map((x) => x.pin));
                  if (rp.target) void openContextMenu(e, rp.target, undefined, items);
                  else openItemsMenu(e, items);
                }}
                data-testid="sidebar-pin"
                data-pin={`${rp.pin.kind}:${rp.pin.id}`}
              >
                <PinThumb rp={rp} />
                <span className="pin-text"><span className="pin-title">{rp.title}</span><span className="pin-kind">{t(`library.kind.${rp.kind}` as never)}</span></span>
                {tap !== "open" && rp.kind !== "track" ? <Icon name={tap} size={14} /> : null}
              </a>
            );
          })}
        </div>
        <SidebarLibrary rail={rail} pinned={pinnedKeys} entries={library} />
      </div>
      {/* Pinned below the scrolling list, like a native source list's footer. */}
      <div className="sidebar-foot">
        <a href="#" className={`nav-item ${route.view === "settings" ? "active" : ""}`} aria-current={route.view === "settings" ? "page" : undefined} title={t("nav.settings")} onClick={(e) => { e.preventDefault(); navigate({ view: "settings" }); }} data-testid="nav-settings">
          <Icon name="settings" size={18} filled={route.view === "settings"} />
          <span>{t("nav.settings")}</span>
        </a>
      </div>
      {rail ? null : <div className={`resize-handle ${dragging ? "dragging" : ""}`} onMouseDown={(e) => { e.preventDefault(); startX.current = e.clientX; startW.current = panels.sidebarWidth; setDragging(true); }} role="separator" aria-orientation="vertical" aria-label={t("a11y.resizeSidebar")} aria-valuenow={panels.sidebarWidth} aria-valuemin={SIDEBAR_MIN} aria-valuemax={SIDEBAR_MAX} tabIndex={0}
        onKeyDown={(e) => { const d = e.key === "ArrowRight" ? 16 : e.key === "ArrowLeft" ? -16 : 0; if (!d) return; e.preventDefault(); e.stopPropagation(); setPanels({ sidebarWidth: clampWidth(panels.sidebarWidth + d) }); }}
        data-testid="sidebar-resize" />}
    </nav>
  );
}

/** A pin's artwork in its kind's shape; genres and filters (no cover) draw their icon. [large]: Edit pins' tiles. */
export function PinThumb({ rp, large = false }: { rp: ResolvedPin; large?: boolean }) {
  const cls = `pin-thumb ${large ? "lg" : ""}`;
  if (rp.kind === "genre" || rp.kind === "filter") return <span className={`${cls} icon-thumb ${rp.kind}`} aria-hidden="true"><Icon name={rp.kind} size={large ? 28 : 16} /></span>;
  return <span className={`${cls} ${rp.kind}`} aria-hidden="true"><Artwork id={rp.coverArt} size={large ? 128 : 64} round={rp.kind === "artist"} /></span>;
}
