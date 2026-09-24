// AMLL lyric renderer wrapper. Drives currentTime from the extrapolated
// position each frame, honours the performance budget (stopped when hidden,
// reduced rate unfocused, 30 fps cap in battery saver), tap-to-seek and the
// per-track offset. The same component renders compact (right panel) and
// large (fullscreen).
//
// Accessibility: the AMLL canvas of word spans is decorative (aria-hidden);
// screen readers get a labelled list of whole lines with the active line
// marked aria-current, and nothing here is a live region (no announcement per
// line or syllable). With reduced motion, more contrast or forced colours the
// synced lyrics render as that same list, visible, with static highlighting:
// no syllable sweep, blur, springs or scaling.
import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { DomLyricPlayer, type LyricLineMouseEvent } from "@applemusic-like-lyrics/core";
import type { Lyrics } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { extrapolate, ticker } from "../store/position";
import { mapLyrics, activeLineIndex, type AmllLine } from "../lib/lyrics-map";
import { bridge } from "../core/bridge";
import { LYRICS_SCALE, LYRICS_SIZES, setLyricsSize, useLyricsSize, type LyricsSize } from "../lib/lyrics-size";
import { Icon } from "./Icon";
import { SK } from "@shared/settings-keys";
import { usePlainLyrics } from "../lib/media";

export function LyricsView({ lyrics, variant, showTools = true }: { lyrics: Lyrics; variant: "compact" | "large"; showTools?: boolean }) {
  const stamp = useApp((s) => s.transport.position);
  const clockOffset = useApp((s) => s.connection.clockOffsetMs);
  const duration = useApp((s) => s.nowPlaying?.track.durationMs);
  const perf = useApp((s) => s.perf);
  const batterySaver = useApp((s) => s.batterySaver);
  const fpsCap = useApp((s) => Number(JSON.parse(s.settings[SK.displayLyricsFps]?.value ?? "60")) || 60);
  const batteryFps = useApp((s) => Number(JSON.parse(s.settings[SK.batteryLyricsFps]?.value ?? "30")) || 30);
  const host = useRef<HTMLDivElement>(null);
  const player = useRef<DomLyricPlayer | undefined>(undefined);
  const mapped = useMemo(() => mapLyrics(lyrics), [lyrics]);
  const size = useLyricsSize();
  const plain = usePlainLyrics();
  const animated = mapped.synced && !plain;
  const stampRef = useRef(stamp);
  stampRef.current = stamp;

  // Create/destroy the DOM player for synced tiers.
  useEffect(() => {
    if (!animated || !host.current) return;
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
  }, [animated, variant]);

  useEffect(() => {
    const p = player.current;
    if (!p || !animated) return;
    p.setEnableSpring(variant === "large" && !batterySaver);
    p.setLyricLines(mapped.lines as never, extrapolate(stampRef.current, Date.now(), clockOffset, duration));
    p.setCurrentTime(extrapolate(stampRef.current, Date.now(), clockOffset, duration), true);
    p.update(0);
  }, [mapped, batterySaver, animated]);

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
    if (!animated || perf === "stopped") return;
    const fps = perf === "background" ? Math.min(24, fpsCap) : batterySaver ? Math.min(batteryFps, fpsCap) : fpsCap;
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
  }, [animated, perf, batterySaver, fpsCap, batteryFps, clockOffset, duration]);

  const tierLabel = lyrics.tier === "syllable" ? t("lyrics.tier.syllable") : lyrics.tier === "line" ? t("lyrics.tier.line") : t("lyrics.tier.unsynced");
  const sourceLabel = lyrics.source === "server" ? t("lyrics.source.server") : lyrics.source === "external" ? t("lyrics.source.external") : t("lyrics.source.embedded");
  return (
    <div className="lyrics-pane" data-testid="lyrics-view" data-tier={lyrics.tier} data-mode={animated ? "animated" : "plain"}>
      {animated ? (
        <>
          <div ref={host} className={`amll-host ${variant}`} style={{ "--lyrics-scale": LYRICS_SCALE[size] } as CSSProperties} aria-hidden="true" data-testid="amll-host" data-size={size} />
          <LyricsList lines={mapped.lines} synced visuallyHidden variant={variant} size={LYRICS_SCALE[size]} />
        </>
      ) : (
        <LyricsList lines={mapped.lines} synced={mapped.synced} variant={variant} size={LYRICS_SCALE[size]} />
      )}
      {showTools ? <LyricsTools lyrics={lyrics} tierLabel={tierLabel} sourceLabel={sourceLabel} /> : null}
    </div>
  );
}

/**
 * The lyrics as a list of whole lines (focusable when shown, so the keyboard
 * can scroll it). Synced: the active line carries
 * aria-current and (when shown) a static highlight, scrolled to without
 * animation. Background vocals are sub-lines of the line before them.
 */
function LyricsList({ lines, synced, visuallyHidden = false, variant, size }: { lines: AmllLine[]; synced: boolean; visuallyHidden?: boolean; variant: "compact" | "large"; size: number }) {
  const active = useActiveLine(synced ? lines : NO_LINES);
  const ref = useRef<HTMLOListElement>(null);
  useEffect(() => {
    if (visuallyHidden || active < 0) return;
    ref.current?.querySelector<HTMLElement>(`[data-line="${active}"]`)?.scrollIntoView({ block: "center", behavior: "auto" });
  }, [active, visuallyHidden]);
  const bgText = (i: number) => lines[i]?.words.map((w) => w.word).join("").trim() ?? "";
  // A background line belongs to the active main line it follows.
  let owner = -1;
  const owners = lines.map((l, i) => (l.isBG ? owner : (owner = i)));
  return (
    <ol ref={ref} className={visuallyHidden ? "sr-only" : `lyrics-static ${variant}`} style={visuallyHidden ? undefined : { fontSize: `calc(${variant === "large" ? 24 : 15}px * ${size})` }} aria-label={t("a11y.lyricsList")} tabIndex={visuallyHidden ? undefined : 0} data-testid={visuallyHidden ? "lyrics-sr-list" : "lyrics-plain"}>
      {lines.map((l, i) => {
        const current = synced && (i === active || (l.isBG && owners[i] === active));
        return (
          <li key={i} data-line={i} className={`${l.isBG ? "bg" : ""} ${l.isDuet ? "duet" : ""} ${current ? "active" : ""}`} aria-current={current && !l.isBG ? "true" : undefined}>
            {l.isBG ? <><span className="sr-only">{t("a11y.backgroundVocal")}</span>{bgText(i)}</> : l.words.map((w) => w.word).join("")}
          </li>
        );
      })}
    </ol>
  );
}

const NO_LINES: AmllLine[] = [];

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
      <span aria-hidden="true">·</span>
      <span>{t("lyrics.offset")}</span>
      <button type="button" className="btn icon sm" aria-label={t("a11y.lyricsEarlier")} title={t("a11y.lyricsEarlier")} onClick={() => apply(offset - 100)}><Icon name="minimize" size={10} /></button>
      <span className="mono" style={{ minWidth: 48, textAlign: "center" }}>{offset > 0 ? "+" : ""}{(offset / 1000).toFixed(1)}s</span>
      <button type="button" className="btn icon sm" aria-label={t("a11y.lyricsLater")} title={t("a11y.lyricsLater")} onClick={() => apply(offset + 100)}><Icon name="plus" size={10} /></button>
      {offset !== 0 ? <button type="button" className="btn sm ghost" onClick={() => apply(0)}>{t("lyrics.offsetReset")}</button> : null}
      <span aria-hidden="true">·</span>
      <LyricsSizeControl />
    </div>
  );
}

/** Small / medium / large, device-local; shared by every lyrics view in this window. */
export function LyricsSizeControl() {
  const size = useLyricsSize();
  const onKey = (e: React.KeyboardEvent) => {
    const i = LYRICS_SIZES.indexOf(size);
    const d = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
    if (!d) return;
    e.preventDefault();
    e.stopPropagation();
    const next = LYRICS_SIZES[(i + d + LYRICS_SIZES.length) % LYRICS_SIZES.length] as LyricsSize;
    setLyricsSize(next);
    (e.currentTarget as HTMLElement).querySelector<HTMLElement>(`[data-testid="lyrics-size-${next}"]`)?.focus();
  };
  return (
    <span className="lyrics-size" role="radiogroup" aria-label={t("lyrics.size")} onKeyDown={onKey} data-testid="lyrics-size">
      {LYRICS_SIZES.map((s) => (
        <button key={s} type="button" role="radio" aria-checked={size === s} tabIndex={size === s ? 0 : -1} className={`btn icon sm ${size === s ? "on" : ""}`} title={t(`lyrics.size.${s}` as never)} aria-label={t(`lyrics.size.${s}` as never)} onClick={() => setLyricsSize(s)} data-testid={`lyrics-size-${s}`}>
          <span aria-hidden="true">{s === "small" ? "S" : s === "medium" ? "M" : "L"}</span>
        </button>
      ))}
    </span>
  );
}

/** Line index helper for compact views that want a "current line" without AMLL. */
export function useActiveLine(lines: AmllLine[]): number {
  const stamp = useApp((s) => s.transport.position);
  const [idx, setIdx] = useState(-1);
  const offset = useApp((s) => s.connection.clockOffsetMs);
  useEffect(() => {
    const tick = () => setIdx(activeLineIndex(lines, extrapolate(stamp, Date.now(), offset)));
    tick();
    return ticker.subscribe(tick);
  }, [lines, stamp, offset]);
  return idx;
}
