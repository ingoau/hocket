// Song info, the desktop dialog: on the left what the library knows (your
// listening, the sound and the file, credits); on the right what the server
// adds from Last.fm and MusicBrainz (similar tracks, the album's notes, the
// artist's biography and similar artists). Read-only: no action buttons
// beyond revealing the file.
import type { Track } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { fmtBytes, fmtDate, fmtTime } from "../lib/format";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";
import { Stars } from "./Stars";

type Row = [string, string | number | undefined];

function Rows({ rows }: { rows: Row[] }) {
  const shown = rows.filter(([, v]) => v !== undefined && v !== "");
  if (!shown.length) return null;
  return (
    <dl className="si-rows">
      {shown.map(([k, v]) => (
        <div key={k} className="si-row"><dt>{k}</dt><dd title={String(v)}>{String(v)}</dd></div>
      ))}
    </dl>
  );
}

export function SongInfo({ trackId }: { trackId: string }) {
  const close = useApp((s) => s.closeDialog);
  const navigate = useApp((s) => s.navigate);
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const { data } = useQuery(() => ({ type: "track", data: { id: trackId } }), "trackDetail", [trackId]);
  const tr = data as Track | undefined;
  const related = useQuery(() => ({ type: "related", data: { track_id: trackId, count: 6 } }), "related", [trackId], { static: true });
  const album = useQuery(() => (tr?.albumId ? { type: "albumInfo", data: { id: tr.albumId } } : null), "externalInfo", [tr?.albumId], { static: true });
  const artist = useQuery(() => (tr?.artistId ? { type: "artistInfo", data: { id: tr.artistId } } : null), "externalInfo", [tr?.artistId], { static: true });
  if (!tr) return <h2 className="dialog-title" id="dialog-title">{t("misc.loading")}</h2>;
  const playNext = (id: string) => bridge().dispatch({ type: "playNext", data: { server_id: serverId, track_ids: [id] } });
  const go = (route: Parameters<typeof navigate>[0]) => { close(); navigate(route); };
  const format = tr.suffix ? [tr.suffix.toUpperCase(), tr.bitRate ? `${tr.bitRate} kbps` : undefined, tr.sampleRate ? `${(tr.sampleRate / 1000).toFixed(1)} kHz` : undefined, tr.bitDepth ? `${tr.bitDepth}-bit` : undefined].filter(Boolean).join(" · ") : undefined;
  const similar = related.data ?? [];
  const albumInfo = album.data;
  const artistInfo = artist.data;
  const external = similar.length || albumInfo || artistInfo;
  return (
    <>
      <div className="si-head">
        <Artwork id={tr.coverArt} size={300} className="si-art" />
        <div className="grow" style={{ minWidth: 0 }}>
          <h2 className="dialog-title si-title" id="dialog-title">{tr.title}</h2>
          <div className="si-sub">
            {tr.artistId ? <a href="#" className="plain-link" onClick={(e) => { e.preventDefault(); go({ view: "artist", id: tr.artistId! }); }}>{tr.artist}</a> : tr.artist}
            {tr.album ? <> · {tr.albumId ? <a href="#" className="plain-link" onClick={(e) => { e.preventDefault(); go({ view: "album", id: tr.albumId! }); }}>{tr.album}</a> : tr.album}</> : null}
          </div>
          <div className="small muted">{[tr.year, tr.genre, fmtTime(tr.durationMs)].filter(Boolean).join(" · ")}</div>
          <Stars value={tr.rating} label={t("a11y.ratingOf", { title: tr.title })} onChange={(r) => bridge().dispatch({ type: "setRating", data: { targets: [{ type: "track", data: { id: tr.id } }], rating: r } })} />
        </div>
        <button type="button" className="btn icon" aria-label={t("misc.close")} title={t("misc.close")} onClick={close} data-testid="song-info-close"><Icon name="close" size={22} /></button>
      </div>
      <div className="si-body" tabIndex={0} role="group" aria-labelledby="dialog-title">
        <div className="si-col">
          <section className="si-card" aria-labelledby="si-listening">
            <h3 id="si-listening">{t("songInfo.listening")}</h3>
            <div className="si-big"><span className="si-num">{tr.playCount ?? 0}</span> {t("songInfo.plays")}</div>
            <Rows rows={[[t("songInfo.lastPlayed"), fmtDate(tr.lastPlayed)], [t("songInfo.added"), fmtDate(tr.created)], [t("songInfo.loved"), tr.loved ? t("songInfo.yes") : undefined]]} />
          </section>
          <section className="si-card" aria-labelledby="si-sound">
            <h3 id="si-sound">{t("songInfo.sound")}</h3>
            {format ? <div className="si-format">{format}</div> : null}
            <Rows rows={[
              [t("songInfo.size"), fmtBytes(tr.sizeBytes)],
              ["ReplayGain", tr.replayGain?.trackGainDb !== undefined ? `track ${tr.replayGain.trackGainDb.toFixed(1)} dB · album ${tr.replayGain.albumGainDb?.toFixed(1) ?? "–"} dB` : undefined],
              ["BPM", tr.sonic?.bpm], [t("songInfo.key"), tr.sonic?.key], [t("songInfo.energy"), tr.sonic?.energy !== undefined ? Math.round(tr.sonic.energy * 100) + "%" : undefined], [t("songInfo.mood"), tr.sonic?.mood],
              [t("songInfo.offline"), tr.offline !== "none" ? tr.offline : undefined],
            ]} />
            {tr.path ? (
              <div className="si-file">
                <code className="truncate" title={tr.path}>{tr.path}</code>
                <button type="button" className="btn text sm" onClick={() => bridge().shell.showItemInFolder(tr.path as string)}><Icon name="folder" size={16} /> {t("action.showInFolder")}</button>
              </div>
            ) : null}
          </section>
          <section className="si-card" aria-labelledby="si-credits">
            <h3 id="si-credits">{t("songInfo.credits")}</h3>
            <Rows rows={[[t("songInfo.artist"), tr.artist], [t("songInfo.albumArtist"), tr.albumArtist], [t("songInfo.track"), tr.trackNumber !== undefined ? `${tr.discNumber ?? 1}-${tr.trackNumber}` : undefined], [t("songInfo.genre"), tr.genre], ["MusicBrainz", tr.musicBrainzId]]} />
          </section>
        </div>
        <div className="si-col">
          <h3 className="si-eyebrow">{t("songInfo.around")}</h3>
          {similar.length ? (
            <section className="si-card" aria-labelledby="si-similar">
              <h3 id="si-similar">{t("songInfo.similar")} <span className="small muted">· {t("songInfo.similarHint")}</span></h3>
              <ul className="plain-list si-similar">
                {similar.map((r) => (
                  <li key={r.track.id}>
                    {/* A button, so the keyboard plays it next too: a key's click has detail 0, a double click's second 2, a single click (1) does nothing. */}
                    <button type="button" className="si-similar-row" onClick={(e) => { if (e.detail !== 1) playNext(r.track.id); }} title={r.reason} aria-label={t("songInfo.playNextNamed", { title: r.track.title, artist: r.track.artist ?? "" })} data-testid="song-info-similar">
                      <Artwork id={r.track.coverArt} size={64} className="art" />
                      <span className="grow truncate">{r.track.title}<span className="muted"> · {r.track.artist}</span></span>
                      <span className="small muted">{fmtTime(r.track.durationMs)}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          ) : null}
          {albumInfo?.text ? (
            <section className="si-card" aria-labelledby="si-album">
              <h3 id="si-album">{t("songInfo.aboutAlbum")}</h3>
              <p className="si-text">{albumInfo.text}</p>
              {albumInfo.lastFmUrl ? <a className="plain-link small" href={albumInfo.lastFmUrl} target="_blank" rel="noreferrer">{t("artist.readOnLastFm")}</a> : null}
            </section>
          ) : null}
          {artistInfo ? (
            <section className="si-card" aria-labelledby="si-artist">
              <h3 id="si-artist">{t("songInfo.aboutArtist")}</h3>
              {artistInfo.text ? <p className="si-text">{artistInfo.text}</p> : null}
              {artistInfo.similarArtists?.length ? (
                <ul className="plain-list si-chips" aria-label={t("songInfo.similarArtists")}>
                  {artistInfo.similarArtists.map((a) => <li key={a.id}><button type="button" className="btn tonal sm" onClick={() => go({ view: "artist", id: a.id })}>{a.name}</button></li>)}
                </ul>
              ) : null}
              {artistInfo.lastFmUrl ? <a className="plain-link small" href={artistInfo.lastFmUrl} target="_blank" rel="noreferrer">{t("artist.readOnLastFm")}</a> : null}
            </section>
          ) : null}
          {external ? <p className="small muted">{t("songInfo.source")}</p> : <p className="small muted">{t("songInfo.noExternal")}</p>}
        </div>
      </div>
    </>
  );
}
