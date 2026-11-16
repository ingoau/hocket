import type { Route, ViewName } from "../store/app";

/** Views that belong to the library: Library lights up on them when no sidebar item of their own is shown. */
const LIBRARY_VIEWS: ViewName[] = ["library", "albums", "artists", "playlists", "songs", "genres", "filters", "downloads", "stats", "snoozed", "album", "artist", "playlist", "genre", "filter"];

/** Whether the sidebar item for [view] is the current page's, leaving Library's fallback aside. */
function ownActive(view: ViewName, route: Route): boolean {
  if (route.view === view) return !(view === "downloads" && route.param === "offline");
  return (view === "albums" && route.view === "album")
    || (view === "artists" && route.view === "artist")
    || (view === "playlists" && route.view === "playlist" && !route.param)
    || (view === "genres" && route.view === "genre")
    || (view === "filters" && route.view === "filter");
}

/**
 * Whether the sidebar item for [view] is highlighted. Only one item ever is: Library stands in for library
 * pages with no item of their own, but not when another item already claims the page. An open pin ([pinnedOpen])
 * is the page's most specific item, so it claims the page from all of them.
 */
export function isSidebarItemActive(view: ViewName | undefined, route: Route, itemViews: (ViewName | undefined)[], pinnedOpen: boolean): boolean {
  if (!view || pinnedOpen) return false;
  if (view !== "library") return ownActive(view, route);
  if (route.view === "library") return true;
  return LIBRARY_VIEWS.includes(route.view)
    && !itemViews.some((v) => v === route.view || (!!v && v !== "library" && ownActive(v, route)));
}
