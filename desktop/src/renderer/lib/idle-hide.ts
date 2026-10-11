// Device-local: whether the fullscreen player's controls fade out while the
// mouse is idle over the immersive artwork (Settings, Appearance). On by
// default; stored with the other local UI conveniences.
import { useSyncExternalStore } from "react";
import { loadLocal, saveLocal } from "./local-settings";

/** How long the mouse rests before the controls fade. */
export const IDLE_HIDE_MS = 3000;

let enabled = loadLocal<boolean>("fullscreenIdleHide", true) !== false;
const listeners = new Set<() => void>();

export function setIdleHide(v: boolean): void {
  enabled = v;
  saveLocal("fullscreenIdleHide", v);
  for (const l of listeners) l();
}

const get = () => enabled;

export function useIdleHide(): boolean {
  return useSyncExternalStore((l) => { listeners.add(l); return () => listeners.delete(l); }, get, get);
}
