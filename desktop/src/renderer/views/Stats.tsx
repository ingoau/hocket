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

/** A bar chart in a card: the title and a readout naming the bar under the pointer (or the peak); each bar on its own track, the peak on the accent. */
function Chart({ title, values, labels, readout, peak, a11y, testId }: { title: string; values: number[]; labels: string[]; readout: (i: number) => string; peak: (i: number) => string; a11y: string; testId: string }) {
  const [hover, setHover] = useState(-1);
  const max = Math.max(1, ...values);
  const top = values.indexOf(Math.max(...values));
  return (
    <section className="stat-card stat-chart" aria-label={title}>
      <div className="stat-card-head">
        <h2>{title}</h2>
        <span className="stat-readout" aria-hidden="true" data-testid={`${testId}-readout`}>{hover >= 0 ? readout(hover) : peak(top)}</span>
      </div>
      <div className="bars" role="img" aria-label={a11y} onMouseLeave={() => setHover(-1)} data-testid={testId}>
        {values.map((v, i) => <div key={i} className={`${i === top ? "peak" : ""} ${i === hover ? "hover" : ""}`} onMouseEnter={() => setHover(i)}><span style={{ height: `${v ? Math.max(6, (v / max) * 100) : 0}%` }} /></div>)}
      </div>
      <div className="bar-labels" aria-hidden="true">{labels.map((l, i) => <span key={i}>{l}</span>)}</div>
    </section>
  );
}

export function Stats() {
  const [days, setDays] = useState(30);
  const navigate = useApp((s) => s.navigate);
  const { data, loading } = useQuery(() => ({ type: "stats", data: { period_days: days } }), "stats", [days]);
  const hours = Math.floor((data?.totalMs ?? 0) / 3_600_000);
  const minutes = Math.round(((data?.totalMs ?? 0) % 3_600_000) / 60_000);
  return (
    <div className="view page-stats" data-testid="view-stats">
      <div className="view-header">
        <h1 className="brand-type">{t("stats.heading")}</h1>
        <div className="actions">
          <div className="button-group stat-periods" role="group" aria-label={t("stats.periodLabel")}>
            {PERIODS.map((d) => <button key={d} type="button" className={`seg ${days === d ? "on" : ""}`} aria-pressed={days === d} onClick={() => setDays(d)} data-testid={`stats-period-${d}`}>{days === d ? <Icon name="check" size={18} /> : null}{d === 365 ? t("stats.year") : t("stats.days", { days: d })}</button>)}
          </div>
        </div>
      </div>
      <div className="view-body">
        {!loading && !data?.totalPlays ? <EmptyState message={t("stats.empty")} icon="stats" /> : null}
        {data?.totalPlays ? (
          <div className="stats-stack">
            <div className="stat-top">
              <div className="stat-heroes">
                <div className="stat-hero"><div className="stat-figure">{data.totalPlays}</div><div className="stat-label">{t("stats.playsLabel")}</div></div>
                <div className="stat-hero"><div className="stat-figure">{hours}<span className="stat-unit"> h</span></div><div className="stat-label">{t("stats.minutesListened", { minutes })}</div></div>
              </div>
              <Chart title={t("stats.byHour")} values={data.playsByHour} labels={data.playsByHour.map((_, i) => (i % 6 === 0 || i === 23 ? String(i) : ""))} readout={(i) => t("stats.readout", { what: hourSpan(i), count: data.playsByHour[i] ?? 0 })} peak={(i) => t("stats.peakHour", { what: hourSpan(i) })} a11y={data.playsByHour.map((v, i) => `${i}:00 ${v}`).join(", ")} testId="stats-hours" />
              <Chart title={t("stats.byWeekday")} values={data.playsByWeekday} labels={DAYS} readout={(i) => t("stats.readout", { what: DAYS_LONG[i] ?? "", count: data.playsByWeekday[i] ?? 0 })} peak={(i) => t("stats.peakDay", { what: DAYS_LONG[i] ?? "" })} a11y={data.playsByWeekday.map((v, i) => `${DAYS[i]} ${v}`).join(", ")} testId="stats-days" />
            </div>
            <div className="stat-tops">
              <section aria-labelledby="stats-top-tracks">
                <h2 className="stat-tops-title" id="stats-top-tracks">{t("stats.topTracks")}</h2>
                <ol className="plain-list stat-rows">{data.topTracks.map((tr, i) => <li key={tr.id} className="stat-row"><span className="rank">{i + 1}</span><Artwork id={tr.coverArt} size={96} className="art" /><div className="text"><div className="t1 truncate">{tr.title}</div><div className="t2 truncate">{[tr.artist, tr.album].filter(Boolean).join(" · ")}</div></div></li>)}</ol>
              </section>
              <section aria-labelledby="stats-top-albums">
                <h2 className="stat-tops-title" id="stats-top-albums">{t("stats.topAlbums")}</h2>
                <ol className="plain-list stat-albums">{data.topAlbums.map((a, i) => <li key={a.id}><button type="button" className="stat-album" onClick={() => navigate({ view: "album", id: a.id })}><Artwork id={a.coverArt} size={300} className="art" /><span className="t1 truncate">{i + 1}. {a.name}</span><span className="t2 truncate">{a.artist}</span></button></li>)}</ol>
              </section>
              <section aria-labelledby="stats-top-artists">
                <h2 className="stat-tops-title" id="stats-top-artists">{t("stats.topArtists")}</h2>
                <ol className="plain-list stat-rows">{data.topArtists.map((a, i) => <li key={a.id}><button type="button" className="stat-row" onClick={() => navigate({ view: "artist", id: a.id })}><span className="rank">{i + 1}</span><Artwork id={a.coverArt} size={96} className="art round" round /><div className="text"><div className="t1 truncate">{a.name}</div><div className="t2 truncate">{t(a.albumCount === 1 ? "release.count.album.one" : "release.count.album.other", { count: a.albumCount })}</div></div></button></li>)}</ol>
              </section>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
