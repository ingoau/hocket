// Generates android/app/src/main/java/app/hocket/ui/icons/HocketIcons.kt: the
// Material Symbols (rounded) glyphs the Android app uses, as Compose
// ImageVectors, from the same Iconify dataset desktop's icons come from
// (@iconify-json/material-symbols, Apache-2.0; a desktop dev dependency, so run
// `pnpm install` in desktop/ first). Only the icons listed here are emitted, so
// the app doesn't ship the whole set.
//
//   node scripts/gen-android-icons.mjs
//
// The object mirrors androidx's `Icons` layout (HocketIcons.Filled.PlayArrow,
// HocketIcons.Outlined.Home, HocketIcons.AutoMirrored.Filled.ArrowBack), keyed
// by the Compose icon names the app used before, so call sites read the same.
// Filled picks the symbol's filled rounded variant, Outlined its outlined one
// (the same glyph when the symbol has only one). Re-run after changing the
// lists; the output is committed.
import { writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const require = createRequire(new URL("../desktop/package.json", import.meta.url));
const set = require("@iconify-json/material-symbols/icons.json");

/**
 * Compose name → Material Symbols name. A ":outline" suffix forces the outlined
 * variant where the Compose name is itself an outlined glyph (FavoriteBorder).
 */
const SYMBOLS = {
  AccountCircle: "account_circle", Add: "add", Album: "album", AllInclusive: "all_inclusive", ArrowBack: "arrow_back",
  ArrowDropDown: "arrow_drop_down", BarChart: "bar_chart", BatterySaver: "battery_saver", Bedtime: "bedtime", Cancel: "cancel",
  Cast: "cast", Category: "category", Check: "check", CheckCircle: "check_circle", ChevronRight: "chevron_right",
  ClearAll: "clear_all", Inventory2: "inventory_2", Close: "close", CloseFullscreen: "close_fullscreen", OpenInFull: "open_in_full", Menu: "menu", MenuOpen: "menu_open", CloudOff: "cloud_off", Computer: "computer", Delete: "delete", Devices: "devices", Dns: "dns",
  Download: "download", DownloadDone: "download_done", DownloadForOffline: "download_for_offline", DragHandle: "drag_handle", Edit: "edit",
  ErrorOutline: "error:outline", Favorite: "favorite", FavoriteBorder: "favorite:outline", FilterAlt: "filter_alt",
  GraphicEq: "graphic_eq", History: "history", Home: "home", Info: "info", KeyboardArrowDown: "keyboard_arrow_down",
  LibraryMusic: "library_music", Logout: "logout", Lyrics: "lyrics", MoreVert: "more_vert", MusicNote: "music_note",
  NetworkCheck: "network_check", OfflineBolt: "offline_bolt", Palette: "palette", Pause: "pause", Person: "person",
  PhoneAndroid: "phone_android", PlayArrow: "play_arrow", PlayCircle: "play_circle", PlaylistAdd: "playlist_add", PlaylistAddCheck: "playlist_add_check",
  PlaylistPlay: "playlist_play", PlaylistRemove: "playlist_remove", PushPin: "push_pin", QueueMusic: "queue_music",
  Redo: "redo", Refresh: "refresh", Remove: "remove", RemoveCircleOutline: "do_not_disturb_on:outline", Repeat: "repeat",
  RepeatOne: "repeat_one", RestartAlt: "restart_alt", Save: "save", Search: "search", SelectAll: "select_all",
  Settings: "settings", Share: "share", SettingsBackupRestore: "settings_backup_restore", Shuffle: "shuffle", SkipNext: "skip_next",
  SkipPrevious: "skip_previous", Sort: "sort", Speaker: "speaker", Star: "star", StarOutline: "star:outline", Stop: "stop",
  Sync: "sync", Tune: "tune", Undo: "undo", Visibility: "visibility", VisibilityOff: "visibility_off", VolumeOff: "volume_off", VolumeUp: "volume_up",
  Watch: "watch",
  ArrowDownward: "arrow_downward", ArrowUpward: "arrow_upward", Bolt: "bolt", CalendarToday: "calendar_today", Schedule: "schedule",
  SortByAlpha: "sort_by_alpha", Speed: "speed", Timer: "timer", TrendingUp: "trending_up", Update: "update",
  BugReport: "bug_report", Code: "code", ContentCopy: "content_copy",
  Bluetooth: "bluetooth", Headphones: "headphones", Usb: "usb",
  Earbuds: "earbuds", DirectionsCar: "directions_car", Hearing: "hearing", Tv: "tv",
  OpenInNew: "open_in_new", Laptop: "laptop_windows", DesktopWindows: "desktop_windows", Warning: "warning", Equalizer: "equalizer",
};

/** Which glyphs each group emits. AutoMirrored ones flip in right-to-left layouts. */
const GROUPS = [
  { path: ["Filled"], fill: true, mirror: false, names: [
    "AccountCircle", "Add", "Album", "AllInclusive", "ArrowDropDown", "BarChart", "BatterySaver", "Bedtime", "Cancel", "Cast",
    "Category", "Check", "CheckCircle", "ChevronRight", "ClearAll", "Close", "Computer", "Delete", "Devices", "Download",
    "DownloadDone", "DownloadForOffline", "DragHandle", "Edit", "ErrorOutline", "Favorite", "FavoriteBorder", "FilterAlt", "GraphicEq",
    "History", "Home", "Info", "KeyboardArrowDown", "LibraryMusic", "Lyrics", "MoreVert", "MusicNote", "NetworkCheck", "Palette",
    "Pause", "Person", "PhoneAndroid", "PlayArrow", "PlayCircle", "PlaylistAdd", "PlaylistAddCheck", "PlaylistPlay", "PlaylistRemove",
    "PushPin", "QueueMusic", "Refresh", "Remove", "RemoveCircleOutline", "Repeat", "RepeatOne", "RestartAlt", "Save", "Search",
    "SelectAll", "Settings", "SettingsBackupRestore", "Share", "Shuffle", "SkipNext", "SkipPrevious", "Sort", "Speaker", "Star",
    "StarOutline", "Stop", "Sync", "Tune", "Visibility", "VisibilityOff", "VolumeOff", "VolumeUp", "Watch",
    "ArrowDownward", "ArrowUpward", "Bolt", "CalendarToday", "Schedule", "SortByAlpha", "Speed", "Timer", "TrendingUp", "Update",
    "BugReport", "Code", "ContentCopy", "Dns", "Bluetooth", "Headphones", "Usb",
    "OpenInNew", "Laptop", "DesktopWindows", "Warning", "Equalizer",
    // The song stash (#150).
    "Inventory2",
    // Large screens: the player panel's full-screen toggle and the navigation rail's menu.
    "OpenInFull", "CloseFullscreen", "Menu", "MenuOpen",
    // What this phone plays out of: the connect button and the Connect sheet.
    "Earbuds", "DirectionsCar", "Hearing", "Tv",
  ] },
  { path: ["Outlined"], fill: false, mirror: false, names: [
    "Album", "BarChart", "Category", "CloudOff", "Download", "FilterAlt", "History", "Home", "Info", "LibraryMusic", "MusicNote",
    "OfflineBolt", "Person", "PushPin", "Search", "StarOutline",
    "Delete", "Edit", "Settings", "Palette", "Bolt", "Warning", "Laptop", "DesktopWindows", "PhoneAndroid", "Headphones",
    "ContentCopy", "Speaker", "Lyrics", "Dns", "Devices", "BatterySaver", "Tune", "Sync",
    "Inventory2",
    "Earbuds", "DirectionsCar", "Hearing", "Tv",
  ] },
  { path: ["AutoMirrored", "Filled"], fill: true, mirror: true, names: [
    "ArrowBack", "Logout", "PlaylistAdd", "PlaylistPlay", "QueueMusic", "Redo", "Undo",
  ] },
  { path: ["AutoMirrored", "Outlined"], fill: false, mirror: true, names: ["QueueMusic"] },
];

function body(name) {
  let n = name;
  for (let i = 0; i < 8; i++) {
    const icon = set.icons[n];
    if (icon) return icon.body;
    const alias = set.aliases?.[n];
    if (!alias) return undefined;
    n = alias.parent;
  }
  return undefined;
}

const first = (...names) => {
  for (const n of names) {
    const b = body(n);
    if (b) return b;
  }
  return undefined;
};

/** Splits minified SVG path data into [command, args] segments, one per command instance. */
function tokenize(d) {
  const ARITY = { m: 2, l: 2, h: 1, v: 1, c: 6, s: 4, q: 4, t: 2, a: 7, z: 0 };
  const segs = [];
  let cmd = "";
  let args = [];
  let i = 0;
  const num = /^[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/;
  const flush = () => {
    const n = ARITY[cmd.toLowerCase()];
    if (n === 0) { segs.push([cmd, []]); return; }
    if (!args.length || args.length % n) throw new Error(`bad argument count for ${cmd} in ${d}`);
    for (let k = 0; k < args.length; k += n) {
      // Extra pairs after a moveto are implicit linetos.
      const c = k > 0 && /[Mm]/.test(cmd) ? (cmd === "M" ? "L" : "l") : cmd;
      segs.push([c, args.slice(k, k + n)]);
    }
  };
  while (i < d.length) {
    const c = d[i];
    if (/[\s,]/.test(c)) { i++; continue; }
    if (/[MmLlHhVvCcSsQqTtAaZz]/.test(c)) {
      if (cmd) flush();
      cmd = c; args = []; i++;
      continue;
    }
    // Arc flags are single digits that may run into the next number.
    const flag = /[Aa]/.test(cmd) && (args.length % 7 === 3 || args.length % 7 === 4);
    const m = flag ? d.slice(i).match(/^[01]/) : d.slice(i).match(num);
    if (!m) throw new Error(`unparseable path data at ${i}: ${d.slice(i, i + 20)}`);
    args.push(Number(m[0]));
    i += m[0].length;
  }
  if (cmd) flush();
  return segs;
}

/**
 * Re-spells SVG path data with absolute commands only (M L C Q A Z; H/V become L,
 * S/T become C/Q with their reflected control points), separated by spaces.
 *
 * Compose hands relative commands straight to android.graphics.Path's rMoveTo,
 * rLineTo… and those resolve against Skia's last point. After a close, older
 * Android (seen on a Galaxy S10 with Android 12) takes that as the contour's last
 * drawn point, not its start, so every "z m…" subpath lands offset: holes go
 * missing and second halves of glyphs fall off the 24dp canvas. Absolute
 * coordinates don't depend on that, so icons draw the same on every version.
 */
function normalize(d) {
  const out = [];
  const r = (n) => String(Math.round(n * 1000) / 1000 + 0); // + 0 turns -0 into 0
  let x = 0, y = 0, sx = 0, sy = 0;
  let prev = ""; // previous command, upper-cased
  let cx = 0, cy = 0; // its last control point, for S/T reflection
  for (const [cmd, a] of tokenize(d)) {
    const rel = cmd === cmd.toLowerCase();
    const ox = rel ? x : 0, oy = rel ? y : 0;
    switch (cmd.toUpperCase()) {
      case "M":
        x = a[0] + ox; y = a[1] + oy; sx = x; sy = y;
        out.push("M", r(x), r(y)); prev = "M"; break;
      case "L":
        x = a[0] + ox; y = a[1] + oy;
        out.push("L", r(x), r(y)); prev = "L"; break;
      case "H":
        x = a[0] + ox;
        out.push("L", r(x), r(y)); prev = "L"; break;
      case "V":
        y = a[0] + oy;
        out.push("L", r(x), r(y)); prev = "L"; break;
      case "C": {
        const p = [a[0] + ox, a[1] + oy, a[2] + ox, a[3] + oy, a[4] + ox, a[5] + oy];
        out.push("C", ...p.map(r));
        [cx, cy, x, y] = [p[2], p[3], p[4], p[5]]; prev = "C"; break;
      }
      case "S": {
        const [x1, y1] = prev === "C" ? [2 * x - cx, 2 * y - cy] : [x, y];
        const p = [a[0] + ox, a[1] + oy, a[2] + ox, a[3] + oy];
        out.push("C", r(x1), r(y1), ...p.map(r));
        [cx, cy, x, y] = p; prev = "C"; break;
      }
      case "Q": {
        const p = [a[0] + ox, a[1] + oy, a[2] + ox, a[3] + oy];
        out.push("Q", ...p.map(r));
        [cx, cy, x, y] = p; prev = "Q"; break;
      }
      case "T": {
        const [x1, y1] = prev === "Q" ? [2 * x - cx, 2 * y - cy] : [x, y];
        const nx = a[0] + ox, ny = a[1] + oy;
        out.push("Q", r(x1), r(y1), r(nx), r(ny));
        [cx, cy, x, y] = [x1, y1, nx, ny]; prev = "Q"; break;
      }
      case "A":
        x = a[5] + ox; y = a[6] + oy;
        out.push("A", r(a[0]), r(a[1]), r(a[2]), String(a[3]), String(a[4]), r(x), r(y)); prev = "A"; break;
      case "Z":
        x = sx; y = sy;
        out.push("Z"); prev = "Z"; break;
    }
  }
  return out.join(" ");
}

function paths(svg) {
  const ds = [...svg.matchAll(/<path\b([^>]*)\/?>/g)].map(([, attrs]) => {
    const fill = attrs.match(/\bfill="([^"]*)"/)?.[1];
    if (fill && fill !== "currentColor") throw new Error(`unexpected fill ${fill}`);
    const d = attrs.match(/\bd="([^"]*)"/)?.[1];
    if (!d) throw new Error(`path without d: ${attrs}`);
    return normalize(d);
  });
  if (!ds.length || svg.replace(/<path\b[^>]*\/?>(<\/path>)?/g, "").trim()) throw new Error(`unexpected SVG body: ${svg.slice(0, 80)}`);
  return ds;
}

function glyph(compose, fill) {
  const spec = SYMBOLS[compose];
  if (!spec) throw new Error(`no Material Symbol mapped for ${compose}`);
  const [symbol, variant] = spec.split(":");
  const m = symbol.replace(/_/g, "-");
  const outline = variant === "outline" || !fill;
  const svg = outline ? first(`${m}-outline-rounded`, `${m}-rounded`, `${m}-outline`, m) : first(`${m}-rounded`, m);
  if (!svg) throw new Error(`missing Material Symbol ${symbol} (${compose})`);
  return paths(svg);
}

const ind = (n) => "    ".repeat(n);
const all = [];
const tree = {};
for (const g of GROUPS) {
  let node = tree;
  for (const p of g.path) node = node[p] ??= {};
  for (const name of g.names) {
    const ref = [...g.path, name].join(".");
    const args = glyph(name, g.fill).map((d) => `"${d}"`).join(", ");
    (node[""] ??= []).push(`val ${name}: ImageVector by lazy { symbol("${ref}", ${args}${g.mirror ? ", autoMirror = true" : ""}) }`);
    all.push(ref);
  }
}

function emit(node, depth) {
  const lines = [];
  for (const [key, child] of Object.entries(node)) {
    if (key === "") lines.push(...child.map((l) => ind(depth) + l));
    else lines.push(`${ind(depth)}object ${key} {`, ...emit(child, depth + 1), `${ind(depth)}}`);
  }
  return lines;
}

const target = fileURLToPath(new URL("../android/app/src/main/java/app/hocket/ui/icons/HocketIcons.kt", import.meta.url));
writeFileSync(target, `// Generated by scripts/gen-android-icons.mjs from @iconify-json/material-symbols
// (Material Symbols, Apache-2.0, rounded). Do not edit; edit the script's lists.
package app.hocket.ui.icons

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp

/** The app's icons: Material Symbols, rounded, as 24dp vectors (tinted by \`Icon\` like any other). */
object HocketIcons {
${emit(tree, 1).join("\n")}

    /** Every icon, so a test can build each one. */
    internal val all: List<ImageVector>
        get() = listOf(
${all.map((r) => `${ind(3)}${r},`).join("\n")}
        )
}

private fun symbol(name: String, vararg paths: String, autoMirror: Boolean = false): ImageVector =
    ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f, autoMirror = autoMirror)
        .apply { for (d in paths) addPath(addPathNodes(d), fill = SolidColor(Color.Black)) }
        .build()
`);
console.log(`[gen-android-icons] ${all.length} icons → ${target}`);
