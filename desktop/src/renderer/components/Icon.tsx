// Inline SVG icons (Lucide-style, 24 grid, stroke). Keyed by the icon names the
// action registry emits, so menus/palette/sidebar render without a mapping table.
import type { CSSProperties } from "react";

const PATHS: Record<string, string> = {
  play: "M6 4l14 8-14 8z",
  pause: "M7 4h4v16H7zM13 4h4v16h-4z",
  stop: "M6 6h12v12H6z",
  next: "M5 4l10 8-10 8zM17 4h2v16h-2z",
  previous: "M19 4L9 12l10 8zM5 4h2v16H5z",
  rewind: "M11 19l-9-7 9-7zM22 19l-9-7 9-7z",
  forward: "M13 19l9-7-9-7zM2 19l9-7-9-7z",
  shuffle: "M16 3h5v5M4 20L21 3M21 16v5h-5M15 15l6 6M4 4l5 5",
  repeat: "M17 1l4 4-4 4M3 11V9a4 4 0 014-4h14M7 23l-4-4 4-4M21 13v2a4 4 0 01-4 4H3",
  repeatOne: "M17 1l4 4-4 4M3 11V9a4 4 0 014-4h14M7 23l-4-4 4-4M21 13v2a4 4 0 01-4 4H3M11 10h2v5",
  autoplay: "M12 2a10 10 0 1010 10M12 2v10l4 4",
  volume: "M11 5L6 9H2v6h4l5 4zM15.5 8.5a5 5 0 010 7M19 5a10 10 0 010 14",
  mute: "M11 5L6 9H2v6h4l5 4zM23 9l-6 6M17 9l6 6",
  heart: "M20.8 4.6a5.5 5.5 0 00-7.8 0L12 5.7l-1-1.1a5.5 5.5 0 00-7.8 7.8l1 1L12 21l7.8-7.8 1-1a5.5 5.5 0 000-7.8z",
  heartOff: "M20.8 4.6a5.5 5.5 0 00-7.8 0L12 5.7l-1-1.1a5.5 5.5 0 00-7.8 7.8l1 1L12 21l7.8-7.8 1-1a5.5 5.5 0 000-7.8zM3 3l18 18",
  star: "M12 2l3.1 6.3 6.9 1-5 4.9 1.2 6.8L12 17.8 5.8 21l1.2-6.8-5-4.9 6.9-1z",
  starOff: "M12 2l3.1 6.3 6.9 1-5 4.9 1.2 6.8L12 17.8 5.8 21l1.2-6.8-5-4.9 6.9-1zM3 3l18 18",
  queue: "M3 6h13M3 12h13M3 18h9M18 14v6M15 17h6",
  lyrics: "M4 5h16M4 10h16M4 15h10M4 20h6",
  fullscreen: "M8 3H5a2 2 0 00-2 2v3M21 8V5a2 2 0 00-2-2h-3M3 16v3a2 2 0 002 2h3M16 21h3a2 2 0 002-2v-3",
  mini: "M4 4h16v10H4zM8 20h8",
  devices: "M2 7h13v10H2zM18 9h4v8h-4z",
  search: "M11 4a7 7 0 100 14 7 7 0 000-14zM21 21l-5-5",
  command: "M18 3a3 3 0 00-3 3v12a3 3 0 103-3H6a3 3 0 103 3V6a3 3 0 10-3 3h12a3 3 0 100-6z",
  settings: "M12 15a3 3 0 100-6 3 3 0 000 6zM19.4 15a1.7 1.7 0 00.3 1.8l.1.1a2 2 0 11-2.8 2.8l-.1-.1a1.7 1.7 0 00-1.8-.3 1.7 1.7 0 00-1 1.5V21a2 2 0 11-4 0v-.1a1.7 1.7 0 00-1.1-1.5 1.7 1.7 0 00-1.8.3l-.1.1a2 2 0 11-2.8-2.8l.1-.1a1.7 1.7 0 00.3-1.8 1.7 1.7 0 00-1.5-1H3a2 2 0 110-4h.1a1.7 1.7 0 001.5-1.1 1.7 1.7 0 00-.3-1.8l-.1-.1a2 2 0 112.8-2.8l.1.1a1.7 1.7 0 001.8.3H9a1.7 1.7 0 001-1.5V3a2 2 0 114 0v.1a1.7 1.7 0 001 1.5 1.7 1.7 0 001.8-.3l.1-.1a2 2 0 112.8 2.8l-.1.1a1.7 1.7 0 00-.3 1.8V9a1.7 1.7 0 001.5 1H21a2 2 0 110 4h-.1a1.7 1.7 0 00-1.5 1z",
  home: "M3 10l9-7 9 7v10a2 2 0 01-2 2h-4v-7h-6v7H5a2 2 0 01-2-2z",
  album: "M12 2a10 10 0 100 20 10 10 0 000-20zM12 9a3 3 0 100 6 3 3 0 000-6z",
  artist: "M12 3a4 4 0 100 8 4 4 0 000-8zM4 21a8 8 0 0116 0",
  playlist: "M3 6h12M3 12h12M3 18h8M19 6v10M16 16a3 3 0 106 0 3 3 0 00-6 0",
  playlistAdd: "M3 6h12M3 12h12M3 18h8M18 12v8M14 16h8",
  song: "M9 18V5l12-2v13M9 18a3 3 0 11-6 0 3 3 0 016 0zM21 16a3 3 0 11-6 0 3 3 0 016 0z",
  genre: "M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z",
  download: "M12 3v12M6 11l6 6 6-6M4 21h16",
  downloadOff: "M12 3v12M6 11l6 6 6-6M4 21h16M3 3l18 18",
  filter: "M3 4h18l-7 9v6l-4 2v-8z",
  filterAdd: "M3 4h18l-7 9v6l-4 2v-8zM19 15v6M16 18h6",
  stats: "M4 20V10M10 20V4M16 20v-8M22 20H2",
  info: "M12 2a10 10 0 100 20 10 10 0 000-20zM12 16v-4M12 8h.01",
  remove: "M5 12h14",
  trash: "M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14",
  edit: "M12 20h9M16.5 3.5a2.1 2.1 0 013 3L7 19l-4 1 1-4z",
  close: "M18 6L6 18M6 6l12 12",
  check: "M20 6L9 17l-5-5",
  chevronDown: "M6 9l6 6 6-6",
  chevronRight: "M9 6l6 6-6 6",
  chevronLeft: "M15 6l-6 6 6 6",
  chevronUp: "M6 15l6-6 6 6",
  back: "M19 12H5M12 19l-7-7 7-7",
  forwardArrow: "M5 12h14M12 5l7 7-7 7",
  undo: "M3 7v6h6M3 13a9 9 0 0117-4",
  redo: "M21 7v6h-6M21 13a9 9 0 00-17-4",
  playNext: "M3 6h10M3 12h10M3 18h6M15 10l6 4-6 4z",
  playLater: "M3 6h10M3 12h10M3 18h6M17 14v6M14 17h6",
  pin: "M12 17v5M5 10l7-7 7 7-4 1v4H9v-4z",
  pinOff: "M12 17v5M5 10l7-7 7 7-4 1v4H9v-4zM3 3l18 18",
  restore: "M3 12a9 9 0 109-9M3 3v6h6",
  seek: "M4 12h16M12 6l6 6-6 6",
  sleep: "M21 12.8A9 9 0 1111.2 3a7 7 0 009.8 9.8z",
  bug: "M8 2l2 2M16 2l-2 2M9 8h6M12 8v13M5 13H2M22 13h-3M6 18l-3 2M18 18l3 2M6 8L3 6M18 8l3-2M7 12a5 5 0 0110 0v4a5 5 0 01-10 0z",
  selectAll: "M4 4h16v16H4zM9 12l2 2 4-4",
  grip: "M9 5h.01M9 12h.01M9 19h.01M15 5h.01M15 12h.01M15 19h.01",
  more: "M12 5h.01M12 12h.01M12 19h.01",
  minimize: "M5 12h14",
  maximize: "M4 4h16v16H4z",
  restoreWin: "M8 8h12v12H8zM4 16V4h12",
  spinner: "M12 2a10 10 0 019.5 7",
  offline: "M1 1l22 22M16.7 11.4A6 6 0 0018 9M5 12.5a10 10 0 0114-3M2 8.8a15 15 0 015.5-3M12 20h.01",
  warn: "M12 2L1 21h22zM12 9v5M12 18h.01",
  battery: "M2 7h18v10H2zM22 10v4",
  external: "M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6M15 3h6v6M10 14L21 3",
  folder: "M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v9a2 2 0 01-2 2H5a2 2 0 01-2-2z",
  copy: "M8 8h12v12H8zM16 8V4H4v12h4",
  resume: "M4 12a8 8 0 018-8h8M20 4v4h-4M6 20l6-4-6-4z",
  music: "M9 18V5l12-2v13M9 18a3 3 0 11-6 0 3 3 0 016 0zM21 16a3 3 0 11-6 0 3 3 0 016 0z",
  cloud: "M18 10h-1.3A7 7 0 104 15h14a4 4 0 000-5z",
  lan: "M4 20h16M12 4v16M4 20a8 8 0 0116 0",
  plus: "M12 5v14M5 12h14",
};

export interface IconProps {
  name: string;
  size?: number;
  className?: string;
  style?: CSSProperties;
  title?: string;
}

/** Material Symbols names (what the registry's descriptors carry) → local glyphs. */
const MATERIAL: Record<string, string> = {
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
  insights: "stats", settings: "settings", filter_alt: "filter", info: "info",
};

export function iconName(name: string): string {
  return PATHS[name] ? name : (MATERIAL[name] ?? "music");
}

export function Icon({ name, size = 16, className, style, title }: IconProps) {
  const d = PATHS[iconName(name)] ?? PATHS.music;
  return (
    <svg className={`icon ${className ?? ""}`} width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.9} strokeLinecap="round" strokeLinejoin="round" style={style} aria-hidden={title ? undefined : true} role={title ? "img" : undefined}>
      {title ? <title>{title}</title> : null}
      <path d={d} />
    </svg>
  );
}

export function hasIcon(name: string): boolean {
  return name in PATHS || name in MATERIAL;
}
