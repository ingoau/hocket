// Gesture playground (developer tool): try trackpad swipes on mock surfaces and tune how they
// feel, live. Song rows (Android's swipe actions), a queue whose rows swipe away, a cover to swipe
// between tracks, and two-finger back/forward on a page. Each surface has its own tuning; a live
// trace shows the travel against the threshold with AppKit's phases and the haptics marked.
// Opened from Developer diagnostics. Tunings are kept per device (localStorage); "Copy" puts them
// all on the clipboard as JSON.
import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import type { HapticPattern, ScrollPhase } from "@shared/bridge-types";
import { HAPTIC_PATTERNS } from "@shared/bridge-types";
import { bridge } from "../core/bridge";
import { Icon } from "../components/Icon";
import { Select, Slider } from "../components/controls";
import { loadLocal, saveLocal } from "../lib/local-settings";
import { spring } from "../lib/spring";
import { DEFAULT_TUNING, type Side, type SwipeEffect, type SwipeTuning } from "../lib/swipe";
import { playHaptic, useScrollPhases, useSwipe, type SwipeTrace } from "../lib/swipe-hook";
import { showToast } from "../lib/ui-toast";
import "../styles/gestures.css";

type SurfaceId = "rows" | "queue" | "cover" | "page";
const SURFACES: { id: SurfaceId; label: string }[] = [
  { id: "rows", label: "Song rows" },
  { id: "queue", label: "Queue" },
  { id: "cover", label: "Cover" },
  { id: "page", label: "Page" },
];

const SURFACE_DEFAULTS: Record<SurfaceId, SwipeTuning> = {
  rows: DEFAULT_TUNING,
  queue: DEFAULT_TUNING,
  cover: { ...DEFAULT_TUNING, threshold: 0.4, maxTravel: 0.35, resistance: 0.25, flingVelocity: 1.2, hapticCommit: "none" },
  page: { ...DEFAULT_TUNING, threshold: 0.16, maxTravel: 0.3, resistance: 0.6, flingVelocity: 1, hapticDisarm: "none", hapticCommit: "none" },
};

const PRESETS: { label: string; desc: string; patch: Partial<SwipeTuning> }[] = [
  { label: "Android-like", desc: "Run on release past 30%", patch: { threshold: 0.3, commit: "release", gain: 1, resistance: 0.45, flingVelocity: 0 } },
  { label: "Instant", desc: "Run the moment it crosses, so the commit haptic is felt", patch: { commit: "threshold", threshold: 0.25, hapticArm: "none", hapticCommit: "levelChange" } },
  { label: "Loose", desc: "Light, short, flickable", patch: { gain: 1.4, threshold: 0.2, resistance: 0.3, flingVelocity: 0.9, commit: "release" } },
  { label: "Heavy", desc: "Deliberate, lots of resistance", patch: { gain: 0.75, threshold: 0.35, resistance: 0.75, flingVelocity: 0, commit: "release" } },
];

type Tunings = Record<SurfaceId, SwipeTuning>;
const STORE_KEY = "gestures.playground.v8";

function loadTunings(): Tunings {
  const stored = loadLocal<Partial<Record<SurfaceId, Partial<SwipeTuning>>>>(STORE_KEY, {});
  const out = {} as Tunings;
  for (const s of SURFACES) out[s.id] = { ...SURFACE_DEFAULTS[s.id], ...(stored[s.id] ?? {}) };
  return out;
}

function haptic(pattern: HapticPattern): void {
  try {
    bridge().gestures.haptic(pattern);
  } catch {
    /* no bridge */
  }
}

/** The settle transition for a tuning, as CSS. */
function useSettle(t: SwipeTuning): string {
  return useMemo(() => {
    const s = spring(t.springStiffness, t.springDamping);
    return `transform ${s.duration}ms ${s.easing}`;
  }, [t.springStiffness, t.springDamping]);
}

// ---- trace & log ------------------------------------------------------------------------------

interface Sample { t: number; v: number; armed: boolean; thr: number }
interface Marker { t: number; kind: "phase" | "haptic" | "commit" | "release"; label: string }
interface LogLine { id: number; t: number; surface: SurfaceId | "trackpad"; text: string; kind: Marker["kind"] | "wheel" | "decided" }

const TRACE_MS = 4000;

function effectText(e: SwipeEffect): string {
  switch (e.kind) {
    case "haptic":
      return `haptic ${e.pattern} (${e.reason})`;
    case "commit":
      return `commit ${e.side > 0 ? "→" : "←"} (${e.how})`;
    case "release":
      return `release: ${e.reason}${e.committed ? "" : ", springs back"}`;
    case "decided":
      return e.axis === "x" ? "swipe (sideways)" : "scroll (vertical), ignored";
  }
}

// ---- the page --------------------------------------------------------------------------------

export function GesturePlayground() {
  const [tunings, setTunings] = useState<Tunings>(loadTunings);
  const [editing, setEditing] = useState<SurfaceId>("rows");
  const [log, setLog] = useState<LogLine[]>([]);
  const [logWheel, setLogWheel] = useState(false);
  const [hapticsOk, setHapticsOk] = useState<boolean | undefined>(undefined);
  const [actuatorOk, setActuatorOk] = useState<boolean | undefined>(undefined);
  const [phases, setPhases] = useState(0);
  const [natural, setNatural] = useState<boolean | undefined>(undefined);
  const samples = useRef<Sample[]>([]);
  const markers = useRef<Marker[]>([]);
  const nextId = useRef(0);
  const logWheelRef = useRef(logWheel);
  logWheelRef.current = logWheel;

  useEffect(() => saveLocal(STORE_KEY, tunings), [tunings]);
  useEffect(() => {
    void bridge().gestures.hapticsAvailable().then(setHapticsOk, () => setHapticsOk(false));
    void bridge().gestures.actuatorAvailable().then(setActuatorOk, () => setActuatorOk(false));
  }, []);

  const push = useCallback((surface: LogLine["surface"], kind: LogLine["kind"], text: string, t: number) => {
    const line = { id: nextId.current++, t, surface, kind, text };
    setLog((l) => [line, ...l].slice(0, 80));
  }, []);

  useScrollPhases(
    useCallback(
      (p: ScrollPhase) => {
        setPhases((n) => n + 1);
        setNatural(p.inverted);
        const t = performance.now();
        const label = p.phase !== "none" && p.phase !== "changed" ? `fingers ${p.phase}` : `momentum ${p.momentum}`;
        markers.current.push({ t, kind: "phase", label });
        push("trackpad", "phase", `${label}${p.precise ? "" : " (mouse wheel)"}`, t);
      },
      [push],
    ),
  );

  const traceFor = useCallback(
    (surface: SurfaceId): SwipeTrace => ({
      wheel: (dx, dy, consumed, t) => {
        if (logWheelRef.current) push(surface, "wheel", `wheel dx ${dx.toFixed(1)} dy ${dy.toFixed(1)}${consumed ? " ✓" : ""}`, t);
      },
      effect: (e, t) => {
        if (e.kind === "haptic" || e.kind === "commit" || e.kind === "release") markers.current.push({ t, kind: e.kind, label: effectText(e) });
        // Detent ticks are on the trace; in the log they would drown everything else.
        if (e.kind === "haptic" && e.reason === "detent") return;
        push(surface, e.kind, effectText(e), t);
      },
      frame: (offset, width, armed, t) => {
        samples.current.push({ t, v: offset / width, armed, thr: tunings[surface].threshold });
      },
    }),
    [push, tunings],
  );

  const tuning = tunings[editing];
  const setTuning = (patch: Partial<SwipeTuning>) => setTunings((all) => ({ ...all, [editing]: { ...all[editing], ...patch } }));

  const copy = () => {
    const json = JSON.stringify(tunings, null, 2);
    try {
      bridge().clipboard.writeText(json);
      showToast("Tunings copied as JSON");
    } catch {
      void navigator.clipboard.writeText(json);
    }
  };

  return (
    <div className="view page-gestures" data-testid="view-gestures">
      <div className="view-header">
        <h1 className="brand-type">Gesture playground</h1>
        <div className="actions">
          <button type="button" className="btn" onClick={copy}><Icon name="copy" size={16} /> Copy tunings</button>
          <button type="button" className="btn" onClick={() => setTunings({ ...SURFACE_DEFAULTS })}><Icon name="restore" size={16} /> Reset all</button>
        </div>
      </div>
      <div className="view-body">
        <div className="gp-layout">
          <div className="gp-surfaces">
            <p className="muted">
              Swipe sideways with two fingers on anything below. Each surface has its own tuning: pick one on the right to change it. A notched mouse wheel never swipes.
            </p>
            <div className="gp-status">
              <Chip ok={hapticsOk}>{hapticsOk === undefined ? "Haptics…" : hapticsOk ? "Haptics on" : "No haptics (macOS + native addon only)"}</Chip>
              <Chip ok={phases > 0}>{phases > 0 ? `AppKit phases: ${phases}` : "No AppKit phases yet (\"native\" release falls back to idle)"}</Chip>
              <Chip ok={actuatorOk}>{actuatorOk === undefined ? "Actuator…" : actuatorOk ? "Private actuator on" : "No private actuator (falls back to generic)"}</Chip>
              {natural !== undefined ? <Chip ok>{natural ? "Natural scrolling" : "Natural scrolling off"}</Chip> : null}
            </div>
            <HapticLab tuning={tunings[editing]} />

            <Section title="Song rows" hint="Right: play next. Left: play later. The rows never leave." active={editing === "rows"} onPick={() => setEditing("rows")}>
              <SongRows tuning={tunings.rows} trace={traceFor("rows")} />
            </Section>
            <Section title="Queue" hint="Either way removes. The row slides out." active={editing === "queue"} onPick={() => setEditing("queue")}>
              <QueueRows tuning={tunings.queue} trace={traceFor("queue")} />
            </Section>
            <Section title="Cover" hint="Swipe the artwork to skip. Right: previous. Left: next." active={editing === "cover"} onPick={() => setEditing("cover")}>
              <Cover tuning={tunings.cover} trace={traceFor("cover")} />
            </Section>
            <Section title="Page back and forward" hint="Right: back. Left: forward. Anywhere on the page." active={editing === "page"} onPick={() => setEditing("page")}>
              <PageNav tuning={tunings.page} trace={traceFor("page")} />
            </Section>
          </div>

          <aside className="gp-panel">
            <section className="card gp-card">
              <div className="gp-tabs" role="tablist" aria-label="Surface to tune">
                {SURFACES.map((s) => (
                  <button key={s.id} type="button" role="tab" aria-selected={editing === s.id} className={`gp-tab ${editing === s.id ? "on" : ""}`} onClick={() => setEditing(s.id)}>{s.label}</button>
                ))}
              </div>
              <div className="gp-presets">
                {PRESETS.map((p) => (
                  <button key={p.label} type="button" className="btn sm" title={p.desc} onClick={() => setTuning(p.patch)}>{p.label}</button>
                ))}
                <button type="button" className="btn sm ghost" onClick={() => setTuning(SURFACE_DEFAULTS[editing])}>Default</button>
              </div>
              <TuningForm tuning={tuning} set={setTuning} />
            </section>
            <section className="card gp-card">
              <h2 className="card-title">Trace</h2>
              <TraceCanvas samples={samples} markers={markers} />
              <p className="muted small">Travel (of the width) over the last 4 s. Dashed: the threshold. Ticks: AppKit phases (blue), haptics (accent), commits (green).</p>
            </section>
            <section className="card gp-card">
              <div className="gp-log-head">
                <h2 className="card-title">Events</h2>
                <label className="gp-check"><input type="checkbox" checked={logWheel} onChange={(e) => setLogWheel(e.target.checked)} /> wheel events</label>
                <button type="button" className="btn sm ghost" onClick={() => setLog([])}>Clear</button>
              </div>
              <ol className="gp-log">
                {log.map((l) => (
                  <li key={l.id} className={`gp-log-${l.kind}`}>
                    <span className="gp-log-t">{(l.t / 1000).toFixed(2)}</span>
                    <span className="gp-log-s">{l.surface}</span>
                    {l.text}
                  </li>
                ))}
              </ol>
            </section>
          </aside>
        </div>
      </div>
    </div>
  );
}

/** Every waveform the actuator accepts (probed: 0–64 refuses all but these). */
const ACTUATOR_IDS = [1, 2, 3, 4, 5, 6, 15, 16];

/**
 * Hover to feel: each tile fires as the pointer glides onto it with a finger resting on the pad.
 * Clicking would fire the trackpad's own click haptic over the one being tried.
 */
function HapticLab({ tuning }: { tuning: SwipeTuning }) {
  const [arg1, setArg1] = useState(0);
  const [arg2, setArg2] = useState(0);
  const [last, setLast] = useState("");
  const fire = (label: string, play: () => void) => {
    play();
    setLast(label);
  };
  const actuate = (id: number) => {
    try {
      bridge().gestures.actuate(id, arg1, arg2);
    } catch {
      /* no bridge */
    }
  };
  const tile = (key: string, label: string, sub: string, play: () => void) => (
    <div key={key} className={`gp-lab-tile ${last === key ? "on" : ""}`} onPointerEnter={() => fire(key, play)}>
      <span className="gp-lab-label">{label}</span>
      <span className="muted small">{sub}</span>
    </div>
  );
  return (
    <section className="card gp-section gp-lab">
      <div className="gp-section-head">
        <h2 className="card-title">Haptic lab</h2>
        <span className="muted small">Rest a finger on the trackpad and glide the pointer across the tiles. Don’t click: the click’s own haptic drowns them out.</span>
      </div>
      <div className="gp-lab-grid">
        {ACTUATOR_IDS.map((id) => tile(`mt${id}`, `#${id}`, id === 3 ? "weak" : id === 4 ? "medium" : id === 6 ? "strong" : "actuator", () => actuate(id)))}
        {HAPTIC_PATTERNS.map((p) => tile(p, p, "AppKit", () => haptic(p)))}
        {tile("first", "first tick", `#${tuning.detentIdFrom}`, () => playHaptic("actuator", tuning, 0))}
        {tile("lastTick", "last tick", `#${tuning.detentIdTo}`, () => playHaptic("actuator", tuning, 1))}
        {tile("kick", "kick", `#${tuning.kickId}`, () => playHaptic("actuator", tuning))}
      </div>
      <div className="gp-lab-args">
        <div className="gp-num">
          <div className="gp-num-head"><span>Lab arg 1</span><span className="gp-num-v">{arg1.toFixed(2)}</span></div>
          <Slider size="sm" min={0} max={3} step={0.05} value={arg1} onChange={setArg1} aria-label="Lab arg 1" />
        </div>
        <div className="gp-num">
          <div className="gp-num-head"><span>Lab arg 2</span><span className="gp-num-v">{arg2.toFixed(2)}</span></div>
          <Slider size="sm" min={0} max={3} step={0.05} value={arg2} onChange={setArg2} aria-label="Lab arg 2" />
        </div>
      </div>
      <p className="muted small">The # tiles use the lab args; first tick, last tick and kick use the tuning of the surface selected on the right.</p>
    </section>
  );
}

function Chip({ ok, children }: { ok: boolean | undefined; children: ReactNode }) {
  return <span className={`gp-chip ${ok ? "ok" : ok === false ? "no" : ""}`}>{children}</span>;
}

function Section({ title, hint, active, onPick, children }: { title: string; hint: string; active: boolean; onPick: () => void; children: ReactNode }) {
  return (
    <section className={`card gp-section ${active ? "active" : ""}`} onPointerDown={onPick}>
      <div className="gp-section-head">
        <h2 className="card-title">{title}</h2>
        <span className="muted small">{hint}</span>
      </div>
      {children}
    </section>
  );
}

// ---- tuning form -----------------------------------------------------------------------------

type NumKey = { [K in keyof SwipeTuning]: SwipeTuning[K] extends number ? K : never }[keyof SwipeTuning];
const NUMBERS: { key: NumKey; label: string; min: number; max: number; step: number; fmt: (v: number) => string; hint?: string }[] = [
  { key: "threshold", label: "Threshold", min: 0.05, max: 0.8, step: 0.01, fmt: pct, hint: "of the width" },
  { key: "gain", label: "Gain", min: 0.3, max: 2.5, step: 0.05, fmt: (v) => `${v.toFixed(2)}×` },
  { key: "resistance", label: "Resistance past threshold", min: 0, max: 0.95, step: 0.05, fmt: pct },
  { key: "maxTravel", label: "Max travel", min: 0.1, max: 1.2, step: 0.01, fmt: pct },
  { key: "hysteresis", label: "Disarm hysteresis", min: 0, max: 0.15, step: 0.005, fmt: pct },
  { key: "detentPx", label: "Detent spacing", min: 0, max: 80, step: 2, fmt: (v) => (v === 0 ? "off" : `${v} px`), hint: "ticks before the threshold" },
  { key: "detentRamp", label: "Detent ramp", min: 1, max: 4, step: 0.1, fmt: (v) => (v <= 1 ? "even" : `${v.toFixed(1)}×`), hint: "crowd towards the threshold" },
  { key: "detentIdFrom", label: "First tick ID", min: 1, max: 16, step: 1, fmt: (v) => `#${v}`, hint: "3 weak · 4 medium · 6 strong" },
  { key: "detentIdTo", label: "Last tick ID", min: 1, max: 16, step: 1, fmt: (v) => `#${v}` },
  { key: "detentStrengthCurve", label: "Strength curve", min: 0.5, max: 5, step: 0.1, fmt: (v) => `${v.toFixed(1)}`, hint: "higher stays light longer" },
  { key: "actuatorArg1", label: "Tick arg 1", min: 0, max: 3, step: 0.05, fmt: (v) => v.toFixed(2) },
  { key: "actuatorArg2", label: "Tick arg 2", min: 0, max: 3, step: 0.05, fmt: (v) => v.toFixed(2) },
  { key: "kickId", label: "Kick ID", min: 1, max: 16, step: 1, fmt: (v) => `#${v}`, hint: "the “actuator” arm and commit" },
  { key: "kickArg1", label: "Kick arg 1", min: 0, max: 3, step: 0.05, fmt: (v) => v.toFixed(2) },
  { key: "kickArg2", label: "Kick arg 2", min: 0, max: 3, step: 0.05, fmt: (v) => v.toFixed(2) },
  { key: "flingVelocity", label: "Fling velocity", min: 0, max: 4, step: 0.1, fmt: (v) => (v === 0 ? "off" : `${v.toFixed(1)} px/ms`) },
  { key: "axisLockPx", label: "Axis lock distance", min: 1, max: 30, step: 1, fmt: (v) => `${v} px` },
  { key: "axisRatio", label: "Sideways ratio", min: 0.5, max: 4, step: 0.1, fmt: (v) => `${v.toFixed(1)}×` },
  { key: "idleMs", label: "Idle gap", min: 30, max: 300, step: 5, fmt: (v) => `${v} ms` },
  { key: "decayEvents", label: "Decaying events", min: 2, max: 10, step: 1, fmt: (v) => String(v), hint: "momentum release" },
  { key: "springStiffness", label: "Spring stiffness", min: 100, max: 1500, step: 10, fmt: (v) => String(v) },
  { key: "springDamping", label: "Spring damping", min: 0.3, max: 1.2, step: 0.01, fmt: (v) => v.toFixed(2) },
];

function pct(v: number): string {
  return `${Math.round(v * 100)}%`;
}

const HAPTIC_CHOICES = ["none", ...HAPTIC_PATTERNS, "actuator"] as const;

function TuningForm({ tuning, set }: { tuning: SwipeTuning; set: (p: Partial<SwipeTuning>) => void }) {
  return (
    <div className="gp-form">
      <Choice label="Release" value={tuning.release} options={[["native", "AppKit phases"], ["idle", "Idle gap"], ["momentum", "Decaying deltas"]]} onChange={(v) => set({ release: v as SwipeTuning["release"] })} />
      <Choice label="Run action" value={tuning.commit} options={[["release", "On release"], ["threshold", "At threshold"]]} onChange={(v) => set({ commit: v as SwipeTuning["commit"] })} />
      <Choice label="Direction" value={tuning.direction} options={[["auto", "Follow system"], ["natural", "Natural"], ["reversed", "Reversed"]]} onChange={(v) => set({ direction: v as SwipeTuning["direction"] })} />
      {NUMBERS.map((n) => (
        <div key={n.key} className="gp-num">
          <div className="gp-num-head">
            <span>{n.label}{n.hint ? <span className="muted"> · {n.hint}</span> : null}</span>
            <span className="gp-num-v">{n.fmt(tuning[n.key])}</span>
          </div>
          <Slider size="sm" min={n.min} max={n.max} step={n.step} value={tuning[n.key]} onChange={(v) => set({ [n.key]: v })} aria-label={n.label} />
        </div>
      ))}
      <div className="gp-haptics">
        {(["hapticStart", "hapticDetent", "hapticArm", "hapticDisarm", "hapticCommit"] as const).map((k) => (
          <Choice key={k} label={`Haptic: ${k.slice(6).toLowerCase()}`} value={tuning[k]} options={HAPTIC_CHOICES.map((c) => [c, c])} onChange={(v) => set({ [k]: v as SwipeTuning[typeof k] })} />
        ))}
      </div>
      <p className="muted small">The trackpad only plays a haptic while a finger is on it, so a commit haptic on release is never felt. Use “At threshold” for that.</p>
    </div>
  );
}

function Choice({ label, value, options, onChange }: { label: string; value: string; options: readonly (readonly [string, string])[]; onChange: (v: string) => void }) {
  return (
    <label className="gp-choice">
      <span>{label}</span>
      <Select value={value} onChange={(e) => onChange(e.target.value)} aria-label={label}>
        {options.map(([v, l]) => (
          <option key={v} value={v}>{l}</option>
        ))}
      </Select>
    </label>
  );
}

// ---- trace canvas ----------------------------------------------------------------------------

function TraceCanvas({ samples, markers }: { samples: React.RefObject<Sample[]>; markers: React.RefObject<Marker[]> }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    let raf = 0;
    const draw = () => {
      raf = requestAnimationFrame(draw);
      const c = ref.current;
      if (!c) return;
      const dpr = window.devicePixelRatio || 1;
      const w = c.clientWidth;
      const h = c.clientHeight;
      if (c.width !== w * dpr || c.height !== h * dpr) {
        c.width = w * dpr;
        c.height = h * dpr;
      }
      const g = c.getContext("2d");
      if (!g) return;
      const css = getComputedStyle(c);
      const fg = css.getPropertyValue("--fg").trim() || "#888";
      const accent = css.getPropertyValue("--accent").trim() || "#8f80f5";
      const now = performance.now();
      const from = now - TRACE_MS;
      const s = samples.current.filter((p) => p.t >= from);
      samples.current = s;
      const m = markers.current.filter((p) => p.t >= from);
      markers.current = m;

      g.setTransform(dpr, 0, 0, dpr, 0, 0);
      g.clearRect(0, 0, w, h);
      const x = (t: number) => ((t - from) / TRACE_MS) * w;
      const y = (v: number) => h / 2 - v * (h / 2) * 1.6;
      g.globalAlpha = 0.25;
      g.strokeStyle = fg;
      g.lineWidth = 1;
      g.beginPath();
      g.moveTo(0, h / 2);
      g.lineTo(w, h / 2);
      g.stroke();
      const thr = s[s.length - 1]?.thr ?? 0.3;
      g.setLineDash([4, 4]);
      for (const v of [thr, -thr]) {
        g.beginPath();
        g.moveTo(0, y(v));
        g.lineTo(w, y(v));
        g.stroke();
      }
      g.setLineDash([]);
      g.globalAlpha = 1;
      for (const mk of m) {
        g.strokeStyle = mk.kind === "phase" ? "#4a9eff" : mk.kind === "commit" ? "#2fbf71" : mk.kind === "haptic" ? accent : fg;
        g.globalAlpha = mk.kind === "release" ? 0.4 : 0.9;
        g.lineWidth = mk.kind === "haptic" ? 3 : 1.5;
        const mx = x(mk.t);
        g.beginPath();
        g.moveTo(mx, mk.kind === "phase" ? 0 : h - 14);
        g.lineTo(mx, mk.kind === "phase" ? 10 : h);
        g.stroke();
      }
      g.globalAlpha = 1;
      g.lineWidth = 2;
      for (let i = 1; i < s.length; i++) {
        const a = s[i - 1];
        const b = s[i];
        if (!a || !b) continue;
        // A gap of more than 150 ms is a new swipe, not a line between two.
        if (b.t - a.t > 150) continue;
        g.strokeStyle = b.armed ? accent : fg;
        g.beginPath();
        g.moveTo(x(a.t), y(a.v));
        g.lineTo(x(b.t), y(b.v));
        g.stroke();
      }
    };
    draw();
    return () => cancelAnimationFrame(raf);
  }, [samples, markers]);
  return <canvas ref={ref} className="gp-trace" aria-label="Swipe trace" />;
}

// ---- surfaces --------------------------------------------------------------------------------

interface MockSong { id: number; title: string; artist: string; hue: number }
const SONGS: MockSong[] = [
  { id: 1, title: "Glass Harbour", artist: "Lumen Tide", hue: 12 },
  { id: 2, title: "Northbound Static", artist: "The Quiet Engines", hue: 48 },
  { id: 3, title: "Paper Satellites", artist: "Mira Okafor", hue: 96 },
  { id: 4, title: "Low Orbit Lullaby", artist: "Fen & Folly", hue: 150 },
  { id: 5, title: "Cinder Bloom", artist: "Halcyon Drift", hue: 200 },
  { id: 6, title: "Weatherglass", artist: "Ada Lind", hue: 240 },
  { id: 7, title: "Tin Roof Arcade", artist: "Petrichor Club", hue: 280 },
  { id: 8, title: "Saltwater Hymnal", artist: "Odd Lanterns", hue: 320 },
];

function Thumb({ song, size = 36 }: { song: MockSong; size?: number }) {
  return <span className="gp-thumb" style={{ width: size, height: size, background: `linear-gradient(135deg, hsl(${song.hue} 70% 62%), hsl(${song.hue + 40} 60% 38%))` }} />;
}

interface RowAction { label: string; icon: string; tone: "primary" | "secondary" | "danger"; dismisses: boolean }

function SwipeRow({ song, tuning, trace, start, end, onAction }: { song: MockSong; tuning: SwipeTuning; trace: SwipeTrace; start: RowAction | null; end: RowAction | null; onAction: (side: Side, a: RowAction) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [leaving, setLeaving] = useState<Side | 0>(0);
  const [flash, setFlash] = useState(false);
  const settle = useSettle(tuning);
  const view = useSwipe(ref, {
    tuning,
    trace,
    sides: { start: !!start, end: !!end },
    disabled: leaving !== 0,
    onCommit: (side) => {
      const a = side > 0 ? start : end;
      if (!a) return;
      if (a.dismisses) setLeaving(side);
      else {
        setFlash(true);
        setTimeout(() => setFlash(false), 450);
      }
      // Let the row slide out before the list drops it.
      setTimeout(() => onAction(side, a), a.dismisses ? 200 : 0);
    },
  });
  const side: Side | 0 = view.offset > 0 ? 1 : view.offset < 0 ? -1 : 0;
  const action = side > 0 ? start : side < 0 ? end : null;
  const tracking = view.stage === "tracking";
  const x = leaving ? `${leaving * 105}%` : `${view.offset}px`;
  const style: CSSProperties = { transform: `translateX(${x})`, transition: tracking ? "none" : leaving ? "transform 180ms cubic-bezier(.3,0,.8,.15)" : settle };
  return (
    <div ref={ref} className={`gp-row ${leaving ? "leaving" : ""}`} data-testid="gp-row" data-armed={view.armed !== 0}>
      {action ? (
        <div className={`gp-row-bg ${action.tone} ${side > 0 ? "from-start" : "from-end"} ${view.armed ? "armed" : ""}`} aria-hidden="true">
          <Icon name={action.icon} size={20} />
          <span>{action.label}</span>
        </div>
      ) : null}
      <div className={`gp-row-fg ${flash ? "flash" : ""}`} style={style}>
        <Thumb song={song} />
        <span className="gp-row-text">
          <span className="gp-row-title">{song.title}</span>
          <span className="gp-row-artist muted">{song.artist}</span>
        </span>
        <span className="muted small">3:{String((song.id * 7) % 60).padStart(2, "0")}</span>
      </div>
    </div>
  );
}

function SongRows({ tuning, trace }: { tuning: SwipeTuning; trace: SwipeTrace }) {
  const [last, setLast] = useState<string>("");
  const start: RowAction = { label: "Play next", icon: "playNext", tone: "primary", dismisses: false };
  const end: RowAction = { label: "Play later", icon: "playLater", tone: "secondary", dismisses: false };
  return (
    <>
      <div className="gp-list" data-testid="gp-rows">
        {SONGS.slice(0, 5).map((s) => (
          <SwipeRow key={s.id} song={s} tuning={tuning} trace={trace} start={start} end={end} onAction={(_, a) => setLast(`${a.label}: ${s.title}`)} />
        ))}
      </div>
      <p className="muted small gp-last">{last || " "}</p>
    </>
  );
}

function QueueRows({ tuning, trace }: { tuning: SwipeTuning; trace: SwipeTrace }) {
  const [songs, setSongs] = useState(SONGS);
  const remove: RowAction = { label: "Remove", icon: "trash", tone: "danger", dismisses: true };
  return (
    <>
      <div className="gp-list gp-queue" data-testid="gp-queue">
        {songs.map((s) => (
          <SwipeRow key={s.id} song={s} tuning={tuning} trace={trace} start={remove} end={remove} onAction={() => setSongs((l) => l.filter((x) => x.id !== s.id))} />
        ))}
        {songs.length === 0 ? <p className="muted">Queue empty.</p> : null}
      </div>
      <button type="button" className="btn sm" onClick={() => setSongs(SONGS)} disabled={songs.length === SONGS.length}><Icon name="undo" size={14} /> Refill queue</button>
    </>
  );
}

function Cover({ tuning, trace }: { tuning: SwipeTuning; trace: SwipeTrace }) {
  const ref = useRef<HTMLDivElement>(null);
  const [index, setIndex] = useState(0);
  /** A skip in flight: the strip slides a whole cover, then the index moves and it snaps back. */
  const [skipping, setSkipping] = useState<Side | 0>(0);
  const [snap, setSnap] = useState(false);
  const settle = useSettle(tuning);
  const view = useSwipe(ref, {
    tuning,
    trace,
    disabled: skipping !== 0,
    onCommit: (side) => {
      setSkipping(side);
      setTimeout(() => {
        setSnap(true);
        setIndex((i) => (i - side + SONGS.length) % SONGS.length);
        setSkipping(0);
        requestAnimationFrame(() => requestAnimationFrame(() => setSnap(false)));
      }, 260);
    },
  });
  const at = (k: number): MockSong => SONGS[(index + k + SONGS.length) % SONGS.length] ?? SONGS[0]!;
  const tracking = view.stage === "tracking";
  const shift = skipping ? `calc(${skipping} * (100% + 16px))` : `${view.offset}px`;
  const style: CSSProperties = { transform: `translateX(${shift})`, transition: tracking || snap ? "none" : skipping ? "transform 260ms cubic-bezier(.2,.8,.2,1)" : settle };
  const song = at(0);
  return (
    <div className="gp-cover-wrap">
      <div ref={ref} className={`gp-cover ${view.armed ? "armed" : ""}`}>
        <div className="gp-cover-strip" style={style}>
          {[-1, 0, 1].map((k) => (
            <div key={k} className="gp-cover-slot" style={{ left: `calc(${k} * (100% + 16px))` }}>
              <Thumb song={at(k)} size={220} />
            </div>
          ))}
        </div>
      </div>
      <div className="gp-cover-meta">
        <div className="gp-row-title">{song.title}</div>
        <div className="muted">{song.artist}</div>
        <div className="gp-cover-controls">
          <button type="button" className="btn icon" aria-label="Previous" onClick={() => setIndex((i) => (i - 1 + SONGS.length) % SONGS.length)}><Icon name="previous" size={18} /></button>
          <span className="muted small">{index + 1} / {SONGS.length}</span>
          <button type="button" className="btn icon" aria-label="Next" onClick={() => setIndex((i) => (i + 1) % SONGS.length)}><Icon name="next" size={18} /></button>
        </div>
      </div>
    </div>
  );
}

const PAGES = [
  { title: "Home", items: ["Recently played", "Spotlight", "Your mixes", "New albums", "Loved"] },
  { title: "Albums", items: SONGS.map((s) => s.artist) },
  { title: "Paper Satellites", items: SONGS.slice(0, 6).map((s) => s.title) },
];

function PageNav({ tuning, trace }: { tuning: SwipeTuning; trace: SwipeTrace }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState(1);
  const [dir, setDir] = useState<Side>(1);
  const settle = useSettle(tuning);
  const view = useSwipe(ref, {
    tuning,
    trace,
    sides: { start: pos > 0, end: pos < PAGES.length - 1 },
    onCommit: (side) => {
      setDir(side);
      setPos((p) => Math.max(0, Math.min(PAGES.length - 1, p - side)));
    },
  });
  const page = PAGES[pos] ?? PAGES[0]!;
  const thr = tuning.threshold;
  const width = ref.current?.clientWidth ?? 1;
  const progress = Math.min(1, Math.abs(view.offset) / (thr * width));
  const tracking = view.stage === "tracking";
  return (
    <div ref={ref} className="gp-page">
      <div className={`gp-page-arrow back ${view.armed > 0 ? "armed" : ""}`} style={{ opacity: view.offset > 0 ? progress : 0, transform: `translate(${view.offset > 0 ? progress * 56 - 48 : -48}px, -50%) scale(${0.6 + 0.4 * (view.offset > 0 ? progress : 0)})`, transition: tracking ? "none" : "all 200ms ease" }}>
        <Icon name="chevronLeft" size={22} />
      </div>
      <div className={`gp-page-arrow fwd ${view.armed < 0 ? "armed" : ""}`} style={{ opacity: view.offset < 0 ? progress : 0, transform: `translate(${view.offset < 0 ? 48 - progress * 56 : 48}px, -50%) scale(${0.6 + 0.4 * (view.offset < 0 ? progress : 0)})`, transition: tracking ? "none" : "all 200ms ease" }}>
        <Icon name="chevronRight" size={22} />
      </div>
      <div className="gp-page-body" style={{ transform: `translateX(${view.offset * 0.25}px)`, transition: tracking ? "none" : settle }}>
        <div key={pos} className={`gp-page-content enter-${dir > 0 ? "back" : "fwd"}`}>
          <div className="gp-crumbs muted small">
            {PAGES.map((p, i) => (
              <span key={p.title} className={i === pos ? "on" : ""}>{p.title}</span>
            ))}
          </div>
          <h3>{page.title}</h3>
          <ul>
            {page.items.map((it) => (
              <li key={it}>{it}</li>
            ))}
          </ul>
        </div>
      </div>
    </div>
  );
}
