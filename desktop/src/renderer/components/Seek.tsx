// The seek bar of the player bar ("bar") and the fullscreen player ("player"): a straight track,
// the played part filled up to a round thumb that grows while hovered, dragged or focused, with a
// value bubble above it while dragging or keyboard-focused and the time under the pointer while
// hovering. The mini player keeps its smaller SeekBar (PlayerBar.tsx); the keys and slider
// semantics are the same.
//
// Painted on the shared position ticker (store/position.ts), which writes the styles directly
// (no React render per frame), only runs while playing or dragging, and keeps to the ticker's
// frame-rate cap.
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { extrapolate, ticker } from "../store/position";
import { bridge } from "../core/bridge";
import { fmtTime } from "../lib/format";
import { seekKeyTarget, seekValueText } from "../lib/a11y";
import { useConnectSyncing } from "../lib/connect-sync";

export function Seek({ durationMs, variant = "player" }: { durationMs: number | undefined; variant?: "player" | "bar" }) {
  const ids = variant === "bar" ? { root: "seek", slider: "seek-slider", position: "seek-position" } : { root: "fs-seek", slider: "fs-seek-slider", position: "fs-seek-position" };
  const stamp = useApp((s) => s.transport.position);
  const offset = useApp((s) => s.connection.clockOffsetMs);
  const syncing = useConnectSyncing();
  const [drag, setDrag] = useState<number | undefined>(undefined);
  const [hover, setHover] = useState<number | undefined>(undefined);
  const [focused, setFocused] = useState(false);
  const bar = useRef<HTMLDivElement>(null);
  const played = useRef<HTMLDivElement>(null);
  const thumb = useRef<HTMLDivElement>(null);
  const timeEl = useRef<HTMLSpanElement>(null);
  const remainEl = useRef<HTMLSpanElement>(null);
  const bubbleEl = useRef<HTMLDivElement>(null);
  const shownRef = useRef(0);
  const dragRef = useRef(drag);
  dragRef.current = drag;
  const dur = durationMs ?? 0;

  const frac = (x: number) => {
    const r = bar.current?.getBoundingClientRect();
    if (!r || !r.width) return 0;
    return Math.max(0, Math.min(1, (x - r.left) / r.width));
  };
  const commit = useCallback((f: number) => bridge().dispatch({ type: "seekTo", data: { position_ms: Math.round(f * dur) } }), [dur]);
  const dragging = drag !== undefined;
  useEffect(() => {
    if (!dragging) return;
    const move = (e: PointerEvent) => setDrag(frac(e.clientX));
    const up = (e: PointerEvent) => { commit(frac(e.clientX)); setDrag(undefined); };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    return () => { window.removeEventListener("pointermove", move); window.removeEventListener("pointerup", up); };
  }, [dragging, commit]);

  // What paint last wrote, per attribute: a write is skipped when the value is unchanged, so the
  // labels and the slider's value (which assistive tech follows) change once a second, not every
  // frame. Cleared whenever React may have rendered those elements afresh (the effect below).
  const written = useRef(new Map<string, string>());
  const put = (key: string, value: string, write: (v: string) => void) => {
    if (written.current.get(key) === value) return;
    written.current.set(key, value);
    write(value);
  };
  const paint = () => {
    const shown = dragRef.current !== undefined ? dragRef.current * dur : extrapolate(stamp, Date.now(), offset, durationMs);
    shownRef.current = shown;
    const pct = `${(dur ? Math.max(0, Math.min(1, shown / dur)) : 0) * 100}%`;
    put("pct", pct, (v) => {
      if (played.current) played.current.style.width = v;
      if (thumb.current) thumb.current.style.left = v;
      if (bubbleEl.current) bubbleEl.current.style.left = v;
    });
    const seconds = Math.floor(shown / 1000);
    put("value", String(seconds), (v) => {
      bar.current?.setAttribute("aria-valuenow", v);
      bar.current?.setAttribute("aria-valuetext", seekValueText(seconds * 1000, dur));
    });
    const time = fmtTime(shown);
    put("time", time, (v) => {
      if (timeEl.current) timeEl.current.textContent = v;
      if (bubbleEl.current) bubbleEl.current.textContent = v;
    });
    put("remain", `-${fmtTime(Math.max(0, dur - shown))}`, (v) => { if (remainEl.current) remainEl.current.textContent = v; });
  };

  const onKey = (e: React.KeyboardEvent) => {
    if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
    const to = seekKeyTarget(e.key, shownRef.current, dur);
    if (to === undefined) return;
    e.preventDefault();
    e.stopPropagation();
    bridge().dispatch({ type: "seekTo", data: { position_ms: Math.round(to) } });
  };

  useLayoutEffect(() => {
    written.current.clear();
    paint();
    if (!stamp.isPlaying && !dragging) return;
    return ticker.subscribe(paint);
  }, [stamp, durationMs, offset, dur, dragging, focused]); // eslint-disable-line react-hooks/exhaustive-deps

  const bubble = dragging || focused;
  const slider = (
    <div ref={bar} className={`np-seek-bar ${dragging ? "dragging" : ""}`} role="slider" aria-label={t("player.seek")} aria-valuemin={0} aria-valuemax={Math.floor(dur / 1000)} aria-valuenow={0} aria-valuetext={seekValueText(0, dur)} aria-disabled={!dur || undefined} tabIndex={dur ? 0 : -1}
      onKeyDown={onKey}
      onFocus={(e) => setFocused(e.currentTarget.matches(":focus-visible"))}
      onBlur={() => setFocused(false)}
      onPointerDown={(e) => { if (!dur || e.button !== 0) return; e.preventDefault(); e.currentTarget.focus({ preventScroll: true }); setDrag(frac(e.clientX)); }}
      onPointerMove={(e) => setHover(frac(e.clientX))}
      onPointerLeave={() => setHover(undefined)}
      data-testid={ids.slider}>
      <div className="rest" aria-hidden="true"><div ref={played} className="played" style={{ width: 0 }} /></div>
      <div ref={thumb} className="thumb" aria-hidden="true" style={{ left: 0 }} />
      {hover !== undefined && dur && !dragging ? <div className="hover-time" style={{ left: `${hover * 100}%` }}>{fmtTime(hover * dur)}</div> : null}
      {bubble && dur ? <div ref={bubbleEl} className="value-bubble" style={{ left: 0 }}>{fmtTime(0)}</div> : null}
    </div>
  );
  if (variant === "bar") {
    return (
      <div className={`np-seek bar-seek ${syncing ? "connect-syncing" : ""}`} aria-busy={syncing || undefined} data-testid={ids.root}>
        <span ref={timeEl} className="time" data-testid={ids.position} aria-hidden="true">{fmtTime(0)}</span>
        {slider}
        <span className="time" aria-hidden="true">{fmtTime(dur)}</span>
      </div>
    );
  }
  return (
    <div className={`np-seek ${syncing ? "connect-syncing" : ""}`} aria-busy={syncing || undefined} data-testid={ids.root}>
      {slider}
      <div className="np-seek-times" aria-hidden="true">
        <span ref={timeEl} className="time" data-testid={ids.position}>{fmtTime(0)}</span>
        <span ref={remainEl} className="time">-{fmtTime(dur)}</span>
      </div>
    </div>
  );
}
