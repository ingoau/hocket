// "Lyrics options": what kind of sync the lyrics have and where they came
// from, the per-track offset (earlier / later / reset) and the device-local
// lyrics size, in a small popover rather than a row laid over the lyrics.
// The side panel's lyrics view has a tune button in its header (the popover
// drops below it). The lyrics toggles (the player bar's, the fullscreen pill's)
// open it on a right-click, or the context-menu key or Shift+F10, anchored to
// them (it rises above), so nothing is added beside them; a click still
// toggles the lyrics. Keyboard: focus moves in on open, Tab moves through the
// controls, Escape closes and focus goes back to what opened it. Closing plays
// an exit animation.
import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { usePresence } from "../lib/presence";
import { LyricsSizeControl } from "./LyricsView";
import { Icon } from "./Icon";

const WIDTH = 300;

/** The current song's lyrics, when it has any. */
function useCurrentLyrics() {
  return useApp((s) => (s.lyrics && s.nowPlaying && s.lyrics.trackId === s.nowPlaying.track.id ? s.lyrics.lyrics : undefined));
}

/**
 * The options popover, anchored to whichever element opened it: `below` it, right edges lined up,
 * or `above` it. `open(el)` shows it there; it closes on Escape, a click outside, or a song
 * without lyrics.
 */
function useLyricsOptionsPopover(placement: "below" | "above") {
  const lyrics = useCurrentLyrics();
  const [anchor, setAnchor] = useState<HTMLElement | undefined>(undefined);
  const [at, setAt] = useState<{ left: number; top?: number; bottom?: number } | undefined>(undefined);
  const pop = useRef<HTMLDivElement>(null);
  const presence = usePresence(anchor && lyrics ? at : undefined, pop);
  const shown = presence.value;

  const close = (refocus = true) => {
    const el = anchor;
    setAnchor(undefined);
    setAt(undefined);
    if (refocus) el?.focus({ preventScroll: true });
  };
  useLayoutEffect(() => {
    if (!anchor) return;
    const place = () => {
      const r = anchor.getBoundingClientRect();
      const left = Math.max(8, Math.min(r.right - WIDTH, window.innerWidth - WIDTH - 8));
      setAt(placement === "below" ? { left, top: r.bottom + 6 } : { left, bottom: window.innerHeight - r.top + 12 });
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [anchor, placement]);
  const placed = !!at;
  useEffect(() => {
    if (anchor && placed) pop.current?.querySelector<HTMLElement>("button")?.focus({ preventScroll: true });
  }, [anchor, placed]);
  useEffect(() => {
    if (!anchor) return;
    const down = (e: MouseEvent) => {
      const target = e.target as Node;
      if (pop.current?.contains(target) || anchor.contains(target)) return;
      setAnchor(undefined);
      setAt(undefined);
    };
    window.addEventListener("mousedown", down, true);
    return () => window.removeEventListener("mousedown", down, true);
  }, [anchor]);
  // A new song without lyrics: nothing to set.
  useEffect(() => {
    if (!lyrics && anchor) {
      setAnchor(undefined);
      setAt(undefined);
    }
  }, [lyrics, anchor]);

  const host = anchor?.closest<HTMLElement>('[role="dialog"]') ?? document.body;
  const popover: ReactNode = shown
    ? createPortal(
        <div {...presence.exitProps} className={`lyrics-options-pop ${placement} ${presence.motion}`} role="dialog" aria-label={t("lyrics.options")} style={{ left: shown.left, top: shown.top, bottom: shown.bottom, width: WIDTH }}
          onKeyDown={(e) => { if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); close(); } }} data-testid="lyrics-tools">
          <LyricsOptionsPanel />
        </div>,
        host,
      )
    : null;
  return { lyrics, anchor, open: (el: HTMLElement) => setAnchor(el), close, popover };
}

/** The side panel's lyrics view header button. */
export function LyricsOptionsButton({ className, placement, iconSize = 22 }: { className: string; placement: "below" | "above"; iconSize?: number }) {
  const { lyrics, anchor, open, close, popover } = useLyricsOptionsPopover(placement);
  const button = useRef<HTMLButtonElement>(null);
  if (!lyrics) return null;
  const isOpen = !!anchor;
  return (
    <>
      <button ref={button} type="button" className={className} aria-haspopup="dialog" aria-expanded={isOpen} aria-label={t("lyrics.options")} title={t("lyrics.options")}
        onClick={() => (isOpen ? close(false) : button.current && open(button.current))} onKeyDown={(e) => { if (e.key === "Escape" && isOpen) { e.preventDefault(); e.stopPropagation(); close(); } }} data-testid="lyrics-options">
        <Icon name="tune" size={iconSize} />
      </button>
      {popover}
    </>
  );
}

/**
 * The lyrics options behind a lyrics toggle: spread `trigger` onto the toggle (a right-click, the
 * context-menu key or Shift+F10 open the options; a click still toggles the lyrics) and render
 * `extra` beside it (the popover and the toggle's description).
 */
export function useLyricsOptionsTrigger(placement: "below" | "above") {
  const { lyrics, anchor, open, close, popover } = useLyricsOptionsPopover(placement);
  const hintId = useId();
  const trigger = {
    "aria-describedby": lyrics ? hintId : undefined,
    "aria-haspopup": lyrics ? ("dialog" as const) : undefined,
    onContextMenu: (e: ReactMouseEvent<HTMLElement>) => {
      e.preventDefault();
      e.stopPropagation();
      if (lyrics) open(e.currentTarget);
    },
    onKeyDown: (e: KeyboardEvent<HTMLElement>) => {
      if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
        e.preventDefault();
        e.stopPropagation();
        if (lyrics) open(e.currentTarget);
      } else if (e.key === "Escape" && anchor) {
        e.preventDefault();
        e.stopPropagation();
        close();
      }
    },
  };
  const extra = (
    <>
      {lyrics ? <span id={hintId} hidden>{t("lyrics.optionsHint")}</span> : null}
      {popover}
    </>
  );
  return { trigger, extra };
}

function LyricsOptionsPanel() {
  const lyrics = useApp((s) => s.lyrics?.lyrics);
  const [offset, setOffset] = useState(lyrics?.offsetMs ?? 0);
  useEffect(() => setOffset(lyrics?.offsetMs ?? 0), [lyrics?.offsetMs, lyrics?.trackId]);
  if (!lyrics) return null;
  const apply = (v: number) => {
    setOffset(v);
    bridge().dispatch({ type: "setLyricsOffset", data: { track_id: lyrics.trackId, offset_ms: v } });
  };
  const tierLabel = lyrics.tier === "syllable" ? t("lyrics.tier.syllable") : lyrics.tier === "line" ? t("lyrics.tier.line") : t("lyrics.tier.unsynced");
  const sourceLabel = lyrics.source === "server" ? t("lyrics.source.server") : lyrics.source === "external" ? t("lyrics.source.external") : t("lyrics.source.embedded");
  return (
    <>
      <div className="lo-head">
        <div className="lo-title">{tierLabel}</div>
        <div className="lo-sub">{sourceLabel}</div>
      </div>
      <div className="lo-row" role="group" aria-labelledby="lo-offset">
        <span className="lo-label" id="lo-offset">{t("lyrics.offset")}</span>
        <span className="lo-stepper">
          <button type="button" className="lo-btn" aria-label={t("a11y.lyricsEarlier")} title={t("a11y.lyricsEarlier")} onClick={() => apply(offset - 100)} data-testid="lyrics-offset-earlier"><Icon name="remove" size={18} /></button>
          <span className="lo-value mono" aria-live="polite" data-testid="lyrics-offset-value">{offset > 0 ? "+" : ""}{(offset / 1000).toFixed(1)}s</span>
          <button type="button" className="lo-btn" aria-label={t("a11y.lyricsLater")} title={t("a11y.lyricsLater")} onClick={() => apply(offset + 100)} data-testid="lyrics-offset-later"><Icon name="plus" size={18} /></button>
        </span>
        {offset !== 0 ? <button type="button" className="lo-reset" onClick={() => apply(0)} data-testid="lyrics-offset-reset">{t("lyrics.offsetReset")}</button> : null}
      </div>
      <div className="lo-row">
        <span className="lo-label">{t("lyrics.size")}</span>
        <LyricsSizeControl />
      </div>
    </>
  );
}
