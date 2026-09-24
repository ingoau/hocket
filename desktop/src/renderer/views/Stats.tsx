import { useState } from "react";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { Artwork } from "../components/Artwork";
import { EmptyState } from "../components/EmptyState";

export function Stats() {
  const [days, setDays] = useState(30);
  const navigate = useApp((s) => s.navigate);
  const { data, loading } = useQuery(() => ({ type: "stats", data: { period_days: days } }), "stats", [days]);
  const maxHour = Math.max(1, ...(data?.playsByHour ?? [0]));
  const maxDay = Math.max(1, ...(data?.playsByWeekday ?? [0]));
  return (
    <div className="view" data-testid="view-stats">
      <div className="view-header">
        <h1>{t("stats.title")}</h1>
        <div className="actions">
          <select className="select" value={days} aria-label={t("stats.title")} onChange={(e) => setDays(Number(e.target.value))}>{[7, 30, 90, 365].map((d) => <option key={d} value={d}>{t("stats.period", { days: d })}</option>)}</select>
        </div>
      </div>
      <div className="view-body">
        {!loading && !data?.totalPlays ? <EmptyState message={t("stats.empty")} /> : null}
        {data?.totalPlays ? (
          <div className="stack" style={{ gap: 18 }}>
            <div className="row" style={{ gap: 24 }}>
              <div><div className="xs muted">{t("stats.plays", { count: "" })}</div><div style={{ fontSize: 28, fontWeight: 600 }}>{data.totalPlays}</div></div>
              <div><div className="xs muted">{t("stats.time", { hours: "" })}</div><div style={{ fontSize: 28, fontWeight: 600 }}>{(data.totalMs / 3_600_000).toFixed(1)} h</div></div>
            </div>
            <div className="stat-grid">
              <div className="card"><h2 className="card-title">{t("stats.byHour")}</h2><div className="bars" role="img" aria-label={data.playsByHour.map((v, i) => `${i}:00 ${v}`).join(", ")}>{data.playsByHour.map((v, i) => <div key={i} style={{ height: `${(v / maxHour) * 100}%` }} title={`${i}:00 · ${v}`} />)}</div><div className="bar-labels" aria-hidden="true">{data.playsByHour.map((_, i) => <span key={i}>{i % 6 === 0 ? i : ""}</span>)}</div></div>
              <div className="card"><h2 className="card-title">{t("stats.byWeekday")}</h2><div className="bars" role="img" aria-label={data.playsByWeekday.map((v, i) => `${["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][i]} ${v}`).join(", ")}>{data.playsByWeekday.map((v, i) => <div key={i} style={{ height: `${(v / maxDay) * 100}%` }} title={`${v}`} />)}</div><div className="bar-labels" aria-hidden="true">{["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].map((d) => <span key={d}>{d}</span>)}</div></div>
            </div>
            <div className="stat-grid">
              <div className="card"><h2 className="card-title">{t("stats.topTracks")}</h2><ol className="plain-list">{data.topTracks.map((tr, i) => <li key={tr.id} className="qrow" style={{ height: 36, margin: 0 }}><span className="faint xs" style={{ width: 16 }}>{i + 1}</span><Artwork id={tr.coverArt} size={64} className="art" /><div className="text"><div className="t1">{tr.title}</div><div className="t2">{tr.artist}</div></div></li>)}</ol></div>
              <div className="card"><h2 className="card-title">{t("stats.topAlbums")}</h2><ol className="plain-list">{data.topAlbums.map((a, i) => <li key={a.id}><button type="button" className="qrow" style={{ height: 36, margin: 0 }} onClick={() => navigate({ view: "album", id: a.id })}><span className="faint xs" style={{ width: 16 }}>{i + 1}</span><Artwork id={a.coverArt} size={64} className="art" /><div className="text"><div className="t1">{a.name}</div><div className="t2">{a.artist}</div></div></button></li>)}</ol></div>
              <div className="card"><h2 className="card-title">{t("stats.topArtists")}</h2><ol className="plain-list">{data.topArtists.map((a, i) => <li key={a.id}><button type="button" className="qrow" style={{ height: 36, margin: 0 }} onClick={() => navigate({ view: "artist", id: a.id })}><span className="faint xs" style={{ width: 16 }}>{i + 1}</span><Artwork id={a.coverArt} size={64} className="art" round /><div className="text"><div className="t1">{a.name}</div></div></button></li>)}</ol></div>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
