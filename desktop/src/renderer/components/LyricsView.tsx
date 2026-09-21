// AMLL lyric renderer wrapper. Drives currentTime from the extrapolated
// position each frame, honours the performance budget (stopped when hidden,
// reduced rate unfocused, 30 fps cap in battery saver), tap-to-seek and the
// per-track offset. The same component renders compact (right panel) and
// large (fullscreen).
import { useEffect, useMemo, useRef, useState } from "react";
import { DomLyricPlayer, type LyricLineMouseEvent } from "@applemusic-like-lyrics/core";
import type { Lyrics } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { extrapolate, ticker } from "../store/position";
import { mapLyrics, activeLineIndex, type AmllLine } from "../lib/lyrics-map";
import { bridge } from "../core/bridge";
import { Icon } from "./Icon";

export function LyricsView({ lyrics, variant, showTools = true }: { lyrics: Lyrics; variant: "compact" | "large"; showTools?: boolean }) {
  const stamp = useApp((s) => s.transport.position);
  const clockOffset = useApp((s) => s.connection.clockOffsetMs);
  const duration = useApp((s) => s.nowPlaying?.track.durationMs);
  const perf = useApp((s) => s.perf);
  const batterySaver = useApp((s) => s.batterySaver);
  const fpsCap = useApp((s) => Number(JSON.parse(s.settings["lyrics.fpsCap"]?.value ?? "60")) || 60);
  const host = useRef<HTMLDivElement>(null);
  const player = useRef<DomLyricPlayer | undefined>(undefined);
  const mapped = useMemo(() => mapLyrics(lyrics), [lyrics]);
  const stampRef = useRef(stamp);
  stampRef.current = stamp;

  // Create/destroy the DOM player for synced tiers.
  useEffect(() => {
    if (!mapped.synced || !host.current) return;
    const p = new DomLyricPlayer();
    p.setEnableBlur(variant === "large");
    p.setEnableSpring(variant === "large" && !batterySaver);
    p.setEnableScale(true);
    p.setAlignPosition(variant === "large" ? 0.5 : 0.3);
    p.setWordFadeWidth(0.5);
    host.current.replaceChildren(p.getElement());
    player.current = p;
    const onClick = (e: Event) => {
      const ev = e as LyricLineMouseEvent;
      const line = mapped.lines[ev.lineIndex];
      if (line) bridge().dispatch({ type: "seekTo", data: { position_ms: Math.max(0, line.startTime - mapped.offsetMs) } });
    };
    p.addEventListener("click", onClick);
    return () => {
      p.removeEventListener("click", onClick);
      p.dispose();
      player.current = undefined;
    };
  }, [mapped.synced, variant]);

  useEffect(() => {
    const p = player.current;
    if (!p || !mapped.synced) return;
    p.setEnableSpring(variant === "large" && !batterySaver);
    p.setLyricLines(mapped.lines as never, extrapolate(stampRef.current, Date.now(), clockOffset, duration));
    p.setCurrentTime(extrapolate(stampRef.current, Date.now(), clockOffset, duration), true);
    p.update(0);
  }, [mapped, batterySaver]);

  // Seek detection: a jump in the stamp means isSeek=true so AMLL relayouts.
  const lastStamp = useRef(stamp);
  useEffect(() => {
    const p = player.current;
    if (!p) return;
    const prev = lastStamp.current;
    lastStamp.current = stamp;
    const jumped = Math.abs(extrapolate(stamp, Date.now(), clockOffset, duration) - extrapolate(prev, Date.now(), clockOffset, duration)) > 1500;
    if (jumped) {
      p.setCurrentTime(extrapolate(stamp, Date.now(), clockOffset, duration), true);
      p.update(0);
    }
    if (stamp.isPlaying) p.resume();
    else p.pause();
  }, [stamp, clockOffset, duration]);

  // Frame loop: explicit stop when hidden; ~24 fps in the background; 30 cap in battery saver.
  useEffect(() => {
    if (!mapped.synced || perf === "stopped") return;
    const fps = perf === "background" ? Math.min(24, fpsCap) : batterySaver ? Math.min(30, fpsCap) : fpsCap;
    let raf = 0;
    let last = performance.now();
    let acc = 0;
    const loop = (now: number) => {
      raf = requestAnimationFrame(loop);
      const dt = now - last;
      acc += dt;
      last = now;
      if (acc < 1000 / fps) return;
      const step = acc;
      acc = 0;
      const p = player.current;
      if (!p) return;
      p.setCurrentTime(extrapolate(stampRef.current, Date.now(), clockOffset, duration));
      p.update(step);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [mapped.synced, perf, batterySaver, fpsCap, clockOffset, duration]);

  const tierLabel = lyrics.tier === "syllable" ? t("lyrics.tier.syllable") : lyrics.tier === "line" ? t("lyrics.tier.line") : t("lyrics.tier.unsynced");
  const sourceLabel = lyrics.source === "server" ? t("lyrics.source.server") : lyrics.source === "external" ? t("lyrics.source.external") : t("lyrics.source.embedded");
  return (
    <div className="lyrics-pane" data-testid="lyrics-view" data-tier={lyrics.tier}>
      {mapped.synced ? <div ref={host} className={`amll-host ${variant}`} /> : <StaticLyrics lines={mapped.lines} variant={variant} />}
      {showTools ? <LyricsTools lyrics={lyrics} tierLabel={tierLabel} sourceLabel={sourceLabel} /> : null}
    </div>
  );
}

function StaticLyrics({ lines, variant }: { lines: AmllLine[]; variant: "compact" | "large" }) {
  return (
    <div className="lyrics-static" style={variant === "large" ? { fontSize: 24 } : undefined}>
      {lines.map((l, i) => (
        <div key={i} className={`${l.isBG ? "bg" : ""} ${l.isDuet ? "duet" : ""}`}>{l.words.map((w) => w.word).join("")}</div>
      ))}
    </div>
  );
}

function LyricsTools({ lyrics, tierLabel, sourceLabel }: { lyrics: Lyrics; tierLabel: string; sourceLabel: string }) {
  const [offset, setOffset] = useState(lyrics.offsetMs);
  useEffect(() => setOffset(lyrics.offsetMs), [lyrics.offsetMs, lyrics.trackId]);
  const apply = (v: number) => {
    setOffset(v);
    bridge().dispatch({ type: "setLyricsOffset", data: { track_id: lyrics.trackId, offset_ms: v } });
  };
  return (
    <div className="lyrics-tools" data-testid="lyrics-tools">
      <span title={sourceLabel}>{tierLabel}</span>
      <span>·</span>
      <span>{t("lyrics.offset")}</span>
      <button type="button" className="btn icon sm" aria-label="-100 ms" onClick={() => apply(offset - 100)}><Icon name="minimize" size={10} /></button>
      <span className="mono" style={{ minWidth: 48, textAlign: "center" }}>{offset > 0 ? "+" : ""}{(offset / 1000).toFixed(1)}s</span>
      <button type="button" className="btn icon sm" aria-label="+100 ms" onClick={() => apply(offset + 100)}><Icon name="plus" size={10} /></button>
      {offset !== 0 ? <button type="button" className="btn sm ghost" onClick={() => apply(0)}>{t("lyrics.offsetReset")}</button> : null}
    </div>
  );
}

/** Line index helper for compact views that want a "current line" without AMLL. */
export function useActiveLine(lines: AmllLine[]): number {
  const stamp = useApp((s) => s.transport.position);
  const [idx, setIdx] = useState(-1);
  useEffect(() => ticker.subscribe(() => setIdx(activeLineIndex(lines, extrapolate(stamp, Date.now())))), [lines, stamp]);
  return idx;
}
