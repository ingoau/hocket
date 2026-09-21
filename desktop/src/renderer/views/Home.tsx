import { t } from "@shared/strings";
import type { Album, TrackSummary } from "@core/api";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { Artwork } from "../components/Artwork";
import { EmptyState } from "../components/EmptyState";
import { openContextMenu } from "../components/ContextMenu";
import { Icon } from "../components/Icon";

export function Home() {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const sync = useApp((s) => s.syncProgress);
  const recent = useQuery(() => ({ type: "albums", data: { server_id: serverId, artist_id: undefined, genre: undefined, sort: "dateAdded", descending: true, page: { offset: 0, limit: 12 } } }), "albums", [serverId]);
  const played = useQuery(() => ({ type: "recentlyPlayed", data: { limit: 12 } }), "history", [serverId]);
  const most = useQuery(() => ({ type: "albums", data: { server_id: serverId, artist_id: undefined, genre: undefined, sort: "playCount", descending: true, page: { offset: 0, limit: 12 } } }), "albums", [serverId]);
  const random = useQuery(() => ({ type: "albums", data: { server_id: serverId, artist_id: undefined, genre: undefined, sort: "random", descending: false, page: { offset: 0, limit: 12 } } }), "albums", [serverId], { static: true });
  const playAlbum = (a: Album) => bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "album", data: { id: a.id } }, label: a.name, sort: "default", tracks: [] }, startIndex: 0, shuffle: false, saveOutgoing: true } } });
  const empty = !recent.loading && !(recent.data?.items.length);
  const albumTile = (a: Album) => (
    <div key={a.id} className="tile" role="button" tabIndex={0} onDoubleClick={() => navigate({ view: "album", id: a.id })} onClick={() => navigate({ view: "album", id: a.id })} onKeyDown={(e) => e.key === "Enter" && navigate({ view: "album", id: a.id })} onContextMenu={(e) => void openContextMenu(e, { type: "albums", data: { ids: [a.id] } })} data-testid="home-album">
      <Artwork id={a.coverArt} size={300} />
      <div className="t1">{a.name}</div>
      <div className="t2">{a.artist}</div>
      <button type="button" className="play" aria-label={t("album.play")} onClick={(e) => { e.stopPropagation(); playAlbum(a); }}><Icon name="play" size={16} style={{ fill: "currentColor" }} /></button>
    </div>
  );
  const trackTile = (tr: TrackSummary, i: number) => (
    <div key={`${tr.id}-${i}`} className="tile" role="button" tabIndex={0} onDoubleClick={() => bridge().dispatch({ type: "playTracks", data: { server_id: serverId, track_ids: [tr.id], start_index: 0, label: tr.title, shuffle: false } })} onContextMenu={(e) => void openContextMenu(e, { type: "tracks", data: { ids: [tr.id] } })}>
      <Artwork id={tr.coverArt} size={300} />
      <div className="t1">{tr.title}</div>
      <div className="t2">{tr.artist}</div>
    </div>
  );
  return (
    <div className="view" data-testid="view-home">
      <div className="view-header"><h1>{t("home.title")}</h1>{sync && !sync.finished ? <span className="badge">{t("sync.syncing", { phase: sync.phase })}</span> : null}</div>
      <div className="view-body">
        {empty ? <EmptyState message={t("home.empty")} /> : null}
        <Section title={t("home.recentlyPlayed")}>{(played.data ?? []).map((h, i) => trackTile(h.track, i))}</Section>
        <Section title={t("home.recentlyAdded")}>{(recent.data?.items ?? []).map(albumTile)}</Section>
        <Section title={t("home.mostPlayed")}>{(most.data?.items ?? []).filter((a) => a.playCount > 0).map(albumTile)}</Section>
        <Section title={t("home.random")}>{(random.data?.items ?? []).map(albumTile)}</Section>
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode[] }) {
  if (!children.length) return null;
  return (
    <section className="home-section">
      <h3>{title}</h3>
      <div className="scroller">{children}</div>
    </section>
  );
}
