// What's pinned to the Library (`library.pins`, synced), shown in the sidebar under Home and the
// Library. Each pin is an album, artist, playlist, song, genre or filter, with what clicking it
// does: open it, or play / shuffle it (those two for albums, artists and playlists only).
import { SK } from "@shared/settings-keys";

export type PinKind = "album" | "artist" | "playlist" | "track" | "genre" | "filter";
export type PinTap = "open" | "play" | "shuffle";

export interface LibraryPin {
  kind: string;
  id: string;
  tap: string;
  /** Fields a newer device added, written back untouched. */
  [field: string]: unknown;
}

/** Most pins the Library keeps (the core's validator refuses more). */
export const LIBRARY_PINS_MAX = 200;

export const PIN_KINDS: readonly PinKind[] = ["album", "artist", "playlist", "track", "genre", "filter"];
export const PIN_TAPS: readonly PinTap[] = ["open", "play", "shuffle"];

export function isPinKind(kind: string): kind is PinKind {
  return (PIN_KINDS as readonly string[]).includes(kind);
}

/** Play and Shuffle are offered as the click for albums, artists and playlists only. */
export function choosesTap(kind: string): boolean {
  return kind === "album" || kind === "artist" || kind === "playlist";
}

export function pinTap(p: LibraryPin): PinTap {
  return (PIN_TAPS as readonly string[]).includes(p.tap) && (choosesTap(p.kind) || p.tap === "open") ? (p.tap as PinTap) : "open";
}

export function parsePins(value: unknown): LibraryPin[] {
  if (!Array.isArray(value)) return [];
  const out: LibraryPin[] = [];
  for (const e of value) {
    if (!e || typeof e !== "object") continue;
    const o = e as Record<string, unknown>;
    if (typeof o.kind !== "string" || typeof o.id !== "string" || !o.id) continue;
    if (out.some((p) => p.kind === o.kind && p.id === o.id)) continue;
    out.push({ ...o, kind: o.kind, id: o.id, tap: typeof o.tap === "string" ? o.tap : "open" });
  }
  return out;
}

export function isPinned(pins: readonly LibraryPin[], kind: PinKind, id: string): boolean {
  return pins.some((p) => p.kind === kind && p.id === id);
}

export function canPin(pins: readonly LibraryPin[]): boolean {
  return pins.length < LIBRARY_PINS_MAX;
}

/**
 * Pins or unpins; a song plays when clicked, everything else opens (as the core's action does). A
 * full Library is left as it is.
 */
export function togglePin(pins: readonly LibraryPin[], kind: PinKind, id: string): LibraryPin[] {
  if (isPinned(pins, kind, id)) return pins.filter((p) => !(p.kind === kind && p.id === id));
  return canPin(pins) ? [...pins, { kind, id, tap: kind === "track" ? "play" : "open" }] : [...pins];
}

export function setTap(pins: readonly LibraryPin[], pin: LibraryPin, tap: PinTap): LibraryPin[] {
  return pins.map((p) => (p.kind === pin.kind && p.id === pin.id ? { ...p, tap } : p));
}

const same = (a: LibraryPin, b: LibraryPin) => a.kind === b.kind && a.id === b.id;

/**
 * The pin [by] places from [pin] among [shown] (the pins drawn: one whose album or filter is gone
 * isn't), or undefined at either end.
 */
export function shownNeighbour(shown: readonly LibraryPin[], pin: LibraryPin, by: -1 | 1): LibraryPin | undefined {
  const i = shown.findIndex((p) => same(p, pin));
  return i < 0 ? undefined : shown[i + by];
}

/**
 * Moves [pin] one place earlier or later among [shown], past its shown neighbour; pins that aren't
 * shown keep their place in the stored list.
 */
export function movePin(pins: readonly LibraryPin[], pin: LibraryPin, by: -1 | 1, shown: readonly LibraryPin[] = pins): LibraryPin[] {
  const other = shownNeighbour(shown, pin, by);
  const moved = pins.find((p) => same(p, pin));
  if (!other || !moved) return [...pins];
  const list = pins.filter((p) => !same(p, pin));
  const j = list.findIndex((p) => same(p, other));
  if (j < 0) return [...pins];
  list.splice(by < 0 ? j : j + 1, 0, moved);
  return list;
}

export function removePin(pins: readonly LibraryPin[], pin: LibraryPin): { pins: LibraryPin[]; at: number } {
  const at = pins.findIndex((p) => same(p, pin));
  return { pins: pins.filter((_, i) => i !== at), at };
}

/** Puts [pin] back where it was, for Undo; the rest of the list stays as it is now. */
export function restorePin(pins: readonly LibraryPin[], pin: LibraryPin, at: number): LibraryPin[] {
  const list = pins.filter((p) => !same(p, pin));
  if (!canPin(list)) return list;
  list.splice(Math.max(0, Math.min(at, list.length)), 0, pin);
  return list;
}

export const LIBRARY_PINS_KEY = SK.libraryPins;
