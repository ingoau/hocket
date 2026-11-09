// The Transitions setting (Off / Crossfade / AutoMix and the crossfade length),
// as the Audio settings page edits it. The core owns the value
// (`playback.transitions`, synced): read with Query transitions, written with
// Command setTransitionSettings.
import type { TransitionMode, TransitionSettings } from "@core/api";
import { TransitionModeValues } from "@core/api";

export const CROSSFADE_MIN = 1;
export const CROSSFADE_MAX = 12;

/** The core's default: Off, with a 6 s crossfade ready for when Crossfade is picked. */
export const DEFAULT_TRANSITIONS: TransitionSettings = { mode: "off", crossfadeSeconds: 6 };

/** Whole seconds in 1..=12; anything unreadable falls back to the default length. */
export function clampCrossfade(seconds: number): number {
  if (!Number.isFinite(seconds)) return DEFAULT_TRANSITIONS.crossfadeSeconds;
  return Math.min(CROSSFADE_MAX, Math.max(CROSSFADE_MIN, Math.round(seconds)));
}

export function isTransitionMode(v: string): v is TransitionMode {
  return (TransitionModeValues as readonly string[]).includes(v);
}

/** Settings with `mode` picked, keeping the crossfade length (AutoMix bases its late-edit fade on it). */
export function withMode(s: TransitionSettings, mode: TransitionMode): TransitionSettings {
  return { mode, crossfadeSeconds: clampCrossfade(s.crossfadeSeconds) };
}

export function withCrossfade(s: TransitionSettings, seconds: number): TransitionSettings {
  return { ...s, crossfadeSeconds: clampCrossfade(seconds) };
}
