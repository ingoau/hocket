import { ErrorBoundary } from "../components/ErrorBoundary";
import { useApp, type Route } from "../store/app";
import { Home } from "./Home";
import { Library } from "./Library";
import { Albums } from "./Albums";
import { Artists } from "./Artists";
import { Playlists } from "./Playlists";
import { Songs } from "./Songs";
import { Genres } from "./Genres";
import { AlbumDetail } from "./AlbumDetail";
import { ArtistDetail } from "./ArtistDetail";
import { PlaylistDetail } from "./PlaylistDetail";
import { GenreDetail } from "./GenreDetail";
import { SearchView } from "./SearchView";
import { Downloads } from "./Downloads";
import { Filters } from "./Filters";
import { FilterBuilder } from "./FilterBuilder";
import { Stats } from "./Stats";
import { Settings } from "./Settings";
import { DeveloperDiagnostics } from "./DeveloperDiagnostics";

/** Detail views: a different id is a different page. */
const DETAIL = new Set<Route["view"]>(["album", "artist", "playlist", "genre"]);

/**
 * The routed view, keyed on the page (not on a tab or section param, so
 * switching tabs keeps focus) so every navigation mounts afresh and plays the
 * enter transition in m3-shell.css (off under reduced motion).
 */
export function Router() {
  const route = useApp((s) => s.route);
  const key = DETAIL.has(route.view) ? `${route.view}:${route.id ?? ""}` : route.view;
  // A page that fails to render shows its error in place; another page starts afresh.
  return <ErrorBoundary key={key} name={`the ${route.view} page`}><RouteView route={route} /></ErrorBoundary>;
}

function RouteView({ route }: { route: Route }) {
  switch (route.view) {
    case "home":
      return <Home />;
    case "library":
      return <Library />;
    case "albums":
      return <Albums />;
    case "artists":
      return <Artists />;
    case "playlists":
      return <Playlists />;
    case "songs":
      return <Songs />;
    case "genres":
      return <Genres />;
    case "album":
      return <AlbumDetail id={route.id ?? ""} />;
    case "artist":
      // Keyed so the next artist's page never shows this one's answers while its own load.
      return <ArtistDetail key={route.id ?? ""} id={route.id ?? ""} />;
    case "playlist":
      return <PlaylistDetail id={route.id ?? ""} />;
    case "genre":
      return <GenreDetail name={route.id ?? ""} />;
    case "search":
      return <SearchView query={route.param ?? ""} />;
    case "downloads":
      return <Downloads tab={route.param} />;
    case "filters":
      return <Filters />;
    case "filter":
      return <FilterBuilder id={route.id ?? "new"} />;
    case "stats":
      return <Stats />;
    case "settings":
      return <Settings section={route.param} />;
    case "developer":
      return <DeveloperDiagnostics />;
  }
}
