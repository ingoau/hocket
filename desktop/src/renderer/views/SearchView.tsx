import { useEffect, useState } from "react";
import type { SearchResults } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { TrackTable } from "../components/TrackTable";
import { useQuery } from "../store/queries";
import { Tile, TileList } from "../components/Tile";

export function SearchView({ query }: { query: string }) {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const serverBatch = useApp((s) => s.lastServerSearch);
  const [reqId] = useState(() => `sv-${Date.now()}`);
  const local = useQuery(() => ({ type: "search", data: { server_id: serverId, query, limit: 100, include_server: true, request_id: reqId } }), "search", [query, serverId], { static: true });
  const [server, setServer] = useState<SearchResults | undefined>(undefined);
  useEffect(() => { if (serverBatch?.requestId === reqId) setServer(serverBatch); }, [serverBatch, reqId]);
  const ids = [...(local.data?.tracks ?? []), ...(server?.tracks ?? [])].map((x) => x.id);
  const tracks = useQuery(() => (ids.length ? { type: "tracksByIds", data: { ids } } : null), "trackList", [ids.join(",")], { static: true });
  const list = tracks.data ?? [];
  return (
    <div className="view" data-testid="view-search">
      <div className="view-header"><h1>{query}</h1><span className="muted">{list.length}</span></div>
      <div className="view-body" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
        {(local.data?.albums.length || server?.albums.length) ? (
          <section className="home-section" aria-labelledby="search-albums">
            <h2 className="section-heading" id="search-albums">{t("search.albums")}</h2>
            <TileList label={t("search.albums")} className="scroller">{[...(local.data?.albums ?? []), ...(server?.albums ?? [])].map((a) => (
              <Tile key={a.id} title={a.name} subtitle={a.artist} coverArt={a.coverArt} onOpen={() => navigate({ view: "album", id: a.id })} />
            ))}</TileList>
          </section>
        ) : null}
        <div style={{ flex: 1, minHeight: 300, display: "flex", flexDirection: "column" }}>
          <TrackTable tracks={list} total={list.length} columns={["art", "title", "artist", "album", "rating", "duration"]} scope={`search:${query}`} label={t("search.tracks")} onPlay={(i) => bridge().dispatch({ type: "playTracks", data: { server_id: serverId, track_ids: list.map((x) => x.id), start_index: i, label: query, shuffle: false } })} emptyMessage={t("search.noResults", { query })} />
        </div>
      </div>
    </div>
  );
}
