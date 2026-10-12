// "About this song": the song's details as a two-column grid, its rating, the
// file path with Show in folder, and similar tracks. One component for the
// side panel's About drill-in and the fullscreen player's About view; the
// side panel's adds the artwork and title on top (the fullscreen player shows
// those beside it already). Colours come from the surrounding tokens (--fg,
// --fg-muted, …), so it reads on the theme's surfaces and on the fullscreen
// player's artwork-coloured background alike.
import type { Track } from "@core/api";
import { t } from "@shared/strings";
import { useApp, type Route } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { fmtBytes, fmtDate, fmtTime } from "../lib/format";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";
import { Stars } from "./Stars";

/** The format line: FLAC · 44.1 kHz · 24-bit … */
export function formatLine(tr: Pick<Track, "suffix" | "bitRate" | "sampleRate" | "bitDepth" | "channels">, short = false): string {
  return [
    tr.suffix?.toUpperCase(),
    !short && tr.bitRate ? t("about.bitRate", { kbps: tr.bitRate }) : undefined,
    tr.sampleRate ? t("about.sampleRate", { khz: Math.round(tr.sampleRate / 100) / 10 }) : undefined,
    !short && tr.bitDepth ? t("about.bitDepth", { bits: tr.bitDepth }) : undefined,
    !short && tr.channels ? t("about.channels", { n: tr.channels }) : undefined,
  ].filter(Boolean).join(" · ");
}

export function AboutSong({ trackId, onNavigate, variant }: { trackId: string | undefined; onNavigate: (route: Route) => void; variant: "side" | "fullscreen" }) {
  const { data } = useQuery(() => (trackId ? { type: "track", data: { id: trackId } } : null), "trackDetail", [trackId]);
  const tr = data as Track | undefined;
  const d = bridge().dispatch;
  if (!trackId) return <div className="empty">{t("player.nothingPlaying")}</div>;
  if (!tr) return <div className="empty" role="status">{t("misc.loading")}</div>;
  const format = formatLine(tr);
  const link = (label: string | undefined, route: Route | undefined) => (label && route ? <button type="button" className="about-link" onClick={() => onNavigate(route)}>{label}</button> : label);
  const rows: [string, React.ReactNode][] = [
    [t("about.album"), link(tr.album, tr.albumId ? { view: "album", id: tr.albumId } : undefined)],
    [t("about.artist"), link(tr.artist, tr.artistId ? { view: "artist", id: tr.artistId } : undefined)],
    [t("about.albumArtist"), tr.albumArtist && tr.albumArtist !== tr.artist ? tr.albumArtist : undefined],
    [t("about.year"), tr.year],
    [t("about.genre"), tr.genre],
    [t("about.track"), tr.trackNumber !== undefined ? t("about.trackOfDisc", { track: tr.trackNumber, disc: tr.discNumber ?? 1 }) : undefined],
    [t("about.duration"), fmtTime(tr.durationMs)],
    [t("about.format"), format || undefined],
    [t("about.playCount"), tr.playCount],
    [t("about.lastPlayed"), tr.lastPlayed ? fmtDate(tr.lastPlayed) : undefined],
    [t("about.added"), tr.created ? fmtDate(tr.created) : undefined],
    [t("about.replayGain"), tr.replayGain?.trackGainDb !== undefined ? `${tr.replayGain.trackGainDb.toFixed(1)} dB` : undefined],
    [t("about.bpm"), tr.sonic?.bpm],
    [t("about.key"), tr.sonic?.key],
    [t("about.size"), tr.sizeBytes ? fmtBytes(tr.sizeBytes) : undefined],
    [t("about.comment"), tr.comment],
  ];
  return (
    <div className={`about-song about-${variant === "fullscreen" ? "player" : "side"}`} data-testid="np-about">
      {variant === "side" ? (
        <div className="about-head">
          <Artwork id={tr.coverArt} size={600} className="about-art" />
          <h3 className="about-title">{tr.title}</h3>
          <div className="about-artist">{tr.artist ?? t("misc.unknownArtist")}</div>
        </div>
      ) : (
        <h2 className="about-heading">{t("nowPlaying.aboutSong")}</h2>
      )}
      <dl className="about-facts">
        {rows.filter(([, v]) => v !== undefined && v !== null && v !== "").map(([k, v]) => (
          <div key={k} className={`about-fact ${k === t("about.comment") ? "wide" : ""}`}>
            <dt>{k}</dt>
            <dd className="selectable">{v}</dd>
          </div>
        ))}
        <div className="about-fact">
          <dt>{t("about.rating")}</dt>
          <dd><Stars value={tr.rating} size={18} label={t("a11y.ratingOf", { title: tr.title })} onChange={(r) => d({ type: "setRating", data: { targets: [{ type: "track", data: { id: tr.id } }], rating: r } })} /></dd>
        </div>
      </dl>
      {tr.path ? (
        <div className="about-path">
          <span className="about-path-text selectable" title={tr.path}><span className="sr-only">{`${t("about.path")}: `}</span>{tr.path}</span>
          <button type="button" className="about-chip" onClick={() => bridge().shell.showItemInFolder(tr.path as string)}><Icon name="folder" size={18} />{t("action.showInFolder")}</button>
        </div>
      ) : null}
      <Related trackId={tr.id} />
    </div>
  );
}

function Related({ trackId }: { trackId: string }) {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const { data, loading } = useQuery(() => ({ type: "related", data: { track_id: trackId, count: 12 } }), "related", [trackId], { static: true });
  const providerLabel = (p: string) => t(`related.provider.${p}` as never);
  const playNext = (id: string) => bridge().dispatch({ type: "playNext", data: { server_id: serverId, track_ids: [id] } });
  return (
    <section className="about-related" aria-labelledby={`related-${trackId}`}>
      <h3 className="about-subheading" id={`related-${trackId}`}>{t("nowPlaying.similar")}</h3>
      {!loading && !data?.length ? <p className="about-muted">{t("fullscreen.relatedEmpty")}</p> : (
        <ul className="plain-list" aria-label={t("nowPlaying.similar")} data-testid="related-list">
          {(data ?? []).map((r) => (
            <li key={r.track.id} className="related-row" onDoubleClick={() => playNext(r.track.id)}>
              <Artwork id={r.track.coverArt} size={64} className="art" />
              <div className="grow" style={{ minWidth: 0 }}>
                <div className="truncate">{r.track.title} <span className="about-muted">· {r.track.artist}</span></div>
                <div className="why truncate">{providerLabel(r.provider)}{r.score !== undefined ? ` · ${t("related.score", { score: Math.round(r.score * 100) })}` : ""} · {r.reason}</div>
              </div>
              <span className="about-muted xs">{fmtTime(r.track.durationMs)}</span>
              <button type="button" className="about-icon-btn" aria-label={`${t("action.playNext")}: ${r.track.title}`} title={t("action.playNext")} onClick={() => playNext(r.track.id)}><Icon name="playNext" size={18} /></button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
