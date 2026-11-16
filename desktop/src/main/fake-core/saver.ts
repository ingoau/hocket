// Battery saver levels, as crates/hocket-core/src/saver.rs resolves them.
import type { BatterySaver, SaverFeature, SaverLevel } from "@core/api";
import { SaverFeatureValues } from "@core/api";

const LIGHT: SaverFeature[] = ["stillBackground", "stillSeekWave", "lighterLyrics"];
const MEDIUM: SaverFeature[] = [...LIGHT, "refreshCap", "slowSync"];
const MAX: SaverFeature[] = [...MEDIUM, "simpleLyrics", "noImmersiveArtwork", "noArtworkBlur", "smallArtwork"];

/** `battery.custom`'s default: medium's features. */
export const DEFAULT_CUSTOM: SaverFeature[] = MEDIUM;

const PRESETS: Record<SaverLevel, SaverFeature[]> = { off: [], light: LIGHT, medium: MEDIUM, max: MAX, custom: [] };
const RANK: Partial<Record<SaverLevel, number>> = { off: 0, light: 1, medium: 2, max: 3 };

/** What `level` turns off, in the core's order and without repeats. */
export function saverFeatures(level: SaverLevel, custom: SaverFeature[]): SaverFeature[] {
  const set = new Set(level === "custom" ? custom : PRESETS[level]);
  return SaverFeatureValues.filter((f) => set.has(f));
}

/** The battery saver in force: power save raises the chosen level to `autoLevel`, never lowers it; custom only replaces "off". */
export function resolveSaver(chosen: SaverLevel, autoLevel: SaverLevel, autoEngage: boolean, powerSave: boolean, custom: SaverFeature[]): BatterySaver {
  const c = RANK[chosen];
  const a = RANK[autoLevel];
  const automatic = autoEngage && powerSave && autoLevel !== "off" && (c === 0 || (c !== undefined && a !== undefined && a > c));
  const level = automatic ? autoLevel : chosen;
  return { level, automatic, features: saverFeatures(level, custom) };
}

/** A `battery.level` / `battery.autoLevel` value, or `fallback` when it isn't one. */
export function parseLevel(value: unknown, fallback: SaverLevel): SaverLevel {
  return typeof value === "string" && value in PRESETS ? (value as SaverLevel) : fallback;
}

/** `battery.custom`'s value: known feature names only. */
export function parseCustom(value: unknown): SaverFeature[] {
  return Array.isArray(value) ? value.filter((f): f is SaverFeature => (SaverFeatureValues as readonly string[]).includes(f)) : [];
}
