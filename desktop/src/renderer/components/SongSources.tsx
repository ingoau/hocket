// The song's sources (docs/providers.md, Phase 3): every copy of it across accounts, which one
// plays here, and the two hand corrections: "Not the same song" takes a copy out of the group,
// "Link to…" joins a song the matcher kept apart. Both sync through the identity.
import { useState } from "react";
import type { Track, TrackRef } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { refKey, refOf } from "../lib/track-ref";

export function SongSources({ track }: { track: TrackRef }) {
  const several = useApp((s) => s.accounts.length > 1);
  const { data } = useQuery(() => ({ type: "trackSources", data: { track } }), "trackSources", [refKey(track)]);
  const [linking, setLinking] = useState(false);
  const sources = data ?? [];
  if (!several && sources.length < 2) return null;
  return (
    <section className="si-sources" aria-labelledby="si-sources-h" data-testid="song-sources">
      <h3 id="si-sources-h" className="si-eyebrow">{t("sources.title")}</h3>
      <ul className="si-source-list">
        {sources.map((s) => (
          <li key={refKey(refOf(s.track))} className="si-source" data-testid="song-source">
            <span className="si-source-name">{s.accountName || s.accountId}</span>
            <span className="muted">
              {[s.preferred ? t("sources.preferred") : undefined, s.manual ? t("sources.linkedByHand") : undefined, s.available ? undefined : t("sources.notHere")].filter(Boolean).join(" · ")}
            </span>
            {sources.length > 1 ? (
              <button type="button" className="btn text sm" onClick={() => bridge().dispatch({ type: "unlinkTrack", data: { track: refOf(s.track) } })} data-testid="song-source-unlink">{t("sources.notSame")}</button>
            ) : null}
          </li>
        ))}
      </ul>
      {several ? (
        linking ? <LinkPicker track={track} onDone={() => setLinking(false)} /> : <button type="button" className="btn tonal sm" onClick={() => setLinking(true)} data-testid="song-source-link">{t("sources.linkTo")}</button>
      ) : null}
    </section>
  );
}

function LinkPicker({ track, onDone }: { track: TrackRef; onDone: () => void }) {
  const [query, setQuery] = useState("");
  const { data } = useQuery(() => ({ type: "linkCandidates", data: { track, query } }), "trackList", [refKey(track), query]);
  const candidates = (data ?? []) as Track[];
  return (
    <div className="si-link" data-testid="song-link-picker">
      <input className="input" autoFocus placeholder={t("sources.linkSearch")} value={query} onChange={(e) => setQuery(e.target.value)} data-testid="song-link-search" />
      {candidates.length ? (
        <ul className="si-source-list">
          {candidates.map((c) => (
            <li key={refKey(refOf(c))}>
              <button type="button" className="btn text sm" onClick={() => { bridge().dispatch({ type: "linkTracks", data: { track, to: refOf(c) } }); onDone(); }} data-testid="song-link-candidate">
                {c.title} <span className="muted">{[c.artist, c.album].filter(Boolean).join(" · ")}</span>
              </button>
            </li>
          ))}
        </ul>
      ) : <div className="muted">{t("sources.noCandidates")}</div>}
    </div>
  );
}
