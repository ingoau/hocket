// Downloads: pinned albums/playlists/songs, and "Available offline" — every
// song that plays with no network (downloads plus complete stream-cache
// entries; the core's AvailableOffline filter field).
import { useCallback } from "react";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { Artwork } from "../components/Artwork";
import { EmptyState } from "../components/EmptyState";
import { Icon } from "../components/Icon";
import { Tabs, tabPanelProps } from "../components/Tabs";
import { TrackTable, usePagedTracks } from "../components/TrackTable";
import { fmtBytes } from "../lib/format";
import { AVAILABLE_OFFLINE_FILTER, AVAILABLE_OFFLINE_NODE } from "../lib/filters";
import { AVAILABLE_OFFLINE_FILTER_ID } from "@shared/constants";

type Tab = "pins" | "offline";

export function Downloads({ tab }: { tab?: string }) {
  const storage = useApp((s) => s.storage);
  const navigate = useApp((s) => s.navigate);
  const current: Tab = tab === "offline" ? "offline" : "pins";
  const st = useQuery(() => ({ type: "storage" }), "storage", [], { static: true });
  const sum = storage ?? st.data;
  return (
    <div className="view" data-testid="view-downloads">
      <div className="view-header">
        <h1>{t("downloads.title")}</h1>
        <div className="actions">
          {sum ? <span className="muted small">{t("downloads.usage", { downloads: fmtBytes(sum.downloadsBytes), cache: fmtBytes(sum.cacheBytes), images: fmtBytes(sum.imagesBytes) })}</span> : null}
          <button type="button" className="btn" onClick={() => bridge().dispatch({ type: "clearStreamCache" })}>{t("downloads.clearCache")}</button>
        </div>
      </div>
      <div className="view-tabs-row">
        <Tabs id="downloads-tabs" className="tabs view-tabs" label={t("downloads.tabs")} selected={current} onSelect={(k) => navigate({ view: "downloads", param: k === "offline" ? "offline" : undefined }, true)}
          tabs={[{ key: "pins", label: t("downloads.tab.pins"), testId: "downloads-tab-pins" }, { key: "offline", label: t("downloads.tab.offline"), testId: "downloads-tab-offline" }]} />
      </div>
      {current === "pins" ? <Pins /> : <AvailableOffline />}
    </div>
  );
}

function Pins() {
  const pins = useApp((s) => s.pins);
  const navigate = useApp((s) => s.navigate);
  const openDialog = useApp((s) => s.openDialog);
  const fetched = useQuery(() => ({ type: "pins" }), "pins", [], { static: true });
  const list = pins.length ? pins : (fetched.data ?? []);
  const remove = (p: (typeof list)[number]) => openDialog({ kind: "confirm", title: t("downloads.remove"), message: t("dialog.deleteDownload", { name: p.label }), confirmLabel: t("dialog.delete"), destructive: true, onConfirm: () => bridge().dispatch({ type: "unpin", data: { target: p.target } }) });
  const open = (p: (typeof list)[number]) => {
    if (p.target.type === "album") navigate({ view: "album", id: p.target.data.id });
    else if (p.target.type === "playlist") navigate({ view: "playlist", id: p.target.data.id });
  };
  return (
    <div className="view-body" {...tabPanelProps("downloads-tabs", "pins")}>
      {!list.length ? <EmptyState message={t("downloads.empty")} action={<button type="button" className="btn" onClick={() => navigate({ view: "albums" })}>{t("nav.albums")}</button>} /> : null}
      {list.map((p) => (
        <div key={JSON.stringify(p.target)} className="pin-row" data-testid="pin-row">
          <Artwork id={p.coverArt} size={64} className="art" />
          <div style={{ minWidth: 0 }}>
            <button type="button" className="link-button truncate" onClick={() => open(p)}>{p.label}</button>
            <div className="small muted">{t("downloads.progress", { done: p.downloadedCount, total: p.trackCount })} · {fmtBytes(p.bytes)}{p.transcoded ? ` · ${t("downloads.transcoded")}` : ""}</div>
            {p.downloadedCount < p.trackCount ? <div className="progress" style={{ marginTop: 4 }}><div style={{ width: `${(p.downloadedCount / Math.max(1, p.trackCount)) * 100}%` }} /></div> : null}
          </div>
          <span className="badge">{p.target.type}</span>
          <button type="button" className="btn icon" aria-label={`${t("downloads.remove")}: ${p.label}`} title={t("downloads.remove")} onClick={() => remove(p)}><Icon name="trash" size={14} /></button>
        </div>
      ))}
    </div>
  );
}

function AvailableOffline() {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const playing = useApp((s) => s.nowPlaying?.track.id);
  const filter = useApp((s) => s.filters.find((f) => f.id === AVAILABLE_OFFLINE_FILTER_ID)) ?? AVAILABLE_OFFLINE_FILTER;
  const fetchPage = useCallback(async (offset: number, limit: number) => {
    const r = await bridge().query({ type: "tracks", data: { server_id: serverId, filter: AVAILABLE_OFFLINE_NODE, sort: filter.sort, descending: filter.descending, page: { offset, limit } } });
    if (r.type !== "tracks") throw new Error("bad result");
    return { items: r.data.items, total: r.data.total };
  }, [serverId, filter.sort, filter.descending]);
  const { rows, total, onNeedRange } = usePagedTracks(fetchPage, [serverId, "offline", filter.sort, filter.descending]);
  const play = (startIndex: number) => bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "filter", data: { filter } }, label: t("downloads.tab.offline"), sort: filter.sort, tracks: [] }, startIndex, shuffle: false, saveOutgoing: true } } });
  return (
    <div className="view-body no-pad" style={{ display: "flex", flexDirection: "column", overflow: "hidden", padding: "0 8px" }} {...tabPanelProps("downloads-tabs", "offline")} data-testid="available-offline">
      <div className="row offline-head">
        <span className="muted small grow">{t("downloads.offlineHint")}</span>
        <span className="muted small" data-testid="available-offline-count">{t("downloads.offlineCount", { count: total })}</span>
        <button type="button" className="btn sm" disabled={!total} onClick={() => play(0)}><Icon name="play" size={12} style={{ fill: "currentColor" }} /> {t("downloads.playOffline")}</button>
      </div>
      <TrackTable tracks={rows} total={total} columns={["art", "title", "artist", "album", "duration", "offline"]} scope="available-offline" label={t("downloads.tab.offline")} onNeedRange={onNeedRange} onPlay={(i) => play(i)} playingTrackId={playing} emptyMessage={t("downloads.offlineEmpty")} testId="available-offline-table" />
    </div>
  );
}
