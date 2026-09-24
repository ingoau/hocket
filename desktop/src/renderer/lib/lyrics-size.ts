// Device-local lyrics size preference for the in-window lyrics view. Not a
// registry setting on purpose: it is about this screen, not the account.
import { useSyncExternalStore } from "react";
import { loadLocal, saveLocal } from "./local-settings";

export type LyricsSize = "small" | "medium" | "large";
export const LYRICS_SIZES: readonly LyricsSize[] = ["small", "medium", "large"];
/** Multiplier applied to the viewport-derived base size. */
export const LYRICS_SCALE: Record<LyricsSize, number> = { small: 0.82, medium: 1, large: 1.25 };

const listeners = new Set<() => void>();
let current: LyricsSize = normalise(loadLocal<string>("lyricsSize", "medium"));

function normalise(v: string): LyricsSize {
  return (LYRICS_SIZES as readonly string[]).includes(v) ? (v as LyricsSize) : "medium";
}

export function getLyricsSize(): LyricsSize {
  return current;
}

export function setLyricsSize(v: LyricsSize): void {
  current = normalise(v);
  saveLocal("lyricsSize", current);
  for (const l of listeners) l();
}

function subscribe(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

export function useLyricsSize(): LyricsSize {
  return useSyncExternalStore(subscribe, getLyricsSize, getLyricsSize);
}
