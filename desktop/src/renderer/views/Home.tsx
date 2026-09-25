import { t } from "@shared/strings";
import type { Album, TrackSummary } from "@core/api";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { EmptyState } from "../components/EmptyState";
import { openContextMenu } from "../components/ContextMenu";
import { Tile, TileList } from "../components/Tile";
import { Icon } from "../components/Icon";
import { usePlayIntent } from "../lib/use-prime";

export function Home() {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const sync = useApp((s) => s.syncProgress);
  const recent = useQuery(() => ({ type: "albums", data: { server_id: serverId, artist_id: undefined, genre: undefined, sort: "dateAdded", descending: true, page: { offset: 0, limit: 12 } } }), "albums", [serverId]);
  const played = useQuery(() => ({ type: "recentlyPlayed", data: { limit: 12 } }), "history", [serverId]);
  const most = useQuery(() => ({ type: "albums", data: { server_id: serverId, artist_id: undefined, genre: undefined, sort: "playCount", descending: true, page: { offset: 0, limit: 12 } } }), "albums", [serverId]);
  const random = useQuery(() => ({ type: "albums", data: { server_id: serverId, artist_id: undefined, genre: undefined, sort: "random", descending: false, page: { offset: 0, limit: 12 } } }), "albums", [serverId], { static: true });
  const playAlbum = (a: Album) => bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "album", data: { id: a.id } }, label: a.name, sort: "default", tracks: [] }, startIndex: 0, shuffle: false, saveOutgoing: true } } });
  const albumIntent = usePlayIntent("home", "album");
  const empty = !recent.loading && !(recent.data?.items.length);
  const albumTile = (a: Album) => (
    <Tile key={a.id} title={a.name} subtitle={a.artist} coverArt={a.coverArt} onOpen={() => navigate({ view: "album", id: a.id })} onPlay={() => playAlbum(a)} playIntent={albumIntent.bind(a.id)} onContextMenu={(e) => void openContextMenu(e, { type: "albums", data: { ids: [a.id] } })} testId="home-album" />
  );
  const trackTile = (tr: TrackSummary, i: number) => (
    <Tile key={`${tr.id}-${i}`} title={tr.title} subtitle={tr.artist} coverArt={tr.coverArt} onActivate={() => bridge().dispatch({ type: "playTracks", data: { server_id: serverId, track_ids: [tr.id], start_index: 0, label: tr.title, shuffle: false } })} onContextMenu={(e) => void openContextMenu(e, { type: "tracks", data: { ids: [tr.id] } })} testId="home-track" />
  );
  return (
    <div className="view" data-testid="view-home">
      <div className="view-header"><h1>{t("home.title")}</h1>{sync && !sync.finished ? <span className="badge">{t("sync.syncing", { phase: sync.phase })}</span> : null}</div>
      <div className="view-body">
        {empty ? <EmptyState message={t("home.empty")} icon="library" /> : null}
        <Section title={t("home.recentlyPlayed")} icon="restore">{(played.data ?? []).map((h, i) => trackTile(h.track, i))}</Section>
        <Section title={t("home.recentlyAdded")} icon="album">{(recent.data?.items ?? []).map(albumTile)}</Section>
        <Section title={t("home.mostPlayed")} icon="stats">{(most.data?.items ?? []).filter((a) => a.playCount > 0).map(albumTile)}</Section>
        <Section title={t("home.random")} icon="shuffle">{(random.data?.items ?? []).map(albumTile)}</Section>
      </div>
    </div>
  );
}

function Section({ title, icon, children }: { title: string; icon: string; children: React.ReactNode[] }) {
  if (!children.length) return null;
  const id = `home-${title.replace(/\W+/g, "-").toLowerCase()}`;
  return (
    <section className="home-section" aria-labelledby={id}>
      <h2 className="section-heading" id={id}><span className="section-icon" aria-hidden="true"><Icon name={icon} size={18} /></span>{title}</h2>
      <TileList label={title} className="scroller">{children}</TileList>
    </section>
  );
}
