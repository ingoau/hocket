import { useState } from "react";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { Artwork } from "../components/Artwork";
import { EmptyState } from "../components/EmptyState";
import { Icon } from "../components/Icon";

const PERIODS = [7, 30, 365] as const;
const DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const DAYS_LONG = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const hourSpan = (h: number) => `${String(h).padStart(2, "0")}:00–${String((h + 1) % 24).padStart(2, "0")}:00`;

/** A bar chart whose readout names the bar under the pointer (or the peak), the peak bar on the accent. */
function Bars({ values, labels, readout, peak, a11y, testId }: { values: number[]; labels: string[]; readout: (i: number) => string; peak: (i: number) => string; a11y: string; testId: string }) {
  const [hover, setHover] = useState(-1);
  const max = Math.max(1, ...values);
  const top = values.indexOf(Math.max(...values));
  return (
    <>
      <div className="stat-readout" aria-hidden="true" data-testid={`${testId}-readout`}>{hover >= 0 ? readout(hover) : peak(top)}</div>
      <div className="bars" role="img" aria-label={a11y} onMouseLeave={() => setHover(-1)} data-testid={testId}>
        {values.map((v, i) => <div key={i} className={`${i === top ? "peak" : ""} ${i === hover ? "hover" : ""}`} style={{ height: `${Math.max(2, (v / max) * 100)}%` }} onMouseEnter={() => setHover(i)} />)}
      </div>
      <div className="bar-labels" aria-hidden="true">{labels.map((l, i) => <span key={i}>{l}</span>)}</div>
    </>
  );
}

export function Stats() {
  const [days, setDays] = useState(30);
  const navigate = useApp((s) => s.navigate);
  const { data, loading } = useQuery(() => ({ type: "stats", data: { period_days: days } }), "stats", [days]);
  return (
    <div className="view page-stats" data-testid="view-stats">
      <div className="view-header">
        <h1 className="brand-type">{t("stats.title")}</h1>
        <div className="actions">
          <div className="seg-group stat-periods" role="group" aria-label={t("stats.periodLabel")}>
            {PERIODS.map((d) => <button key={d} type="button" className={`seg ${days === d ? "on" : ""}`} aria-pressed={days === d} onClick={() => setDays(d)} data-testid={`stats-period-${d}`}>{days === d ? <Icon name="check" size={16} /> : null}{d === 365 ? t("stats.year") : t("stats.days", { days: d })}</button>)}
          </div>
        </div>
      </div>
      <div className="view-body">
        {!loading && !data?.totalPlays ? <EmptyState message={t("stats.empty")} icon="stats" /> : null}
        {data?.totalPlays ? (
          <div className="stack stats-stack">
            <div className="stat-heroes">
              <div className="stat-hero"><Icon name="headphones" size={24} className="stat-icon" /><div className="stat-label">{t("stats.plays", { count: "" })}</div><div className="stat-figure">{data.totalPlays}</div></div>
              <div className="stat-hero"><Icon name="schedule" size={24} className="stat-icon" /><div className="stat-label">{t("stats.time", { hours: "" })}</div><div className="stat-figure">{(data.totalMs / 3_600_000).toFixed(1)}<span className="stat-unit"> h</span></div></div>
            </div>
            <div className="stat-grid">
              <div className="card"><h2 className="card-title">{t("stats.byHour")}</h2><Bars values={data.playsByHour} labels={data.playsByHour.map((_, i) => (i % 6 === 0 || i === 23 ? String(i) : ""))} readout={(i) => t("stats.readout", { what: hourSpan(i), count: data.playsByHour[i] ?? 0 })} peak={(i) => t("stats.peakHour", { what: hourSpan(i) })} a11y={data.playsByHour.map((v, i) => `${i}:00 ${v}`).join(", ")} testId="stats-hours" /></div>
              <div className="card"><h2 className="card-title">{t("stats.byWeekday")}</h2><Bars values={data.playsByWeekday} labels={DAYS} readout={(i) => t("stats.readout", { what: DAYS_LONG[i] ?? "", count: data.playsByWeekday[i] ?? 0 })} peak={(i) => t("stats.peakDay", { what: DAYS_LONG[i] ?? "" })} a11y={data.playsByWeekday.map((v, i) => `${DAYS[i]} ${v}`).join(", ")} testId="stats-days" /></div>
            </div>
            <div className="stat-grid">
              <div className="card"><h2 className="card-title">{t("stats.topTracks")}</h2><ol className="plain-list">{data.topTracks.map((tr, i) => <li key={tr.id} className="qrow"><span className="rank">{i + 1}</span><Artwork id={tr.coverArt} size={64} className="art" /><div className="text"><div className="t1">{tr.title}</div><div className="t2">{tr.artist}</div></div></li>)}</ol></div>
              <div className="card"><h2 className="card-title">{t("stats.topAlbums")}</h2><ol className="plain-list">{data.topAlbums.map((a, i) => <li key={a.id}><button type="button" className="qrow" onClick={() => navigate({ view: "album", id: a.id })}><span className="rank">{i + 1}</span><Artwork id={a.coverArt} size={64} className="art" /><div className="text"><div className="t1">{a.name}</div><div className="t2">{a.artist}</div></div></button></li>)}</ol></div>
              <div className="card"><h2 className="card-title">{t("stats.topArtists")}</h2><ol className="plain-list">{data.topArtists.map((a, i) => <li key={a.id}><button type="button" className="qrow" onClick={() => navigate({ view: "artist", id: a.id })}><span className="rank">{i + 1}</span><Artwork id={a.coverArt} size={64} className="art" round /><div className="text"><div className="t1">{a.name}</div></div></button></li>)}</ol></div>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
