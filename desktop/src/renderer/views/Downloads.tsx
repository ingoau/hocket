import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { Artwork } from "../components/Artwork";
import { EmptyState } from "../components/EmptyState";
import { Icon } from "../components/Icon";
import { fmtBytes } from "../lib/format";

export function Downloads() {
  const pins = useApp((s) => s.pins);
  const storage = useApp((s) => s.storage);
  const navigate = useApp((s) => s.navigate);
  const openDialog = useApp((s) => s.openDialog);
  const fetched = useQuery(() => ({ type: "pins" }), "pins", [], { static: true });
  const st = useQuery(() => ({ type: "storage" }), "storage", [pins.length], { static: true });
  const list = pins.length ? pins : (fetched.data ?? []);
  const sum = storage ?? st.data;
  const remove = (p: (typeof list)[number]) => openDialog({ kind: "confirm", title: t("downloads.remove"), message: t("dialog.deleteDownload", { name: p.label }), confirmLabel: t("dialog.delete"), destructive: true, onConfirm: () => bridge().dispatch({ type: "unpin", data: { target: p.target } }) });
  const open = (p: (typeof list)[number]) => {
    if (p.target.type === "album") navigate({ view: "album", id: p.target.data.id });
    else if (p.target.type === "playlist") navigate({ view: "playlist", id: p.target.data.id });
  };
  return (
    <div className="view" data-testid="view-downloads">
      <div className="view-header">
        <h1>{t("downloads.title")}</h1>
        <div className="actions">
          {sum ? <span className="muted small">{t("downloads.usage", { downloads: fmtBytes(sum.downloadsBytes), cache: fmtBytes(sum.cacheBytes), images: fmtBytes(sum.imagesBytes) })}</span> : null}
          <button type="button" className="btn" onClick={() => bridge().dispatch({ type: "clearStreamCache" })}>{t("downloads.clearCache")}</button>
        </div>
      </div>
      <div className="view-body">
        {!list.length ? <EmptyState message={t("downloads.empty")} action={<button type="button" className="btn" onClick={() => navigate({ view: "albums" })}>{t("nav.albums")}</button>} /> : null}
        {list.map((p) => (
          <div key={JSON.stringify(p.target)} className="pin-row" data-testid="pin-row">
            <Artwork id={p.coverArt} size={64} className="art" />
            <div style={{ minWidth: 0 }}>
              <div className="truncate" role="button" tabIndex={0} onClick={() => open(p)} onKeyDown={(e) => e.key === "Enter" && open(p)} style={{ cursor: "pointer" }}>{p.label}</div>
              <div className="small muted">{t("downloads.progress", { done: p.downloadedCount, total: p.trackCount })} · {fmtBytes(p.bytes)}{p.transcoded ? ` · ${t("downloads.transcoded")}` : ""}</div>
              {p.downloadedCount < p.trackCount ? <div className="progress" style={{ marginTop: 4 }}><div style={{ width: `${(p.downloadedCount / Math.max(1, p.trackCount)) * 100}%` }} /></div> : null}
            </div>
            <span className="badge">{p.target.type}</span>
            <button type="button" className="btn icon" aria-label={t("downloads.remove")} title={t("downloads.remove")} onClick={() => remove(p)}><Icon name="trash" size={14} /></button>
          </div>
        ))}
      </div>
    </div>
  );
}
