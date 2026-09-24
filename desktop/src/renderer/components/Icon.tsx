// Icons come from lucide-react (ISC): one name → component map, keyed by the
// short names the renderer uses, plus the Material Symbols names the action
// registry emits in `ActionDescriptor.icon`. Nothing in the UI draws an icon
// from a unicode glyph; anything unknown renders as a music note so a new
// registry icon is visible (and `icon.test.ts` fails) rather than blank.
import type { CSSProperties } from "react";
import {
  ArrowDown, ArrowLeft, ArrowRight, ArrowUp, BatteryMedium, Bug, ChartColumn, Check, ChevronDown, ChevronLeft, ChevronRight, ChevronUp,
  Cloud, CloudOff, Command, Copy, Disc3, Download, Ellipsis, ExternalLink, FastForward, Filter, Folder, GripVertical, HardDrive, Heart, HeartOff,
  History, House, Infinity as InfinityIcon, Info, LayoutGrid, Library, ListEnd, ListFilterPlus, ListMusic, ListPlus, ListStart, LoaderCircle,
  Maximize2, MicVocal, Minus, MonitorSmartphone, Moon, Music, Network, Pause, Pencil, PictureInPicture2, Pin, PinOff, Play, Plus,
  Redo2, Repeat, Repeat1, Rewind, RotateCcw, Search, Settings, Shuffle, SkipBack, SkipForward, Square, SquareCheck, Star, StarOff,
  Trash2, TriangleAlert, Undo2, User, Volume2, VolumeX, WifiOff, X,
  type LucideIcon,
} from "lucide-react";

/** Renderer icon names → lucide components. */
const ICONS: Record<string, LucideIcon> = {
  play: Play, pause: Pause, stop: Square, next: SkipForward, previous: SkipBack, rewind: Rewind, forward: FastForward,
  shuffle: Shuffle, repeat: Repeat, repeatOne: Repeat1, autoplay: InfinityIcon, volume: Volume2, mute: VolumeX,
  heart: Heart, heartOff: HeartOff, star: Star, starOff: StarOff,
  queue: ListMusic, lyrics: MicVocal, fullscreen: Maximize2, mini: PictureInPicture2, devices: MonitorSmartphone,
  search: Search, command: Command, settings: Settings,
  home: House, library: Library, album: Disc3, artist: User, playlist: ListMusic, playlistAdd: ListPlus, song: Music, genre: LayoutGrid,
  download: Download, downloadOff: CloudOff, cached: HardDrive, filter: Filter, filterAdd: ListFilterPlus, stats: ChartColumn, info: Info,
  remove: Minus, trash: Trash2, edit: Pencil, close: X, check: Check,
  chevronDown: ChevronDown, chevronRight: ChevronRight, chevronLeft: ChevronLeft, chevronUp: ChevronUp,
  arrowDown: ArrowDown, arrowUp: ArrowUp, back: ArrowLeft, forwardArrow: ArrowRight, undo: Undo2, redo: Redo2,
  playNext: ListStart, playLater: ListEnd, pin: Pin, pinOff: PinOff, restore: History, seek: ArrowRight, sleep: Moon, bug: Bug,
  selectAll: SquareCheck, grip: GripVertical, more: Ellipsis, minimize: Minus, maximize: Square, restoreWin: Copy,
  spinner: LoaderCircle, offline: WifiOff, warn: TriangleAlert, battery: BatteryMedium, external: ExternalLink, folder: Folder,
  copy: Copy, resume: RotateCcw, music: Music, cloud: Cloud, lan: Network, plus: Plus,
};

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
}

/** Resolves a renderer or Material name to the renderer name that has a glyph. */
export function iconName(name: string): string {
  return ICONS[name] ? name : (MATERIAL[name] ?? "music");
}

export function Icon({ name, size = 16, className, style, title }: IconProps) {
  const C = ICONS[iconName(name)] ?? Music;
  return (
    <C className={`icon ${className ?? ""}`} size={size} strokeWidth={1.75} style={style} aria-hidden={title ? undefined : true} role={title ? "img" : undefined} aria-label={title}>
      {title ? <title>{title}</title> : null}
    </C>
  );
}

export function hasIcon(name: string): boolean {
  return name in ICONS || name in MATERIAL;
}

/** Every name the renderer can draw (for tests). */
export const ICON_NAMES: readonly string[] = Object.keys(ICONS);
