// The command palette's model, pure: the rows it lists (grouped Suggestions, Songs, Albums,
// Artists, Playlists, Commands), the recent picks behind Suggestions, and the actions menu for the
// selected row. The menu is built from the context menu's own layout (lib/menu-model buildMenu over
// the registry's contextMenu descriptors), so Play next, Play later, Add to playlist, Love and the
// rest are the same actions run the same way; only the shape differs (a flat list with a shortcut
// on each row). The component (components/CommandPalette) fetches, draws and runs.
import type { ActionDescriptor, ActionTarget, TrackRef } from "@core/api";
import { t } from "@shared/strings";
import { rank, type Rankable, type Ranked } from "../store/palette-rank";
import { parseChord, chordsEqual, chordToString, foldPrimary, type Chord } from "../store/shortcuts";
import type { MenuRow } from "./menu-model";

export type EntryKind = Rankable["kind"];

/** One row the palette can list: a library item or a command. */
export interface PaletteEntry extends Rankable {
  coverArt?: string;
  icon?: string;
  /** A command's chord, from the keymap. */
  shortcut?: string;
  /** A song's reference on its account: what its actions run on. */
  ref?: TrackRef;
  /** A lyric hit: the line, the matched phrase in it, and where it is sung (synced lyrics). */
  lyric?: { line: string; start: number; end: number; startMs?: number };
}

export type GroupId = "suggestions" | "track" | "lyrics" | "album" | "artist" | "playlist" | "action";

export interface PaletteGroup {
  id: GroupId;
  rows: Ranked<PaletteEntry>[];
}

export interface PaletteSources {
  recents: readonly PaletteEntry[];
  tracks: readonly PaletteEntry[];
  /** Lyric hits, already matched by the core (which ignores case, punctuation and diacritics). */
  lyrics?: readonly PaletteEntry[];
  albums: readonly PaletteEntry[];
  artists: readonly PaletteEntry[];
  playlists: readonly PaletteEntry[];
  commands: readonly PaletteEntry[];
}

/** How many rows each group shows while searching. */
export const GROUP_LIMITS: Record<Exclude<GroupId, "suggestions">, number> = { track: 6, lyrics: 4, album: 4, artist: 4, playlist: 4, action: 6 };
export const MAX_RECENTS = 6;

/**
 * The palette's groups, in their fixed order, empty ones left out. With no query: the recent picks
 * as Suggestions, then every command. With one: each kind ranked on its own.
 */
export function paletteGroups(query: string, src: PaletteSources): PaletteGroup[] {
  const q = query.trim();
  const groups: PaletteGroup[] = [];
  const add = (id: GroupId, rows: Ranked<PaletteEntry>[]) => {
    if (rows.length) groups.push({ id, rows });
  };
  if (!q) {
    const recents = src.recents.filter((r) => r.kind !== "action" || src.commands.some((c) => c.id === r.id));
    const shown = new Set(recents.map((r) => r.id));
    add("suggestions", recents.map((r) => ({ item: src.commands.find((c) => c.id === r.id) ?? r, score: 1, matches: [] })));
    add("action", src.commands.filter((c) => !shown.has(c.id)).map((c) => ({ item: c, score: 1, matches: [] })));
    return groups;
  }
  const songs = rank(q, [...src.tracks], GROUP_LIMITS.track);
  add("track", songs);
  // Not ranked here (the palette's ranker can't see the match the core found); a song already
  // listed under Songs isn't listed again.
  const listed = new Set(songs.map((r) => rawId(r.item)));
  const lyrics: Ranked<PaletteEntry>[] = [];
  for (const hit of src.lyrics ?? []) {
    const id = rawId(hit);
    if (listed.has(id) || lyrics.length >= GROUP_LIMITS.lyrics) continue;
    listed.add(id);
    lyrics.push({ item: hit, score: 0, matches: [] });
  }
  add("lyrics", lyrics);
  add("album", rank(q, [...src.albums], GROUP_LIMITS.album));
  add("artist", rank(q, [...src.artists], GROUP_LIMITS.artist));
  add("playlist", rank(q, [...src.playlists], GROUP_LIMITS.playlist));
  add("action", rank(q, [...src.commands], GROUP_LIMITS.action));
  return groups;
}

/** The groups' rows in order: what ↑↓ move through. */
export function flatRows(groups: readonly PaletteGroup[]): Ranked<PaletteEntry>[] {
  return groups.flatMap((g) => g.rows);
}

export function groupLabel(id: GroupId): string {
  switch (id) {
    case "suggestions": return t("palette.suggestions");
    case "track": return t("palette.songs");
    case "lyrics": return t("palette.lyrics");
    case "album": return t("palette.albums");
    case "artist": return t("palette.artists");
    case "playlist": return t("palette.playlists");
    case "action": return t("palette.commands");
  }
}

/** The type label at the right of a row. */
export function typeLabel(kind: EntryKind): string {
  switch (kind) {
    case "track": return t("palette.type.song");
    case "album": return t("palette.type.album");
    case "artist": return t("palette.type.artist");
    case "playlist": return t("palette.type.playlist");
    case "action": return t("palette.type.command");
  }
}

/** A pick goes to the front of the recents, once. */
export function pushRecent(recents: readonly PaletteEntry[], entry: PaletteEntry, max = MAX_RECENTS): PaletteEntry[] {
  // A lyric hit is remembered as its song.
  const id = entry.lyric ? entryId(entry.kind, rawId(entry)) : entry.id;
  const sub = entry.lyric ? undefined : entry.sub;
  const keep: PaletteEntry = { id, label: entry.label, kind: entry.kind, sub, coverArt: entry.coverArt, icon: entry.icon, ref: entry.ref };
  return [keep, ...recents.filter((r) => r.id !== id)].slice(0, max);
}

/** Recents read back from storage: anything malformed is dropped. */
export function sanitizeRecents(raw: unknown): PaletteEntry[] {
  if (!Array.isArray(raw)) return [];
  const kinds = new Set<EntryKind>(["track", "album", "artist", "playlist", "action"]);
  return raw
    .filter((r): r is PaletteEntry => !!r && typeof r === "object" && typeof r.id === "string" && typeof r.label === "string" && kinds.has(r.kind))
    // A song needs its reference to be played or acted on.
    .filter((r) => r.kind !== "track" || (!!r.ref && typeof r.ref.id === "string" && typeof r.ref.source === "string"))
    .slice(0, MAX_RECENTS);
}

/** Entry ids are `<prefix>:<library id>` (`ly:` for a lyric hit); commands are `act:<action id>`. */
const PREFIX: Record<EntryKind, string> = { track: "t", album: "al", artist: "ar", playlist: "pl", action: "act" };
export function entryId(kind: EntryKind, id: string): string {
  return `${PREFIX[kind]}:${id}`;
}
export function rawId(entry: Pick<PaletteEntry, "id">): string {
  return entry.id.slice(entry.id.indexOf(":") + 1);
}

/** What the registry's actions run on for a library row. */
export function targetOf(entry: PaletteEntry): ActionTarget | undefined {
  const ids = [rawId(entry)];
  switch (entry.kind) {
    case "track": return entry.ref ? { type: "tracks", data: { tracks: [entry.ref] } } : undefined;
    case "album": return { type: "albums", data: { ids } };
    case "artist": return { type: "artists", data: { ids } };
    case "playlist": return { type: "playlists", data: { ids } };
    default: return undefined;
  }
}

// ---- The actions menu -----------------------------------------------------------------------------

export type PaletteRun =
  /** Open the item's own page. */
  | { type: "open" }
  /** Run the command the row is. */
  | { type: "command"; id: string }
  /** A registry action on the row's target, through executeAction, as the context menu runs it. */
  | { type: "action"; id: string };

export interface PaletteAction {
  key: string;
  label: string;
  icon?: string;
  /** Canonical chord ("Mod+Enter"); Enter is the primary action. */
  shortcut?: string;
  destructive?: boolean;
  run: PaletteRun;
}

/**
 * The palette's own chords for the menu's actions, Raycast-style: Enter runs the primary one and
 * the rest take a modifier, so they work while the search field has focus. Love keeps the keymap's
 * own Mod+L.
 */
export const PALETTE_SHORTCUTS: Record<string, string> = {
  playShuffled: "Alt+Enter",
  playNext: "Mod+Enter",
  // The core's Add to queue (the Queue section setting) takes the chord ahead of Play later.
  addToQueue: "Mod+Shift+Enter",
  playLater: "Mod+Shift+Enter",
  addToPlaylist: "Mod+P",
  love: "Mod+L",
  unlove: "Mod+L",
  download: "Mod+D",
  unpin: "Mod+D",
  pinToLibrary: "Mod+Shift+P",
  unpinFromLibrary: "Mod+Shift+P",
  goToAlbum: "Mod+Shift+A",
  goToArtist: "Mod+Shift+G",
  "ui.info": "Mod+I",
};

/** Context-menu labels the palette words for itself (its rows are actions, not checkboxes). */
const LABELS: Record<string, Parameters<typeof t>[0]> = {
  play: "menu.play",
  playShuffled: "menu.shuffle",
  playNext: "menu.playNext",
  playLater: "palette.addToQueue",
  addToPlaylist: "palette.addToPlaylist",
};

const OPEN = { album: "palette.open.album", artist: "palette.open.artist", playlist: "palette.open.playlist" } as const;

const ICONS: Record<string, string> = { addToQueue: "queue", play: "play", playShuffled: "shuffle", playNext: "playNext", playLater: "playLater", addToPlaylist: "playlistAdd", love: "heart", unlove: "heartOff", download: "download", unpin: "downloadOff", "ui.info": "info", goToAlbum: "album", goToArtist: "artist" };

/** Registry descriptors the palette leaves out, as the context menu does (the inapplicable library pin). */
export function menuActions(registry: readonly ActionDescriptor[]): ActionDescriptor[] {
  return registry.filter((a) => !((a.id === "pinToLibrary" || a.id === "unpinFromLibrary") && !a.enabled));
}

/**
 * The actions menu for a row, in groups (drawn with separators between). For a library row, from
 * the context menu's rows: Open (albums, artists, playlists) and the play strip first, then Add to
 * playlist and the toggles, then the rest in the menu's order, the destructive last. The rating row
 * is left out (its digits are typing here), and so are the menu's nested Share and Snooze menus and
 * Open in Genius. A command has one action, Run.
 */
export function paletteActions(entry: PaletteEntry, rows: readonly MenuRow[], registry: readonly ActionDescriptor[], platform: "macOs" | "linux" | "windows" = "linux"): PaletteAction[][] {
  if (entry.kind === "action") return [[{ key: "run", label: t("palette.runCommand"), icon: entry.icon, shortcut: "Enter", run: { type: "command", id: rawId(entry) } }]];
  const byId = new Map(registry.map((a) => [a.id, a]));
  const groups: PaletteAction[][] = [];
  let cur: PaletteAction[] = [];
  const cut = () => {
    if (cur.length) groups.push(cur);
    cur = [];
  };
  const collection = entry.kind !== "track";
  if (collection) cur.push({ key: "open", label: t(OPEN[entry.kind as keyof typeof OPEN]), icon: "chevronRight", shortcut: "Enter", run: { type: "open" } });
  const push = (id: string, label: string, enabled: boolean, destructive = false) => {
    if (!enabled) return;
    const shortcut = id === "play" ? (collection ? "Shift+Enter" : "Enter") : (PALETTE_SHORTCUTS[id] ?? modifierChord(byId.get(id)?.defaultShortcut, platform));
    cur.push({ key: id, label, icon: ICONS[id] ?? byId.get(id)?.icon, shortcut, destructive: destructive || undefined, run: { type: "action", id } });
  };
  for (const r of rows) {
    switch (r.kind) {
      case "strip": {
        // Play later reads "Add to queue" here, unless the core offers its own Add to queue.
        const ownQueue = r.cells.some((c) => c.id === "addToQueue");
        for (const c of r.cells) push(c.id, LABELS[c.id] && !(c.id === "playLater" && ownQueue) ? t(LABELS[c.id] as Parameters<typeof t>[0]) : c.label, c.enabled);
        cut();
        break;
      }
      case "check":
        // A checkbox in the menu; here the row names what it does: the registry's own label for the half that runs.
        push(r.runId, byId.get(r.runId)?.label || r.label, r.enabled, r.runId === "unpin");
        break;
      case "submenu":
        // Add to playlist opens the playlist picker; Share and Snooze are the context menu's own
        // nested menus, so they stay there.
        if (r.id === "addToPlaylist") push(r.id, t("palette.addToPlaylist"), r.enabled);
        break;
      case "action":
        push(r.action.id, LABELS[r.action.id] ? t(LABELS[r.action.id] as Parameters<typeof t>[0]) : r.label, r.action.enabled, r.destructive);
        break;
      case "sep":
        cut();
        break;
      default:
        break;
    }
  }
  cut();
  return dedupeShortcuts(groups);
}

/**
 * A descriptor's default chord, only when it has a modifier (a bare key is typing in the palette),
 * in "Mod+…" form: the core spells it "Cmd+…" or "Ctrl+…", and the pressed key arrives as `mod`.
 */
function modifierChord(s: string | undefined, platform: "macOs" | "linux" | "windows"): string | undefined {
  const parsed = s ? parseChord(s) : undefined;
  const c = parsed && foldPrimary(parsed, platform);
  return c && (c.mod || c.ctrl || c.alt || c.meta) ? chordToString(c) : undefined;
}

/** One action per chord: a later row that repeats an earlier row's chord shows none. */
function dedupeShortcuts(groups: PaletteAction[][]): PaletteAction[][] {
  const seen: Chord[] = [];
  for (const g of groups) for (const a of g) {
    const c = a.shortcut ? parseChord(a.shortcut) : undefined;
    if (!c) continue;
    if (seen.some((s) => chordsEqual(s, c))) a.shortcut = undefined;
    else seen.push(c);
  }
  return groups;
}

/** The action a chord runs, if any row carries it. */
export function actionForChord(groups: readonly PaletteAction[][], chord: Chord): PaletteAction | undefined {
  for (const g of groups) for (const a of g) {
    const c = a.shortcut ? parseChord(a.shortcut) : undefined;
    if (c && chordsEqual(c, chord)) return a;
  }
  return undefined;
}

/** Whether a chord could be one of the menu's (decided before the row's actions have loaded). */
export function isPaletteChord(chord: Chord): boolean {
  if (!(chord.mod || chord.ctrl || chord.alt || chord.meta || (chord.shift && chord.key === "Enter"))) return false;
  return [...Object.values(PALETTE_SHORTCUTS), "Shift+Enter"].some((s) => {
    const c = parseChord(s);
    return !!c && chordsEqual(c, chord);
  });
}

/** Actions whose label matches the menu's filter, groups kept (empty ones dropped). */
export function filterActions(groups: readonly PaletteAction[][], query: string): PaletteAction[][] {
  const q = query.trim().toLowerCase();
  if (!q) return groups.map((g) => [...g]);
  return groups.map((g) => g.filter((a) => a.label.toLowerCase().includes(q))).filter((g) => g.length);
}

const MAC_MODS = { ctrl: "⌃", alt: "⌥", shift: "⇧", mod: "⌘", meta: "⌘" } as const;
const KEY_GLYPHS: Record<string, string> = { Enter: "↵", ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→", Escape: "Esc", Backspace: "⌫", Delete: "Del", Space: "Space" };

/**
 * A chord as keycaps, one per key: ⌘ ⇧ ↵ on macOS (in Apple's ⌃⌥⇧⌘ order), Ctrl Shift ↵ elsewhere.
 */
export function keycaps(shortcut: string, platform: "macOs" | "linux" | "windows"): string[] {
  const c = parseChord(shortcut);
  if (!c) return [];
  const key = KEY_GLYPHS[c.key] ?? c.key;
  if (platform === "macOs") {
    const out: string[] = [];
    if (c.ctrl) out.push(MAC_MODS.ctrl);
    if (c.alt) out.push(MAC_MODS.alt);
    if (c.shift) out.push(MAC_MODS.shift);
    if (c.mod || c.meta) out.push(MAC_MODS.mod);
    return [...out, key];
  }
  const out: string[] = [];
  if (c.mod || c.ctrl) out.push("Ctrl");
  if (c.alt) out.push("Alt");
  if (c.shift) out.push("Shift");
  if (c.meta) out.push(platform === "windows" ? "Win" : "Super");
  return [...out, key];
}

/** The chord in aria-keyshortcuts form ("Control+Shift+Enter", "Meta+Enter"). */
export function ariaShortcut(shortcut: string | undefined, platform: "macOs" | "linux" | "windows"): string | undefined {
  const c = shortcut ? parseChord(shortcut) : undefined;
  if (!c) return undefined;
  const parts: string[] = [];
  if (c.mod) parts.push(platform === "macOs" ? "Meta" : "Control");
  if (c.ctrl) parts.push("Control");
  if (c.alt) parts.push("Alt");
  if (c.shift) parts.push("Shift");
  if (c.meta) parts.push("Meta");
  return [...parts, c.key].join("+");
}
