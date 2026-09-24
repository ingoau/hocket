// The fullscreen player's seek bar: Material 3 Expressive's wavy progress.
// The played part is a travelling sine wave that flattens to a line while
// paused (and stands still with reduced motion); the rest is a flat track;
// the thumb is a short vertical bar. Same slider semantics and keys as the
// player bar's SeekBar.
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { usePosition } from "../store/position";
import { bridge } from "../core/bridge";
import { fmtTime } from "../lib/format";
import { seekKeyTarget, seekValueText } from "../lib/a11y";

const WAVELENGTH = 32;
const AMPLITUDE = 3;
const HEIGHT = 28;
/** Gap either side of the thumb. */
const GAP = 6;

/** A sine wave from x0 to x1 around y = mid, as a path of quadratic segments (two per wavelength). */
export function wavePath(x0: number, x1: number, mid: number, amplitude = AMPLITUDE, wavelength = WAVELENGTH): string {
  const half = wavelength / 2;
  // Start on a crest boundary so the wave lines up however far it is drawn.
  const start = Math.floor(x0 / wavelength) * wavelength;
  let d = `M ${start} ${mid}`;
  let up = true;
  for (let x = start; x < x1; x += half) {
    d += ` Q ${x + half / 2} ${mid + (up ? -2 : 2) * amplitude} ${x + half} ${mid}`;
    up = !up;
  }
  return d;
}

export function WavySeek({ durationMs, playing }: { durationMs: number | undefined; playing: boolean }) {
  const stamp = useApp((s) => s.transport.position);
  const offset = useApp((s) => s.connection.clockOffsetMs);
  const pos = usePosition(stamp, durationMs, offset);
  const [drag, setDrag] = useState<number | undefined>(undefined);
  const [hover, setHover] = useState<number | undefined>(undefined);
  const [width, setWidth] = useState(0);
  const bar = useRef<HTMLDivElement>(null);
  const dur = durationMs ?? 0;

  useLayoutEffect(() => {
    const el = bar.current;
    if (!el) return;
    setWidth(el.getBoundingClientRect().width);
    const ro = new ResizeObserver(() => setWidth(el.getBoundingClientRect().width));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

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

  const shown = drag !== undefined ? drag * dur : pos;
  const f = dur ? Math.max(0, Math.min(1, shown / dur)) : 0;
  const seconds = Math.floor(shown / 1000);
  const onKey = (e: React.KeyboardEvent) => {
    if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
    const to = seekKeyTarget(e.key, shown, dur);
    if (to === undefined) return;
    e.preventDefault();
    e.stopPropagation();
    bridge().dispatch({ type: "seekTo", data: { position_ms: Math.round(to) } });
  };

  const x = f * width;
  const mid = HEIGHT / 2;
  const playedEnd = Math.max(0, x - GAP);
  const restStart = Math.min(width, x + GAP);
  // The wave starts a wavelength before 0: it travels right by up to a
  // wavelength (CSS), and the clip keeps it to the played part.
  const clipId = "np-seek-clip";
  return (
    <div className="np-seek" data-testid="fs-seek">
      <div ref={bar} className={`np-seek-bar ${dragging ? "dragging" : ""} ${playing && !dragging ? "playing" : ""}`} role="slider" aria-label={t("player.seek")} aria-valuemin={0} aria-valuemax={Math.floor(dur / 1000)} aria-valuenow={seconds} aria-valuetext={seekValueText(seconds * 1000, dur)} aria-disabled={!dur || undefined} tabIndex={dur ? 0 : -1}
        onKeyDown={onKey}
        onPointerDown={(e) => { if (!dur || e.button !== 0) return; e.preventDefault(); e.currentTarget.focus({ preventScroll: true }); setDrag(frac(e.clientX)); }}
        onPointerMove={(e) => setHover(frac(e.clientX))}
        onPointerLeave={() => setHover(undefined)}
        data-testid="fs-seek-slider">
        {width ? (
          <svg width={width} height={HEIGHT} viewBox={`0 0 ${width} ${HEIGHT}`} aria-hidden="true" focusable="false">
            <defs><clipPath id={clipId}><rect x={0} y={0} width={playedEnd} height={HEIGHT} /></clipPath></defs>
            {restStart < width ? <line className="rest" x1={restStart} y1={mid} x2={width} y2={mid} /> : null}
            <g clipPath={`url(#${clipId})`}>
              <g className="wave">
                <path className="played" d={wavePath(-WAVELENGTH, playedEnd, mid)} />
              </g>
            </g>
            <rect className="thumb" x={x - 2} y={mid - 10} width={4} height={20} rx={2} />
          </svg>
        ) : null}
        {hover !== undefined && dur ? <div className="hover-time" style={{ left: `${hover * 100}%` }}>{fmtTime(hover * dur)}</div> : null}
      </div>
      <div className="np-seek-times" aria-hidden="true">
        <span className="time" data-testid="fs-seek-position">{fmtTime(shown)}</span>
        <span className="time">{fmtTime(dur)}</span>
      </div>
    </div>
  );
}
