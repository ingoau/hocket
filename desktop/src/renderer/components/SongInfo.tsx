// Song info, the desktop dialog. One scrolling page: the song's title, links
// and your rating; chips that jump to its sections; then three cards (your
// listening, with how this song's plays compare to the rest of its album; the
// sound and the file; credits) and, below, what the server adds from Last.fm
// and MusicBrainz (similar tracks, the album's notes, the artist's biography
// and similar artists). Read-only: no action buttons beyond revealing the file.
import { useRef, useState } from "react";
import type { Track } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { fmtBytes, fmtDate, fmtTime } from "../lib/format";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";
import { Stars } from "./Stars";

type Row = [string, React.ReactNode | undefined];
type Section = "listening" | "sound" | "credits" | "around";
const SECTIONS: Section[] = ["listening", "sound", "credits", "around"];

function Rows({ rows }: { rows: Row[] }) {
  const shown = rows.filter(([, v]) => v !== undefined && v !== "");
  if (!shown.length) return null;
  return (
    <dl className="si-rows">
      {shown.map(([k, v]) => (
        <div key={k} className="si-row"><dt>{k}</dt><dd title={typeof v === "string" || typeof v === "number" ? String(v) : undefined}>{v}</dd></div>
      ))}
    </dl>
  );
}

const ordinal = (n: number) => {
  const s = n % 100 >= 11 && n % 100 <= 13 ? "th" : ({ 1: "st", 2: "nd", 3: "rd" } as Record<number, string>)[n % 10] ?? "th";
  return `${n}${s}`;
};

export function SongInfo({ trackId }: { trackId: string }) {
  const close = useApp((s) => s.closeDialog);
  const navigate = useApp((s) => s.navigate);
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const { data } = useQuery(() => ({ type: "track", data: { id: trackId } }), "trackDetail", [trackId]);
  const tr = data as Track | undefined;
  const related = useQuery(() => ({ type: "related", data: { track_id: trackId, count: 6 } }), "related", [trackId], { static: true });
  const albumTracks = useQuery(() => (tr?.albumId ? { type: "albumTracks", data: { id: tr.albumId } } : null), "trackList", [tr?.albumId], { static: true });
  const album = useQuery(() => (tr?.albumId ? { type: "albumInfo", data: { id: tr.albumId } } : null), "externalInfo", [tr?.albumId], { static: true });
  const artist = useQuery(() => (tr?.artistId ? { type: "artistInfo", data: { id: tr.artistId } } : null), "externalInfo", [tr?.artistId], { static: true });
  const scroller = useRef<HTMLDivElement>(null);
  const [on, setOn] = useState<Section>("listening");
  if (!tr) return <h2 className="dialog-title" id="dialog-title">{t("misc.loading")}</h2>;
  const playNext = (id: string) => bridge().dispatch({ type: "playNext", data: { server_id: serverId, track_ids: [id] } });
  const go = (route: Parameters<typeof navigate>[0]) => { close(); navigate(route); };
  const link = (label: string | undefined, route: Parameters<typeof navigate>[0] | undefined) => (label ? (route ? <a href="#" className="plain-link" onClick={(e) => { e.preventDefault(); go(route); }}>{label}</a> : label) : undefined);
  const artistLink = link(tr.artist, tr.artistId ? { view: "artist", id: tr.artistId } : undefined);
  const albumLink = link(tr.album, tr.albumId ? { view: "album", id: tr.albumId } : undefined);
  const similar = related.data ?? [];
  const albumInfo = album.data;
  const artistInfo = artist.data;
  const external = similar.length || albumInfo || artistInfo;
  const siblings = albumTracks.data ?? [];
  const most = Math.max(1, ...siblings.map((s) => s.playCount ?? 0));
  const rank = siblings.length > 1 && (tr.playCount ?? 0) > 0 ? 1 + siblings.filter((s) => (s.playCount ?? 0) > (tr.playCount ?? 0)).length : undefined;
  const jump = (s: Section) => {
    setOn(s);
    scroller.current?.querySelector(`#si-${s}`)?.scrollIntoView({ block: "start", behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
  };
  return (
    <>
      <button type="button" className="btn icon si-close" aria-label={t("misc.close")} title={t("misc.close")} onClick={close} data-testid="song-info-close"><Icon name="close" size={22} /></button>
      <div className="si-scroll" ref={scroller} tabIndex={0} role="group" aria-labelledby="dialog-title">
        <header className="si-head">
          <h2 className="dialog-title si-title" id="dialog-title">{tr.title}</h2>
          <div className="si-sub">
            {artistLink}{albumLink ? <span className="si-dot" aria-hidden="true">·</span> : null}{albumLink}
            <span className="muted">{[tr.year, tr.genre, fmtTime(tr.durationMs)].filter(Boolean).join(" · ")}</span>
          </div>
          <div className="si-rating">
            <span className="muted" aria-hidden="true">{t("album.yourRating")}</span>
            <Stars value={tr.rating} label={t("a11y.ratingOf", { title: tr.title })} onChange={(r) => bridge().dispatch({ type: "setRating", data: { targets: [{ type: "track", data: { id: tr.id } }], rating: r } })} />
          </div>
        </header>
        <nav className="si-tabs" aria-label={t("songInfo.sections")}>
          {SECTIONS.map((s) => (
            <button key={s} type="button" className={`si-tab ${on === s ? "on" : ""}`} aria-current={on === s || undefined} onClick={() => jump(s)} data-testid={`song-info-tab-${s}`}>{t(`songInfo.tab.${s}`)}</button>
          ))}
        </nav>
        <div className="si-cards">
          <section className="si-card" id="si-listening" aria-labelledby="si-listening-h">
            <h3 id="si-listening-h" className="si-eyebrow">{t("songInfo.listening")}</h3>
            <div className="si-big"><span className="si-num">{tr.playCount ?? 0}</span> {t("songInfo.plays")}</div>
            {siblings.length > 1 ? (
              <div className="si-bars" aria-hidden="true">
                {siblings.map((s) => <span key={s.id} className={s.id === tr.id ? "on" : ""} style={{ height: `${Math.max(8, ((s.playCount ?? 0) / most) * 100)}%` }} />)}
              </div>
            ) : null}
            {rank && tr.album ? <div className="si-caption">{t("songInfo.rank", { rank: ordinal(rank), album: tr.album })}</div> : null}
            <Rows rows={[[t("songInfo.lastPlayed"), fmtDate(tr.lastPlayed)], [t("songInfo.added"), fmtDate(tr.created)], [t("songInfo.loved"), tr.loved ? t("songInfo.yes") : undefined], [t("songInfo.offline"), tr.offline !== "none" ? t(`songInfo.offline.${tr.offline}` as never) : undefined]]} />
          </section>
          <section className="si-card" id="si-sound" aria-labelledby="si-sound-h">
            <h3 id="si-sound-h" className="si-eyebrow">{t("songInfo.sound")}</h3>
            {tr.suffix ? (
              <>
                <div className="si-num si-format">{tr.suffix.toUpperCase()}</div>
                <div className="si-caption">{[tr.bitDepth ? `${tr.bitDepth}-bit` : undefined, tr.sampleRate ? `${(tr.sampleRate / 1000).toFixed(1)} kHz` : undefined, tr.bitRate ? `${tr.bitRate} kbps` : undefined].filter(Boolean).join(" · ")}</div>
              </>
            ) : null}
            <Rows rows={[
              ["ReplayGain", tr.replayGain?.trackGainDb !== undefined ? `track ${tr.replayGain.trackGainDb.toFixed(1)} dB · album ${tr.replayGain.albumGainDb?.toFixed(1) ?? "–"} dB` : undefined],
              ["BPM", tr.sonic?.bpm], [t("songInfo.key"), tr.sonic?.key], [t("songInfo.energy"), tr.sonic?.energy !== undefined ? Math.round(tr.sonic.energy * 100) + "%" : undefined], [t("songInfo.mood"), tr.sonic?.mood],
              [t("songInfo.size"), fmtBytes(tr.sizeBytes)],
            ]} />
            {tr.path ? (
              <div className="si-file">
                <span className="si-file-label muted">{t("songInfo.file")}</span>
                <code title={tr.path}>{tr.path}</code>
                <button type="button" className="btn tonal sm" onClick={() => bridge().shell.showItemInFolder(tr.path as string)}><Icon name="folder" size={16} /> {t("action.showInFolder")}</button>
              </div>
            ) : null}
          </section>
          <section className="si-card" id="si-credits" aria-labelledby="si-credits-h">
            <h3 id="si-credits-h" className="si-eyebrow">{t("songInfo.credits")}</h3>
            <Rows rows={[[t("songInfo.title"), tr.title], [t("songInfo.album"), albumLink], [t("songInfo.artist"), artistLink], [t("songInfo.albumArtist"), tr.albumArtist && tr.albumArtist !== tr.artist ? tr.albumArtist : undefined], [t("songInfo.year"), tr.year], [t("songInfo.genre"), tr.genre], [t("songInfo.track"), tr.trackNumber !== undefined ? t("songInfo.trackDisc", { track: tr.trackNumber, disc: tr.discNumber ?? 1 }) : undefined], [t("songInfo.duration"), fmtTime(tr.durationMs)], ["MusicBrainz", tr.musicBrainzId]]} />
          </section>
        </div>
        <section className="si-around" id="si-around" aria-labelledby="si-around-h">
          <h3 id="si-around-h" className="si-around-title">{t("songInfo.around")}</h3>
          {external ? (
            <div className="si-around-grid">
              <div>
                {similar.length ? (
                  <section aria-labelledby="si-similar">
                    <div className="si-around-head"><h4 id="si-similar">{t("songInfo.similar")}</h4><span className="small muted">{t("songInfo.similarHint")}</span></div>
                    <ul className="plain-list si-similar">
                      {similar.map((r) => (
                        <li key={r.track.id}>
                          {/* A button, so the keyboard plays it next too: a key's click has detail 0, a double click's second 2, a single click (1) does nothing. */}
                          <button type="button" className="si-similar-row" onClick={(e) => { if (e.detail !== 1) playNext(r.track.id); }} aria-label={t("songInfo.playNextNamed", { title: r.track.title, artist: r.track.artist ?? "" })} data-testid="song-info-similar">
                            <Artwork id={r.track.coverArt} size={96} className="art" />
                            <span className="grow" style={{ minWidth: 0 }}>
                              <span className="truncate si-similar-title">{r.track.title}<span className="muted"> · {r.track.artist}</span></span>
                              {r.reason ? <span className="truncate small muted si-similar-why">{r.reason}</span> : null}
                            </span>
                            <span className="small muted">{fmtTime(r.track.durationMs)}</span>
                          </button>
                        </li>
                      ))}
                    </ul>
                  </section>
                ) : null}
              </div>
              <div className="si-col">
                {albumInfo?.text ? (
                  <section className="si-card" aria-labelledby="si-album">
                    <div className="si-about-head">
                      <Artwork id={tr.coverArt} size={96} className="art" />
                      <div><h4 id="si-album">{t("songInfo.aboutAlbum")}</h4><div className="small muted">{[tr.album, tr.year].filter(Boolean).join(" · ")}</div></div>
                    </div>
                    <p className="si-text">{albumInfo.text}</p>
                    {albumInfo.lastFmUrl ? <div className="si-chips"><a className="btn sm si-ext" href={albumInfo.lastFmUrl} target="_blank" rel="noreferrer">Last.fm <Icon name="external" size={14} /></a></div> : null}
                  </section>
                ) : null}
                {artistInfo ? (
                  <section className="si-card" aria-labelledby="si-artist">
                    <div className="si-about-head">
                      <div><h4 id="si-artist">{t("songInfo.aboutArtist")}</h4>{artistLink ? <div className="si-about-link">{artistLink}</div> : null}</div>
                    </div>
                    {artistInfo.text ? <p className="si-text">{artistInfo.text}</p> : null}
                    {artistInfo.similarArtists?.length ? (
                      <>
                        <h5 className="si-mini">{t("songInfo.similarArtists")}</h5>
                        <ul className="plain-list si-chips" aria-label={t("songInfo.similarArtists")}>
                          {artistInfo.similarArtists.map((a) => <li key={a.id}><button type="button" className="btn sm si-ext" onClick={() => go({ view: "artist", id: a.id })}>{a.name}</button></li>)}
                        </ul>
                      </>
                    ) : null}
                    {artistInfo.lastFmUrl ? <div className="si-chips"><a className="btn sm si-ext" href={artistInfo.lastFmUrl} target="_blank" rel="noreferrer">Last.fm <Icon name="external" size={14} /></a></div> : null}
                  </section>
                ) : null}
              </div>
            </div>
          ) : null}
          <p className="small muted si-source">{external ? t("songInfo.source") : t("songInfo.noExternal")}</p>
        </section>
      </div>
    </>
  );
}
