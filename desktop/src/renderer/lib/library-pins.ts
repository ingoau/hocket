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
}

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
    out.push({ kind: o.kind, id: o.id, tap: typeof o.tap === "string" ? o.tap : "open" });
  }
  return out;
}

export function isPinned(pins: readonly LibraryPin[], kind: PinKind, id: string): boolean {
  return pins.some((p) => p.kind === kind && p.id === id);
}

/** Pins or unpins; a song plays when clicked, everything else opens (as the core's action does). */
export function togglePin(pins: readonly LibraryPin[], kind: PinKind, id: string): LibraryPin[] {
  return isPinned(pins, kind, id) ? pins.filter((p) => !(p.kind === kind && p.id === id)) : [...pins, { kind, id, tap: kind === "track" ? "play" : "open" }];
}

export function setTap(pins: readonly LibraryPin[], pin: LibraryPin, tap: PinTap): LibraryPin[] {
  return pins.map((p) => (p.kind === pin.kind && p.id === pin.id ? { ...p, tap } : p));
}

export function movePin(pins: readonly LibraryPin[], pin: LibraryPin, by: number): LibraryPin[] {
  const i = pins.findIndex((p) => p.kind === pin.kind && p.id === pin.id);
  const j = i + by;
  if (i < 0 || j < 0 || j >= pins.length) return [...pins];
  const list = [...pins];
  const [moved] = list.splice(i, 1);
  if (moved) list.splice(j, 0, moved);
  return list;
}

export function removePin(pins: readonly LibraryPin[], pin: LibraryPin): { pins: LibraryPin[]; at: number } {
  const at = pins.findIndex((p) => p.kind === pin.kind && p.id === pin.id);
  return { pins: pins.filter((_, i) => i !== at), at };
}

/** Puts [pin] back where it was, for Undo. */
export function restorePin(pins: readonly LibraryPin[], pin: LibraryPin, at: number): LibraryPin[] {
  const list = pins.filter((p) => !(p.kind === pin.kind && p.id === pin.id));
  list.splice(Math.max(0, Math.min(at, list.length)), 0, pin);
  return list;
}

export const LIBRARY_PINS_KEY = SK.libraryPins;
