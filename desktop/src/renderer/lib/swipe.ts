// Trackpad swipes from `wheel` events: a horizontal two-finger swipe drags a surface sideways, and
// letting go past a threshold runs that side's action (Android's SwipeActionBox, on a trackpad).
//
// The web gives a swipe only as a stream of wheel deltas, with no "fingers down" or "fingers up",
// and after a flick macOS keeps sending momentum deltas with no finger on the pad. So when the
// swipe ends is the hard part. Three ways, chosen by `release`:
// - "native": AppKit's scroll phases (bridge().gestures.onScrollPhase), exact. macOS only; until a
//   phase has arrived it behaves as "idle".
// - "idle": a gap in the events. Momentum keeps the events coming, so a flick only ends once the
//   coasting stops.
// - "momentum": deltas shrinking for a few events in a row read as the coast after a lift. A
//   finger slowing down looks the same, so it can end a swipe early.
//
// Windows has no phases, but its precision touchpads coast through Chromium too, so it uses
// "momentum"; Linux uses "idle" ("native" falls back to it when no phase ever arrives).
//
// A mouse wheel is never a swipe: a notched wheel comes as lines, or (Windows, Linux) as coarse
// pixel deltas of 50 to 120 a notch, and Shift turns a wheel sideways. `coarseWheel` spots them and
// the tracker leaves a stream that starts like that alone.
//
// Pure: time comes in with each call and side effects go out as `SwipeEffect`s, so it is tested
// without a DOM. `useSwipe` (swipe-hook.ts) drives it from real events.
import type { HapticPattern, ScrollPhase } from "@shared/bridge-types";

/**
 * "actuator": the trackpad's actuator driven directly (`kickId`, `kickArg1`, `kickArg2`) through
 * MultitouchSupport's MTActuatorActuate, a private macOS API (crates/hocket-node/src/gestures.rs):
 * a firmer click than AppKit's patterns. Falls back to AppKit's "generic" where it is missing.
 */
export type HapticSource = HapticPattern | "actuator";
export type HapticChoice = HapticSource | "none";
export type ReleaseMode = "native" | "idle" | "momentum";
/** Run the action when the swipe ends past the threshold, or the moment it crosses it. */
export type CommitMode = "release" | "threshold";
/** Which way the deltas run against the fingers: from AppKit (natural scrolling), or forced. */
export type DirectionMode = "auto" | "natural" | "reversed";

export interface SwipeTuning {
  /** Pixels of travel per pixel of wheel delta. */
  gain: number;
  /** Of the surface's width: how far a swipe goes before letting go runs the action. */
  threshold: number;
  /** Of the width: once armed, coming back this much short of the threshold disarms. */
  hysteresis: number;
  /** Of the width: how far the surface follows the fingers (then it stretches a little). */
  maxTravel: number;
  /** 0..1: how much travel past the threshold is held back (0 follows the fingers exactly). */
  resistance: number;
  /** Pixels of movement before deciding between a swipe and a scroll. */
  axisLockPx: number;
  /** A swipe needs this many times more sideways movement than vertical. */
  axisRatio: number;
  release: ReleaseMode;
  /** "idle" (and the fallback of the others): a gap this long ends the swipe. */
  idleMs: number;
  /** "momentum": this many shrinking deltas in a row read as a lift. */
  decayEvents: number;
  commit: CommitMode;
  direction: DirectionMode;
  /** px/ms: ending a swipe faster than this towards a side runs it short of the threshold (0: off). */
  flingVelocity: number;
  /** Pixels of travel between detent ticks while short of the threshold (0: none), on average. */
  detentPx: number;
  /** 1: evenly spaced detents; above 1 they crowd towards the threshold (spacing ∝ distance^(ramp−1)). */
  detentRamp: number;
  /** An "actuator" haptic: MTActuatorActuate's actuation ID and its two float arguments. */
  kickId: number;
  kickArg1: number;
  kickArg2: number;
  hapticStart: HapticChoice;
  /** The tick every `detentPx` on the way to the threshold: a texture under the fingers. */
  hapticDetent: HapticChoice;
  hapticArm: HapticChoice;
  hapticDisarm: HapticChoice;
  hapticCommit: HapticChoice;
  /** The settle back (or out): spring stiffness and damping ratio. */
  springStiffness: number;
  springDamping: number;
}

/** Tuned on a MacBook's trackpad: song rows and the queue. */
export const DEFAULT_TUNING: SwipeTuning = {
  gain: 1,
  threshold: 0.2,
  hysteresis: 0.03,
  maxTravel: 0.7,
  resistance: 0.45,
  axisLockPx: 6,
  axisRatio: 1.2,
  release: "native",
  idleMs: 90,
  decayEvents: 4,
  commit: "release",
  direction: "auto",
  flingVelocity: 1,
  detentPx: 28,
  detentRamp: 3,
  kickId: 15,
  kickArg1: 3,
  kickArg2: 3,
  hapticStart: "none",
  hapticDetent: "alignment",
  hapticArm: "actuator",
  hapticDisarm: "alignment",
  hapticCommit: "levelChange",
  springStiffness: 500,
  springDamping: 0.82,
};

/** 1: towards the end of the line (fingers moving right in LTR); -1: towards the start. */
export type Side = 1 | -1;

export type HapticReason = "start" | "detent" | "arm" | "disarm" | "commit";

export type SwipeEffect =
  | { kind: "haptic"; pattern: HapticSource; reason: HapticReason }
  | { kind: "commit"; side: Side; how: "threshold" | "release" | "fling" }
  | { kind: "release"; committed: boolean; reason: string }
  | { kind: "decided"; axis: "x" | "y" };

/**
 * - idle: no swipe; the next event may start one.
 * - deciding: moving, not yet sure whether sideways.
 * - tracking: the surface follows the fingers.
 * - passthrough: a scroll (or a stream that started elsewhere), left alone until it ends.
 * - coasting: the swipe ended; its remaining events (momentum) are swallowed until they stop.
 */
export type SwipeStage = "idle" | "deciding" | "tracking" | "passthrough" | "coasting";

/**
 * What the trackpad is doing, shared by every surface in a window: a swipe may only start at the
 * start of a stream of events, never on a surface that momentum (or a scroll that moved the list)
 * brings under the pointer halfway through one.
 */
/** Momentum with no wheel event for this long is over, even without its "ended" (it went to another app). */
export const STALE_MOMENTUM_MS = 400;
/** Fingers resting this long without a wheel event no longer hold a swipe or a stream open. */
export const STALE_FINGERS_MS = 3000;

export class WheelSequence {
  /** Time of the last wheel event anywhere. */
  lastWheel = -Infinity;
  /** The one before it: nested surfaces (a row inside the page) see the same event in turn. */
  private prevWheel = -Infinity;
  /** Whether AppKit phases have arrived (so "native" can rely on them). */
  native = false;
  fingersDown = false;
  momentum = false;
  /** Natural scrolling, from AppKit; macOS's default until told otherwise. */
  inverted = true;
  /** A touch began and no wheel event has come since: the next one starts a stream. */
  private began = false;
  /** When that first event came (nested surfaces see it in turn). */
  private startT = NaN;

  phase(p: ScrollPhase): void {
    this.native = true;
    this.inverted = p.inverted;
    if (p.phase === "began" || p.phase === "mayBegin") {
      this.fingersDown = true;
      this.began = true;
    }
    if (p.phase === "ended" || p.phase === "cancelled") this.fingersDown = false;
    if (p.momentum === "began") this.momentum = true;
    if (p.momentum === "ended" || p.momentum === "cancelled" || p.phase === "began" || p.phase === "mayBegin") this.momentum = false;
  }

  /** Fingers on the pad, as of `t` (a lift that never reached us expires). */
  fingersAt(t: number): boolean {
    return this.native && this.fingersDown && t - this.lastWheel <= STALE_FINGERS_MS;
  }

  /** Momentum still coasting, as of `t`. */
  momentumAt(t: number): boolean {
    return this.native && this.momentum && t - this.lastWheel <= STALE_MOMENTUM_MS;
  }

  /**
   * Whether an event at `t` (before `note`) may start a new swipe: after a gap in the events, or
   * with AppKit's phases the first event of a touch. Never halfway through one, nor in momentum.
   */
  canStart(t: number, idleMs: number): boolean {
    const last = t === this.lastWheel ? this.prevWheel : this.lastWheel;
    const gap = t - last > idleMs;
    if (!this.native) return gap;
    if (this.momentum && t - last <= STALE_MOMENTUM_MS) return false;
    return gap || this.began || t === this.startT;
  }

  note(t: number): void {
    if (t === this.lastWheel) return;
    if (this.began) {
      this.startT = t;
      this.began = false;
    }
    this.prevWheel = this.lastWheel;
    this.lastWheel = t;
  }
}

/** The surface's displayed travel for raw finger travel `raw`, both in px, signed. */
export function shapeTravel(raw: number, width: number, t: SwipeTuning): number {
  const a = Math.abs(raw);
  const thr = t.threshold * width;
  const max = t.maxTravel * width;
  let d = a <= thr ? a : thr + (a - thr) * (1 - t.resistance);
  // Past the end it stretches towards 48 px more, never further.
  if (d > max) {
    const over = d - max;
    d = max + (48 * over) / (48 + over);
  }
  return Math.sign(raw) * d;
}

function detentCount(thr: number, t: SwipeTuning): number {
  return t.detentPx > 0 && thr > 0 ? Math.max(1, Math.round(thr / t.detentPx)) : 0;
}

/** Which detent travel `a` (0..thr px) is in: the count, spread by (a/thr)^ramp. */
export function detentIndex(a: number, thr: number, t: SwipeTuning): number {
  const n = detentCount(thr, t);
  if (n === 0) return 0;
  return Math.floor(n * Math.pow(Math.max(0, Math.min(1, a / thr)), Math.max(0.2, t.detentRamp)) + 1e-9);
}

export class SwipeTracker {
  stage: SwipeStage = "idle";
  /** Raw finger travel (px, signed by Side). */
  raw = 0;
  /** Displayed travel: what the surface is offset by. */
  offset = 0;
  armed: Side | 0 = 0;
  width = 1;
  /** Which sides have an action; a side without one does not move. */
  sides: { 1: boolean; [-1]: boolean } = { 1: true, [-1]: true };

  private accX = 0;
  private accY = 0;
  private lastT = -Infinity;
  private lastAbs: number[] = [];
  /** Which detent (travel / detentPx) the surface was last in. */
  private detent = 0;
  private samples: { t: number; raw: number }[] = [];

  constructor(
    public tuning: SwipeTuning,
    private seq: WheelSequence,
  ) {}

  /**
   * Whether a wheel event at `t` would start a new stream here (so the caller can say what it is
   * on first). Ask before `wheel`.
   */
  wouldStart(t: number): boolean {
    if (!this.seq.canStart(t, this.tuning.idleMs)) return false;
    if (this.stage === "idle") return true;
    return t - this.lastT > this.tuning.idleMs && (this.stage === "passthrough" || this.stage === "coasting");
  }

  /**
   * One wheel event: deltas as the event has them (px), at `t` ms. `consumed` says the caller
   * should preventDefault it (it belongs to a swipe). `coarse`: it looks like a mouse wheel
   * (`coarseWheel`); a stream that starts like that is left alone.
   */
  wheel(dx: number, dy: number, t: number, coarse = false): { consumed: boolean; effects: SwipeEffect[] } {
    const effects: SwipeEffect[] = [];
    const tun = this.tuning;
    const fresh = t - this.lastT > tun.idleMs;
    const startable = this.seq.canStart(t, tun.idleMs);
    this.seq.note(t);

    // A stream that ended without a phase (or without native phases at all) ends here.
    if ((this.stage === "passthrough" || this.stage === "coasting") && fresh && startable) this.stage = "idle";
    // A finger resting on the pad is not a lift when the phases say so.
    const held = this.tuning.release === "native" && this.seq.fingersAt(t);
    if (this.stage === "tracking" && fresh && !held) effects.push(...this.release(t, "idle gap"));
    if (this.stage === "coasting" || this.stage === "passthrough") {
      this.lastT = t;
      return { consumed: this.stage === "coasting", effects };
    }
    if (this.stage === "idle") {
      // A mouse wheel, momentum from elsewhere, or nothing to swipe to: not a swipe.
      if (!startable || coarse || (!this.sides[1] && !this.sides[-1])) {
        this.stage = "passthrough";
        this.lastT = t;
        return { consumed: false, effects };
      }
      this.stage = "deciding";
      this.accX = 0;
      this.accY = 0;
    }
    this.lastT = t;

    const fx = this.fingers(dx) * tun.gain;
    if (this.stage === "deciding") {
      this.accX += fx;
      this.accY += dy;
      if (Math.hypot(this.accX, this.accY) < tun.axisLockPx) {
        // Hold on to it while it looks sideways: Chromium only lets the rest of a stream be
        // cancelled if its first event was.
        return { consumed: Math.abs(this.accX) >= Math.abs(this.accY), effects };
      }
      if (Math.abs(this.accX) > tun.axisRatio * Math.abs(this.accY)) {
        this.stage = "tracking";
        this.raw = 0;
        this.samples = [];
        this.lastAbs = [];
        this.detent = 0;
        effects.push({ kind: "decided", axis: "x" });
        effects.push(...this.haptic(tun.hapticStart, "start"));
        effects.push(...this.move(this.accX, t));
        return { consumed: true, effects };
      }
      this.stage = "passthrough";
      effects.push({ kind: "decided", axis: "y" });
      return { consumed: false, effects };
    }

    // tracking
    effects.push(...this.move(fx, t));
    if (this.stage === "tracking" && tun.release === "momentum" && this.decaying(Math.abs(dx))) {
      effects.push(...this.release(t, "deltas decaying"));
    }
    return { consumed: true, effects };
  }

  /** An AppKit phase transition. */
  phase(p: ScrollPhase, t: number): SwipeEffect[] {
    this.seq.phase(p);
    const effects: SwipeEffect[] = [];
    if (this.tuning.release === "native" && this.stage === "tracking") {
      if (p.phase === "ended") effects.push(...this.release(t, "fingers lifted"));
      else if (p.phase === "cancelled") effects.push(...this.release(t, "cancelled", true));
    }
    if (this.stage === "deciding" && (p.phase === "ended" || p.phase === "cancelled")) this.stage = "idle";
    // The stream is over: the next touch starts afresh. Not on the fingers' "ended": momentum's
    // "began" comes after it, and a lift without momentum is caught by tick().
    if ((this.stage === "coasting" || this.stage === "passthrough") && (p.momentum === "ended" || p.momentum === "cancelled" || p.phase === "began" || p.phase === "mayBegin")) {
      this.stage = "idle";
    }
    return effects;
  }

  /** Called a little after the last event: ends a swipe, or a stream, that went quiet. */
  tick(t: number): SwipeEffect[] {
    if (t - this.lastT <= this.tuning.idleMs) return [];
    if (this.stage === "tracking") {
      // With native phases, "ended" decides; a quiet finger still on the pad is not a lift.
      if (this.tuning.release === "native" && this.seq.fingersAt(t)) return [];
      return this.release(t, "idle gap");
    }
    if (this.stage === "deciding") this.stage = "idle";
    if ((this.stage === "coasting" || this.stage === "passthrough") && !(this.seq.fingersAt(t) || this.seq.momentumAt(t))) this.stage = "idle";
    return [];
  }

  /** Finger travel from a delta, by the direction setting. */
  private fingers(dx: number): number {
    const inverted = this.tuning.direction === "auto" ? this.seq.inverted : this.tuning.direction === "natural";
    return inverted ? -dx : dx;
  }

  private move(d: number, t: number): SwipeEffect[] {
    const tun = this.tuning;
    this.raw += d;
    if (!this.sides[1] && this.raw > 0) this.raw = 0;
    if (!this.sides[-1] && this.raw < 0) this.raw = 0;
    this.offset = shapeTravel(this.raw, this.width, tun);
    this.samples.push({ t, raw: this.raw });
    while (this.samples.length > 2 && t - (this.samples[0]?.t ?? t) > 80) this.samples.shift();

    const effects: SwipeEffect[] = [];
    const a = Math.abs(this.offset);
    const side: Side = this.offset >= 0 ? 1 : -1;
    const thr = tun.threshold * this.width;
    // Detents: thr/detentPx ticks on the way to the threshold, crowding towards it with the ramp.
    // None at the threshold itself (that's the arm haptic) or past it.
    const detent = detentIndex(Math.min(a, thr), thr, tun);
    if (detent !== this.detent) {
      const top = Math.max(detent, this.detent);
      this.detent = detent;
      if (this.armed === 0 && a < thr && top < detentCount(thr, tun)) {
        effects.push(...this.haptic(tun.hapticDetent, "detent"));
      }
    }
    if (this.armed === 0 && a >= thr && this.offset !== 0) {
      this.armed = side;
      effects.push(...this.haptic(tun.hapticArm, "arm"));
      if (tun.commit === "threshold") effects.push(...this.commitNow(side, "threshold", t));
    } else if (this.armed !== 0 && (this.armed !== side || a < thr - tun.hysteresis * this.width)) {
      this.armed = 0;
      effects.push(...this.haptic(tun.hapticDisarm, "disarm"));
    }
    return effects;
  }

  /** px/ms over the last ~80 ms of finger travel. */
  velocity(): number {
    const s = this.samples;
    const first = s[0];
    const last = s[s.length - 1];
    if (!first || !last || s.length < 2) return 0;
    const dt = last.t - first.t;
    return dt > 0 ? (last.raw - first.raw) / dt : 0;
  }

  private decaying(abs: number): boolean {
    this.lastAbs.push(abs);
    const n = this.tuning.decayEvents;
    if (this.lastAbs.length > n + 1) this.lastAbs.shift();
    if (this.lastAbs.length < n + 1) return false;
    for (let i = 1; i < this.lastAbs.length; i++) if (!((this.lastAbs[i] ?? 0) < (this.lastAbs[i - 1] ?? 0))) return false;
    return (this.lastAbs[0] ?? 0) >= 1;
  }

  private commitNow(side: Side, how: "threshold" | "release" | "fling", t: number): SwipeEffect[] {
    const effects: SwipeEffect[] = [{ kind: "commit", side, how }];
    effects.push(...this.haptic(this.tuning.hapticCommit, "commit"));
    effects.push(...this.settle(t, true, how === "threshold" ? "committed at threshold" : how === "fling" ? "fling" : "released past threshold"));
    return effects;
  }

  private release(t: number, reason: string, cancelled = false): SwipeEffect[] {
    if (this.stage !== "tracking") return [];
    const v = this.velocity();
    const fling = this.tuning.flingVelocity;
    if (!cancelled && this.armed !== 0) return this.commitNow(this.armed, "release", t);
    if (!cancelled && fling > 0 && Math.abs(v) >= fling && this.sides[v > 0 ? 1 : -1] && Math.sign(v) === Math.sign(this.offset)) {
      return this.commitNow(v > 0 ? 1 : -1, "fling", t);
    }
    return this.settle(t, false, reason);
  }

  private settle(_t: number, committed: boolean, reason: string): SwipeEffect[] {
    this.stage = "coasting";
    this.raw = 0;
    this.offset = 0;
    this.armed = 0;
    this.samples = [];
    this.detent = 0;
    return [{ kind: "release", committed, reason }];
  }

  private haptic(choice: HapticChoice, reason: HapticReason): SwipeEffect[] {
    return choice === "none" ? [] : [{ kind: "haptic", pattern: choice, reason }];
  }
}

/** The wheel event fields `coarseWheel` reads. */
export interface WheelLike {
  deltaX: number;
  deltaY: number;
  /** 0: pixels; 1: lines; 2: pages. */
  deltaMode: number;
  shiftKey: boolean;
}

/** Above this in one event (px), the first event of a stream is a mouse wheel's notch, not fingers. */
export const NOTCH_PX = 50;

/**
 * Whether a wheel event looks like a mouse wheel rather than a trackpad: lines or pages, Shift held
 * (Shift turns a wheel sideways), or a coarse jump no finger starts with (Windows reports a notch
 * as ~100 px, Linux as ~53, or multiples of 120). Only the first event of a stream decides:
 * trackpad momentum gets as large later on.
 */
export function coarseWheel(e: WheelLike): boolean {
  if (e.deltaMode !== 0 || e.shiftKey) return true;
  return Math.max(Math.abs(e.deltaX), Math.abs(e.deltaY)) >= NOTCH_PX;
}

export type Platform = "macOs" | "windows" | "linux";

/**
 * How a swipe ends by default on each platform: AppKit's phases on macOS; on Windows the decaying
 * deltas, as precision touchpads coast through Chromium (with the idle gap a flick would only end
 * once the coasting stops); on Linux the idle gap.
 */
export function defaultRelease(platform: Platform): ReleaseMode {
  return platform === "macOs" ? "native" : platform === "windows" ? "momentum" : "idle";
}

/**
 * "Follow system" by platform. macOS says (AppKit's natural scrolling flag). Elsewhere nothing
 * does: precision touchpads on Windows and GNOME's touchpad default both scroll naturally.
 */
export function systemDirection(platform: Platform): DirectionMode {
  return platform === "macOs" ? "auto" : "natural";
}

export type SwipeSurface = "rows" | "queue" | "cover" | "page";
export type Sensitivity = "low" | "medium" | "high";
/** The direction setting: "system" follows the platform (`systemDirection`). */
export type DirectionPref = "system" | "natural" | "reversed";

/** Each surface's tuning before the person's settings. */
export const SURFACE_TUNING: Record<SwipeSurface, SwipeTuning> = {
  rows: DEFAULT_TUNING,
  queue: DEFAULT_TUNING,
  // Swipe the artwork to skip: further to go, a short leash, and the skip says it all (no commit haptic).
  cover: { ...DEFAULT_TUNING, threshold: 0.4, maxTravel: 0.35, resistance: 0.25, flingVelocity: 1.2, hapticCommit: "none" },
  // Back and forward over the page: a short pull with a lot of resistance.
  page: { ...DEFAULT_TUNING, threshold: 0.16, maxTravel: 0.3, resistance: 0.6, flingVelocity: 1, hapticDisarm: "none", hapticCommit: "none" },
};

/** How far a swipe has to go, by sensitivity: of the surface's threshold (high = shorter). */
export const SENSITIVITY_SCALE: Record<Sensitivity, number> = { low: 1.35, medium: 1, high: 0.7 };

export interface TuningPrefs {
  sensitivity: Sensitivity;
  direction: DirectionPref;
  haptics: boolean;
}

/**
 * A surface's tuning on a platform with the person's settings: sensitivity scales the threshold
 * (and the detent spacing and the fling speed with it, so the feel stays the same, only shorter or
 * longer), the direction setting picks the direction, and haptics off silences every haptic.
 */
export function resolveTuning(surface: SwipeSurface, platform: Platform, prefs: TuningPrefs): SwipeTuning {
  const base = SURFACE_TUNING[surface];
  const k = SENSITIVITY_SCALE[prefs.sensitivity] ?? 1;
  const t: SwipeTuning = {
    ...base,
    threshold: base.threshold * k,
    // The threshold must stay inside the travel.
    maxTravel: Math.max(base.maxTravel, base.threshold * k + 0.05),
    detentPx: base.detentPx * k,
    flingVelocity: base.flingVelocity * k,
    release: base.release === "native" ? defaultRelease(platform) : base.release,
    direction: prefs.direction === "system" ? systemDirection(platform) : prefs.direction,
  };
  if (!prefs.haptics || platform !== "macOs") {
    t.hapticStart = t.hapticDetent = t.hapticArm = t.hapticDisarm = t.hapticCommit = "none";
  }
  return t;
}
