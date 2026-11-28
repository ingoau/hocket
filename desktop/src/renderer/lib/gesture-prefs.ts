// Device-local: trackpad swipe gestures (Settings, Gestures). Which surfaces swipe, how far a
// swipe goes, which way the fingers run, and whether macOS plays haptics. Stored with the other
// local UI conveniences: the trackpad is this machine's. What a row's swipe does is the core's
// synced `swipe.list.*` / `swipe.queue.*` / `swipe.stash.*`, shared with the phone.
import { useMemo, useSyncExternalStore } from "react";
import { loadLocal, saveLocal } from "./local-settings";
import { resolveTuning, type DirectionPref, type Platform, type Sensitivity, type SwipeSurface, type SwipeTuning } from "./swipe";
import { useApp } from "../store/app";

export interface GesturePrefs {
  /** Every swipe gesture at once. */
  enabled: boolean;
  /** Per surface, under `enabled`. */
  surfaces: Record<SwipeSurface, boolean>;
  sensitivity: Sensitivity;
  direction: DirectionPref;
  /** macOS only: the trackpad's haptic clicks. */
  haptics: boolean;
}

export const DEFAULT_GESTURE_PREFS: GesturePrefs = {
  enabled: true,
  surfaces: { rows: true, queue: true, stash: true, cover: true, page: true },
  sensitivity: "medium",
  direction: "system",
  haptics: true,
};

const KEY = "gestures";
const SENSITIVITIES: readonly Sensitivity[] = ["low", "medium", "high"];
const DIRECTIONS: readonly DirectionPref[] = ["system", "natural", "reversed"];

/** Whatever was stored, made whole: unknown values fall back to the defaults. */
export function normaliseGesturePrefs(raw: unknown): GesturePrefs {
  const d = DEFAULT_GESTURE_PREFS;
  const o = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const s = o.surfaces && typeof o.surfaces === "object" ? (o.surfaces as Record<string, unknown>) : {};
  const bool = (v: unknown, f: boolean) => (typeof v === "boolean" ? v : f);
  return {
    enabled: bool(o.enabled, d.enabled),
    surfaces: { rows: bool(s.rows, true), queue: bool(s.queue, true), stash: bool(s.stash, true), cover: bool(s.cover, true), page: bool(s.page, true) },
    sensitivity: SENSITIVITIES.includes(o.sensitivity as Sensitivity) ? (o.sensitivity as Sensitivity) : d.sensitivity,
    direction: DIRECTIONS.includes(o.direction as DirectionPref) ? (o.direction as DirectionPref) : d.direction,
    haptics: bool(o.haptics, d.haptics),
  };
}

let current = normaliseGesturePrefs(loadLocal<unknown>(KEY, {}));
const listeners = new Set<() => void>();

export function setGesturePrefs(patch: Partial<Omit<GesturePrefs, "surfaces">> & { surfaces?: Partial<GesturePrefs["surfaces"]> }): void {
  current = normaliseGesturePrefs({ ...current, ...patch, surfaces: { ...current.surfaces, ...patch.surfaces } });
  saveLocal(KEY, current);
  for (const l of listeners) l();
}

const get = () => current;
const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => listeners.delete(l);
};

export function useGesturePrefs(): GesturePrefs {
  return useSyncExternalStore(subscribe, get, get);
}

/** A surface's tuning with this device's settings, or null while that surface's swipe is off. */
export function useSurfaceTuning(surface: SwipeSurface): SwipeTuning | null {
  const prefs = useGesturePrefs();
  const platform: Platform = useApp((s) => s.meta?.platform ?? "linux");
  const on = prefs.enabled && prefs.surfaces[surface];
  return useMemo(() => (on ? resolveTuning(surface, platform, prefs) : null), [on, surface, platform, prefs]);
}
