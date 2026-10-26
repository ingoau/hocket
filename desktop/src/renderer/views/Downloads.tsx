// Downloads: pinned albums/playlists/songs, and "Available offline" — every
// song that plays with no network (downloads plus complete stream-cache
// entries; the core's AvailableOffline filter field).
import { useCallback } from "react";
import { t } from "@shared/strings";
import { useApp, useSetting } from "../store/app";
import { SK } from "@shared/settings-keys";
import { Select } from "../components/controls";
import { GIB, warnChoices } from "../lib/storage-warn";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { Artwork } from "../components/Artwork";
import { EmptyState } from "../components/EmptyState";
import { Icon } from "../components/Icon";
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
  const summary = sum ? [t("downloads.sumDownloads", { size: fmtBytes(sum.downloadsBytes) }), t("downloads.sumCache", { size: fmtBytes(sum.cacheBytes) }), sum.freeBytes !== undefined ? t("downloads.sumFree", { size: fmtBytes(sum.freeBytes) }) : ""].filter(Boolean).join(" · ") : "";
  // Available offline opens from the storage card as a page of its own, with a way back.
  return (
    <div className="view page-downloads" data-testid="view-downloads">
      <div className="view-header dl-header">
        {current === "offline" ? <button type="button" className="btn icon dl-back" aria-label={t("downloads.title")} title={t("downloads.title")} onClick={() => navigate({ view: "downloads" }, true)} data-testid="downloads-back"><Icon name="back" size={22} /></button> : null}
        <h1 className="brand-type">{current === "offline" ? t("downloads.tab.offline") : t("downloads.title")}</h1>
        {summary ? <p className="dl-summary" data-testid="downloads-summary">{current === "offline" ? t("downloads.offlineHint") : summary}</p> : null}
      </div>
      {current === "pins" ? <Pins /> : <AvailableOffline />}
    </div>
  );
}

/** Downloads as a table (what, how far along, size, files), with the device's storage beside it. */
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
    <div className="view-body downloads-body">
      <div className="downloads-main">
        {!list.length ? <EmptyState message={t("downloads.empty")} icon="download" action={<button type="button" className="btn" onClick={() => navigate({ view: "albums" })}>{t("nav.albums")}</button>} /> : (
          <section aria-labelledby="dl-count">
          <h2 className="dl-count" id="dl-count">{t(list.length === 1 ? "downloads.countOne" : "downloads.count", { count: list.length })}</h2>
          <table className="dl-table" data-testid="downloads-table">
            <thead><tr><th scope="col">{t("downloads.col.title")}</th><th scope="col">{t("downloads.col.downloaded")}</th><th scope="col" className="num">{t("downloads.col.size")}</th><th scope="col">{t("downloads.col.files")}</th><th scope="col"><span className="sr-only">{t("downloads.remove")}</span></th></tr></thead>
            <tbody>
              {list.map((p) => {
                const done = p.downloadedCount >= p.trackCount;
                return (
                  <tr key={JSON.stringify(p.target)} data-testid="pin-row">
                    <td>
                      <div className="dl-title">
                        <Artwork id={p.coverArt} size={96} className="art" />
                        <div style={{ minWidth: 0 }}>
                          <button type="button" className="link-button truncate" onClick={() => open(p)}>{p.label}</button>
                          <div className="small muted">{t(`downloads.kind.${p.target.type}`)}</div>
                        </div>
                      </div>
                    </td>
                    <td>
                      {done ? <span className="dl-of">{t("downloads.of", { done: p.downloadedCount, total: p.trackCount })}</span> : (
                        <div className="dl-progress"><span className="dl-of active">{t("downloads.of", { done: p.downloadedCount, total: p.trackCount })}</span><div className="dl-meter" role="progressbar" aria-label={p.label} aria-valuemin={0} aria-valuemax={p.trackCount} aria-valuenow={p.downloadedCount}><div style={{ width: `${(p.downloadedCount / Math.max(1, p.trackCount)) * 100}%` }} /></div></div>
                      )}
                    </td>
                    <td className="num">{fmtBytes(p.bytes)}</td>
                    <td><span className="dl-files">{p.transcoded ? t("downloads.transcoded") : t("downloads.original")}</span></td>
                    <td className="num"><button type="button" className="btn icon sm dl-remove" aria-label={`${t("downloads.remove")}: ${p.label}`} title={t("downloads.remove")} onClick={() => remove(p)}><Icon name="trash" size={18} /></button></td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          </section>
        )}
      </div>
      <StorageCard />
    </div>
  );
}

function StorageCard() {
  const storage = useApp((s) => s.storage);
  const navigate = useApp((s) => s.navigate);
  const st = useQuery(() => ({ type: "storage" }), "storage", [], { static: true });
  const sum = storage ?? st.data;
  const warn = useSetting<number | null>(SK.storageWarnThresholdBytes, null);
  if (!sum) return null;
  const free = sum.freeBytes ?? 0;
  const whole = Math.max(1, sum.downloadsBytes + sum.cacheBytes + free);
  const pct = (n: number) => `${(n / whole) * 100}%`;
  return (
    <aside className="dl-side" aria-label={t("downloads.storage")} data-testid="downloads-storage">
      <section className="dl-storage" aria-labelledby="dl-storage-title">
        <h2 id="dl-storage-title">{t("downloads.storage")}</h2>
        <div className="dl-bar" aria-hidden="true"><span className="dl-bar-dl" style={{ width: pct(sum.downloadsBytes) }} /><span className="dl-bar-cache" style={{ width: pct(sum.cacheBytes) }} /></div>
        <dl className="dl-legend">
          <div><dt><span className="dot dl" />{t("downloads.tab.pins")}</dt><dd>{fmtBytes(sum.downloadsBytes)}</dd></div>
          <div><dt><span className="dot cache" />{t("downloads.cache")}</dt><dd>{fmtBytes(sum.cacheBytes)}</dd></div>
          {sum.freeBytes !== undefined ? <div><dt><span className="dot free" />{t("downloads.free")}</dt><dd>{fmtBytes(free)}</dd></div> : null}
        </dl>
      </section>
      <button type="button" className="dl-link" onClick={() => navigate({ view: "downloads", param: "offline" }, true)} data-testid="downloads-open-offline">
        <Icon name="offline" size={22} />
        <span className="grow"><span className="dl-link-title">{t("downloads.tab.offline")}</span><span className="dl-link-sub">{t("downloads.offlineShort")}</span></span>
        <Icon name="chevronRight" size={20} />
      </button>
      <div className="dl-actions">
        <button type="button" className="dl-action" onClick={() => bridge().dispatch({ type: "clearStreamCache" })} data-testid="downloads-clear-cache"><Icon name="trash" size={20} /><span className="grow">{t("downloads.clearCache")}</span><span className="muted">{fmtBytes(sum.cacheBytes)}</span></button>
        <label className="dl-action dl-warn">
          <Icon name="warn" size={20} />
          <span className="grow">{t("downloads.warnAbove")}</span>
          <Select value={warn ? Math.round(warn / GIB) : 0} aria-label={t("settings.storageWarn")} onChange={(e) => bridge().dispatch({ type: "setStorageWarnThreshold", data: { bytes: Number(e.target.value) ? Number(e.target.value) * GIB : undefined } })}>
            <option value={0}>–</option>
            {warnChoices(warn ? Math.round(warn / GIB) : 0).map((g) => <option key={g} value={g}>{g} GB</option>)}
          </Select>
        </label>
      </div>
    </aside>
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
    <div className="view-body no-pad" style={{ display: "flex", flexDirection: "column", overflow: "hidden" }} data-testid="available-offline">
      <div className="row offline-head">
        <span className="muted small grow">{t("downloads.offlineHint")}</span>
        <span className="muted small" data-testid="available-offline-count">{t("downloads.offlineCount", { count: total })}</span>
        <button type="button" className="btn sm text" onClick={() => bridge().dispatch({ type: "clearStreamCache" })}>{t("downloads.clearCache")}</button>
        <button type="button" className="btn sm tonal" disabled={!total} onClick={() => play(0)}><Icon name="play" size={12} filled /> {t("downloads.playOffline")}</button>
      </div>
      <TrackTable tracks={rows} total={total} columns={["art", "title", "artist", "album", "duration", "offline"]} scope="available-offline" label={t("downloads.tab.offline")} onNeedRange={onNeedRange} onPlay={(i) => play(i)} playingTrackId={playing} emptyMessage={t("downloads.offlineEmpty")} testId="available-offline-table" />
    </div>
  );
}
