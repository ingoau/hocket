// Search results: chips to narrow to one kind (each with its count), then the
// library's matches — the first songs (all of them under Songs), albums,
// artists and playlists — and below them what only the server found.
import { useEffect, useState } from "react";
import type { SearchResults } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { TrackTable } from "../components/TrackTable";
import { Icon } from "../components/Icon";
import { useQuery } from "../store/queries";
import { Tile, TileList } from "../components/Tile";

type Kind = "all" | "songs" | "albums" | "artists" | "playlists";
const KINDS: Kind[] = ["all", "songs", "albums", "artists", "playlists"];
/** Songs shown under All before "See all songs". */
const PREVIEW = 5;

export function SearchView({ query }: { query: string }) {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const serverBatch = useApp((s) => s.lastServerSearch);
  const [reqId] = useState(() => `sv-${Date.now()}`);
  const [kind, setKind] = useState<Kind>("all");
  useEffect(() => setKind("all"), [query]);
  const local = useQuery(() => ({ type: "search", data: { server_id: serverId, query, limit: 100, include_server: true, request_id: reqId } }), "search", [query, serverId], { static: true });
  const [server, setServer] = useState<SearchResults | undefined>(undefined);
  useEffect(() => { if (serverBatch?.requestId === reqId) setServer(serverBatch); }, [serverBatch, reqId]);
  const found = local.data;
  const ids = [...(found?.tracks ?? []), ...(server?.tracks ?? [])].map((x) => x.id);
  const tracks = useQuery(() => (ids.length ? { type: "tracksByIds", data: { ids } } : null), "trackList", [ids.join(",")], { static: true });
  const songs = tracks.data ?? [];
  const albums = [...(found?.albums ?? []), ...(server?.albums ?? [])];
  const artists = [...(found?.artists ?? []), ...(server?.artists ?? [])];
  const playlists = [...(found?.playlists ?? []), ...(server?.playlists ?? [])];
  const counts: Record<Kind, number> = { all: songs.length + albums.length + artists.length + playlists.length, songs: songs.length, albums: albums.length, artists: artists.length, playlists: playlists.length };
  const show = (k: Kind) => (kind === "all" || kind === k) && counts[k] > 0;
  const shownSongs = kind === "songs" ? songs : songs.slice(0, PREVIEW);
  const play = (i: number) => bridge().dispatch({ type: "playTracks", data: { server_id: serverId, track_ids: songs.map((x) => x.id), start_index: i, label: query, shuffle: false } });
  const nothing = !!found && counts.all === 0 && !!server;
  return (
    <div className="view page-search" data-testid="view-search">
      <div className="view-header"><h1>{query}</h1><span className="muted count-chip">{counts.all}</span></div>
      <div className="search-kinds" role="group" aria-label={t("search.kinds")} data-testid="search-kinds">
        {KINDS.map((k) => (
          <button key={k} type="button" className={`search-kind ${kind === k ? "on" : ""}`} aria-pressed={kind === k} disabled={k !== "all" && counts[k] === 0} onClick={() => setKind(k)} data-testid={`search-kind-${k}`}>
            {kind === k ? <Icon name="check" size={16} /> : null}{t(`search.kind.${k}`)}{k !== "all" ? <span className="search-kind-count">{counts[k]}</span> : null}
          </button>
        ))}
      </div>
      <div className="view-body" style={{ display: "flex", flexDirection: "column", gap: 20 }}>
        {nothing ? <p className="muted search-nothing" data-testid="search-nothing">{t("search.noResults", { query })}</p> : null}
        {show("songs") ? (
          <section className="search-songs" aria-labelledby="search-tracks" style={kind === "songs" ? { flex: 1, minHeight: 300, display: "flex", flexDirection: "column" } : { display: "flex", flexDirection: "column", height: `calc(${shownSongs.length} * var(--row-h) + 64px)` }}>
            <div className="search-section-head">
              <h2 className="section-heading" id="search-tracks">{t("search.tracks")}</h2>
              {kind === "all" && songs.length > PREVIEW ? <button type="button" className="btn text sm" onClick={() => setKind("songs")} data-testid="search-see-all-songs">{t("search.seeAllSongs", { count: songs.length })}</button> : null}
            </div>
            <TrackTable tracks={shownSongs} total={shownSongs.length} columns={["art", "title", "artist", "album", "rating", "duration"]} scope={`search:${query}`} label={t("search.tracks")} onPlay={(i) => play(i)} emptyMessage={t("search.noResults", { query })} />
          </section>
        ) : null}
        {show("albums") ? (
          <section className="home-section" aria-labelledby="search-albums">
            <h2 className="section-heading" id="search-albums">{t("search.albums")}</h2>
            <TileList label={t("search.albums")} className={kind === "albums" ? "grid" : "scroller"}>{albums.map((a) => (
              <Tile key={a.id} title={a.name} subtitle={a.artist} coverArt={a.coverArt} onOpen={() => navigate({ view: "album", id: a.id })} dragTarget={{ type: "albums", data: { ids: [a.id] } }} />
            ))}</TileList>
          </section>
        ) : null}
        {show("artists") ? (
          <section className="home-section" aria-labelledby="search-artists">
            <h2 className="section-heading" id="search-artists">{t("search.artists")}</h2>
            <TileList label={t("search.artists")} className={kind === "artists" ? "grid" : "scroller"}>{artists.map((a) => (
              <Tile key={a.id} title={a.name} subtitle={t("search.albumCount", { count: a.albumCount })} coverArt={a.coverArt} round onOpen={() => navigate({ view: "artist", id: a.id })} />
            ))}</TileList>
          </section>
        ) : null}
        {show("playlists") ? (
          <section className="home-section" aria-labelledby="search-playlists">
            <h2 className="section-heading" id="search-playlists">{t("search.playlists")}</h2>
            <TileList label={t("search.playlists")} className={kind === "playlists" ? "grid" : "scroller"}>{playlists.map((p) => (
              <Tile key={p.id} title={p.name} subtitle={t("search.songCount", { count: p.songCount })} coverArt={p.coverArt} onOpen={() => navigate({ view: "playlist", id: p.id })} />
            ))}</TileList>
          </section>
        ) : null}
        {found && !server ? <p className="small muted search-asking" data-testid="search-asking">{t("search.searching")}</p> : null}
      </div>
    </div>
  );
}
