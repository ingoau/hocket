// "Lyrics options": what kind of sync the lyrics have and where they came
// from, the per-track offset (earlier / later / reset) and the device-local
// lyrics size, in a small popover behind an icon button, rather than a row
// laid over the lyrics. The side panel's lyrics view has the button in its
// header (the popover drops below it); the fullscreen player's pill gains it
// while the lyrics are showing (the popover rises above it, like volume).
// Keyboard: focus moves in on open, Tab moves through the controls, Escape
// closes and focus goes back to the button. Closing plays an exit animation.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { usePresence } from "../lib/presence";
import { LyricsSizeControl } from "./LyricsView";
import { Icon } from "./Icon";

const WIDTH = 300;

export function LyricsOptionsButton({ className, placement, iconSize = 22 }: { className: string; placement: "below" | "above"; iconSize?: number }) {
  const lyrics = useApp((s) => (s.lyrics && s.nowPlaying && s.lyrics.trackId === s.nowPlaying.track.id ? s.lyrics.lyrics : undefined));
  const [open, setOpen] = useState(false);
  const [at, setAt] = useState<{ left: number; top?: number; bottom?: number } | undefined>(undefined);
  const button = useRef<HTMLButtonElement>(null);
  const pop = useRef<HTMLDivElement>(null);
  const presence = usePresence(open && lyrics ? at : undefined, pop);
  const shown = presence.value;

  const close = (refocus = true) => {
    setOpen(false);
    setAt(undefined);
    if (refocus) button.current?.focus({ preventScroll: true });
  };
  // Lined up with the button's right edge; below it in the side panel, above it in the fullscreen pill.
  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      const r = button.current?.getBoundingClientRect();
      if (!r) return;
      const left = Math.max(8, Math.min(r.right - WIDTH, window.innerWidth - WIDTH - 8));
      setAt(placement === "below" ? { left, top: r.bottom + 6 } : { left, bottom: window.innerHeight - r.top + 12 });
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [open, placement]);
  const placed = !!at;
  useEffect(() => {
    if (open && placed) pop.current?.querySelector<HTMLElement>("button")?.focus({ preventScroll: true });
  }, [open, placed]);
  useEffect(() => {
    if (!open) return;
    const down = (e: MouseEvent) => {
      const target = e.target as Node;
      if (pop.current?.contains(target) || button.current?.contains(target)) return;
      setOpen(false);
      setAt(undefined);
    };
    window.addEventListener("mousedown", down, true);
    return () => window.removeEventListener("mousedown", down, true);
  }, [open]);
  // A new song without lyrics: nothing to set.
  useEffect(() => {
    if (!lyrics && open) {
      setOpen(false);
      setAt(undefined);
    }
  }, [lyrics, open]);

  if (!lyrics) return null;
  const host = button.current?.closest<HTMLElement>('[role="dialog"]') ?? document.body;
  return (
    <>
      <button ref={button} type="button" className={className} aria-haspopup="dialog" aria-expanded={open} aria-label={t("lyrics.options")} title={t("lyrics.options")}
        onClick={() => (open ? close(false) : setOpen(true))} onKeyDown={(e) => { if (e.key === "Escape" && open) { e.preventDefault(); e.stopPropagation(); close(); } }} data-testid="lyrics-options">
        <Icon name="tune" size={iconSize} />
      </button>
      {shown ? createPortal(
        <div {...presence.exitProps} className={`lyrics-options-pop ${placement} ${presence.motion}`} role="dialog" aria-label={t("lyrics.options")} style={{ left: shown.left, top: shown.top, bottom: shown.bottom, width: WIDTH }}
          onKeyDown={(e) => { if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); close(); } }} data-testid="lyrics-tools">
          <LyricsOptionsPanel />
        </div>,
        host,
      ) : null}
    </>
  );
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
