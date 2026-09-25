// Icons are Material Symbols (rounded; Apache-2.0), generated into
// material-icons.ts by scripts/gen-icons.mjs: one name → glyph map, keyed by
// the short names the renderer uses, plus the Material Symbols names the
// action registry emits in `ActionDescriptor.icon`. Each glyph has an
// outlined and a filled variant: outlined by default (transport glyphs are
// filled), `filled` picks explicitly (a loved heart, a set star). Nothing in
// the UI draws an icon from a unicode glyph; anything unknown renders as a
// music note so a new registry icon is visible (and `icon.test.ts` fails)
// rather than blank.
import type { CSSProperties } from "react";
import { MATERIAL_ICONS } from "./material-icons";

/** Solid shapes read better filled; everything else defaults to outlined. */
const FILLED_BY_DEFAULT = new Set(["play", "pause", "stop", "next", "previous", "rewind", "forward"]);

/** Material Symbols names (what the registry's descriptors carry) → renderer names. */
export const MATERIAL: Record<string, string> = {
  play_arrow: "play", play_pause: "play", pause: "pause", stop: "stop", skip_next: "next", skip_previous: "previous",
  forward_10: "forward", replay_10: "rewind", volume_up: "volume", volume_down: "volume", playlist_play: "playNext",
  playlist_add: "playLater", remove_from_queue: "remove", delete: "trash", shuffle: "shuffle", shuffle_on: "shuffle",
  repeat: "repeat", all_inclusive: "autoplay", clear_all: "trash", playlist_remove: "remove", playlist_add_check: "playlistAdd",
  history: "restore", push_pin: "pin", keep_off: "pinOff", delete_forever: "trash", star_rate: "star", star_outline: "starOff",
  star: "star", favorite: "heart", heart_minus: "heartOff", download: "download", download_done: "download", file_download_off: "downloadOff",
  album: "album", artist: "artist", person: "artist", undo: "undo", redo: "redo", select_all: "selectAll",
  keyboard_command_key: "command", search: "search", queue_music: "queue", fullscreen: "fullscreen",
  picture_in_picture_alt: "mini", lyrics: "lyrics", bedtime: "sleep", bedtime_off: "sleep", cast: "devices",
  play_circle: "resume", close: "close", bug_report: "bug", home: "home", music_note: "song", category: "genre",
  insights: "stats", settings: "settings", filter_alt: "filter", info: "info", library_music: "library",
};

export interface IconProps {
  name: string;
  size?: number;
  className?: string;
  style?: CSSProperties;
  title?: string;
  /** Filled or outlined variant; defaults per glyph. */
  filled?: boolean;
}

/** Resolves a renderer or Material name to the renderer name that has a glyph. */
export function iconName(name: string): string {
  return MATERIAL_ICONS[name] ? name : (MATERIAL[name] ?? "music");
}

const escapeText = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

export function Icon({ name, size = 16, className, style, title, filled }: IconProps) {
  const n = iconName(name);
  const [outline, solid] = MATERIAL_ICONS[n] ?? MATERIAL_ICONS.music ?? [""];
  const fill = filled ?? FILLED_BY_DEFAULT.has(n);
  const body = fill ? (solid ?? outline) : outline;
  return (
    <svg className={`icon ${className ?? ""}`} width={size} height={size} viewBox="0 0 24 24" style={style} aria-hidden={title ? undefined : true} role={title ? "img" : undefined} aria-label={title} data-icon={n}
      dangerouslySetInnerHTML={{ __html: (title ? `<title>${escapeText(title)}</title>` : "") + body }} />
  );
}

export function hasIcon(name: string): boolean {
  return name in MATERIAL_ICONS || name in MATERIAL;
}

/** Every name the renderer can draw (for tests). */
export const ICON_NAMES: readonly string[] = Object.keys(MATERIAL_ICONS);
