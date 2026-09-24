// The fullscreen player's seek bar: Material 3 Expressive's wavy progress.
//
// - The played part is a thick, round-capped sine wave travelling forward.
//   Its amplitude is a spring: it swells when playback starts and settles
//   flat when it pauses (or buffers). The wave tapers in from the start and
//   into the thumb, so it never meets either with a kink.
// - A gap either side of the thumb; the rest of the track is flat and dimmed,
//   with a stop indicator dot at its end.
// - The thumb is a rounded vertical bar that stretches while hovered or
//   dragged, with a value bubble above it while dragging or keyboard-focused.
// - Reduced motion: the wave neither travels nor springs (it is simply wavy
//   while playing and flat when paused).
//
// Same slider semantics and keys as the player bar's SeekBar. The wave is
// drawn by a requestAnimationFrame loop that writes the path directly (no
// React render per frame) and only runs while something is moving.
import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { usePosition } from "../store/position";
import { bridge } from "../core/bridge";
import { fmtTime } from "../lib/format";
import { seekKeyTarget, seekValueText } from "../lib/a11y";
import { usePrefersReducedMotion } from "../lib/media";

export const WAVE = {
  wavelength: 44,
  amplitude: 4.5,
  stroke: 6,
  height: 40,
  /** Gap either side of the thumb. */
  gap: 7,
  /** Seconds for the wave to travel one wavelength. */
  period: 1.6,
};
// Amplitude spring (unit mass): a little overshoot when it swells.
const STIFFNESS = 220;
const DAMPING = 2 * 0.65 * Math.sqrt(STIFFNESS);

const smooth = (v: number) => {
  const c = Math.max(0, Math.min(1, v));
  return c * c * (3 - 2 * c);
};

/**
 * The played wave from x0 to x1 around `mid`: a polyline sampled every 2 px,
 * its amplitude tapering to zero over the first wavelength and the last half
 * wavelength. `phase` (px) moves the crests forward.
 */
export function wavePath(x0: number, x1: number, mid: number, amplitude: number, phase = 0, wavelength = WAVE.wavelength): string {
  if (x1 <= x0) return "";
  const k = (2 * Math.PI) / wavelength;
  const y = (x: number) => {
    const taper = Math.min(smooth((x - x0) / wavelength), smooth((x1 - x) / (wavelength / 2)));
    return mid - amplitude * taper * Math.sin(k * (x - phase));
  };
  let d = `M ${x0.toFixed(1)} ${y(x0).toFixed(2)}`;
  for (let x = x0 + 2; x < x1; x += 2) d += ` L ${x.toFixed(1)} ${y(x).toFixed(2)}`;
  return `${d} L ${x1.toFixed(1)} ${y(x1).toFixed(2)}`;
}

export function WavySeek({ durationMs, playing }: { durationMs: number | undefined; playing: boolean }) {
  const stamp = useApp((s) => s.transport.position);
  const offset = useApp((s) => s.connection.clockOffsetMs);
  const stopped = useApp((s) => s.perf === "stopped");
  const pos = usePosition(stamp, durationMs, offset);
  const reducedMotion = usePrefersReducedMotion();
  const [drag, setDrag] = useState<number | undefined>(undefined);
  const [hover, setHover] = useState<number | undefined>(undefined);
  const [focused, setFocused] = useState(false);
  const [width, setWidth] = useState(0);
  const bar = useRef<HTMLDivElement>(null);
  const path = useRef<SVGPathElement>(null);
  const clipId = useId();
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

  const { stroke, gap, height } = WAVE;
  const mid = height / 2;
  const x0 = stroke / 2;
  const xEnd = Math.max(x0, width - stroke / 2);
  const x = x0 + f * (xEnd - x0);
  const playedEnd = x - gap;
  const restStart = x + gap;
  const waving = playing && !dragging;

  // The wave's live state, read by the frame loop and by render.
  const live = useRef({ amp: waving ? WAVE.amplitude : 0, vel: 0, phase: 0, x0, x1: playedEnd, mid });
  live.current.x0 = x0;
  live.current.x1 = playedEnd;
  live.current.mid = mid;
  if (reducedMotion) {
    live.current.amp = waving ? WAVE.amplitude : 0;
    live.current.vel = 0;
  }
  const d = wavePath(x0, playedEnd, mid, live.current.amp, live.current.phase);

  useEffect(() => {
    if (reducedMotion || stopped) return;
    const target = waving ? WAVE.amplitude : 0;
    const s = live.current;
    if (!waving && Math.abs(s.amp) < 0.01 && Math.abs(s.vel) < 0.01) return;
    let raf = 0;
    let last = performance.now();
    const frame = (now: number) => {
      const dt = Math.min(0.05, (now - last) / 1000);
      last = now;
      s.vel += (-STIFFNESS * (s.amp - target) - DAMPING * s.vel) * dt;
      s.amp += s.vel * dt;
      s.phase = (s.phase + (WAVE.wavelength / WAVE.period) * dt) % WAVE.wavelength;
      path.current?.setAttribute("d", wavePath(s.x0, s.x1, s.mid, Math.max(0, s.amp), s.phase));
      const settled = !waving && Math.abs(s.amp) < 0.01 && Math.abs(s.vel) < 0.01;
      if (settled) {
        s.amp = 0;
        s.vel = 0;
        path.current?.setAttribute("d", wavePath(s.x0, s.x1, s.mid, 0, s.phase));
        return;
      }
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, [waving, reducedMotion, stopped]);

  const bubble = dragging || focused;
  return (
    <div className="np-seek" data-testid="fs-seek">
      <div ref={bar} className={`np-seek-bar ${dragging ? "dragging" : ""} ${waving ? "playing" : ""}`} role="slider" aria-label={t("player.seek")} aria-valuemin={0} aria-valuemax={Math.floor(dur / 1000)} aria-valuenow={seconds} aria-valuetext={seekValueText(seconds * 1000, dur)} aria-disabled={!dur || undefined} tabIndex={dur ? 0 : -1}
        onKeyDown={onKey}
        onFocus={(e) => setFocused(e.currentTarget.matches(":focus-visible"))}
        onBlur={() => setFocused(false)}
        onPointerDown={(e) => { if (!dur || e.button !== 0) return; e.preventDefault(); e.currentTarget.focus({ preventScroll: true }); setDrag(frac(e.clientX)); }}
        onPointerMove={(e) => setHover(frac(e.clientX))}
        onPointerLeave={() => setHover(undefined)}
        data-testid="fs-seek-slider">
        {width ? (
          <svg width={width} height={height} viewBox={`0 0 ${width} ${height}`} aria-hidden="true" focusable="false">
            <defs><clipPath id={clipId}><rect x={0} y={0} width={Math.max(0, playedEnd + stroke / 2)} height={height} /></clipPath></defs>
            {restStart < xEnd ? <line className="rest" x1={restStart} y1={mid} x2={xEnd} y2={mid} /> : null}
            {restStart < xEnd - stroke * 2 ? <circle className="stop" cx={xEnd} cy={mid} r={2} /> : null}
            {playedEnd > x0 ? <path ref={path} className="played" d={d} clipPath={`url(#${clipId})`} /> : null}
            <rect className="thumb" x={x - 2.5} y={mid - 14} width={5} height={28} rx={2.5} />
          </svg>
        ) : null}
        {hover !== undefined && dur && !dragging ? <div className="hover-time" style={{ left: `${hover * 100}%` }}>{fmtTime(hover * dur)}</div> : null}
        {bubble && dur ? <div className="value-bubble" style={{ left: x }}>{fmtTime(shown)}</div> : null}
      </div>
      <div className="np-seek-times" aria-hidden="true">
        <span className="time" data-testid="fs-seek-position">{fmtTime(shown)}</span>
        <span className="time">-{fmtTime(Math.max(0, dur - shown))}</span>
      </div>
    </div>
  );
}
