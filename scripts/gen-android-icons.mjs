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
  ClearAll: "clear_all", Close: "close", CloudOff: "cloud_off", Computer: "computer", Delete: "delete", Devices: "devices",
  Download: "download", DownloadDone: "download_done", DownloadForOffline: "download_for_offline", DragHandle: "drag_handle",
  ErrorOutline: "error:outline", Favorite: "favorite", FavoriteBorder: "favorite:outline", FilterAlt: "filter_alt",
  GraphicEq: "graphic_eq", History: "history", Home: "home", Info: "info", KeyboardArrowDown: "keyboard_arrow_down",
  LibraryMusic: "library_music", Logout: "logout", Lyrics: "lyrics", MoreVert: "more_vert", MusicNote: "music_note",
  NetworkCheck: "network_check", OfflineBolt: "offline_bolt", Palette: "palette", Pause: "pause", Person: "person",
  PhoneAndroid: "phone_android", PlayArrow: "play_arrow", PlaylistAdd: "playlist_add", PlaylistAddCheck: "playlist_add_check",
  PlaylistPlay: "playlist_play", PlaylistRemove: "playlist_remove", PushPin: "push_pin", QueueMusic: "queue_music",
  Redo: "redo", Refresh: "refresh", Remove: "remove", RemoveCircleOutline: "do_not_disturb_on:outline", Repeat: "repeat",
  RepeatOne: "repeat_one", RestartAlt: "restart_alt", Save: "save", Search: "search", SelectAll: "select_all",
  Settings: "settings", SettingsBackupRestore: "settings_backup_restore", Shuffle: "shuffle", SkipNext: "skip_next",
  SkipPrevious: "skip_previous", Sort: "sort", Speaker: "speaker", Star: "star", StarOutline: "star:outline", Stop: "stop",
  Sync: "sync", Tune: "tune", Undo: "undo", Visibility: "visibility", VisibilityOff: "visibility_off", VolumeUp: "volume_up",
};

/** Which glyphs each group emits. AutoMirrored ones flip in right-to-left layouts. */
const GROUPS = [
  { path: ["Filled"], fill: true, mirror: false, names: [
    "AccountCircle", "Add", "Album", "AllInclusive", "ArrowDropDown", "BarChart", "BatterySaver", "Bedtime", "Cancel", "Cast",
    "Category", "Check", "CheckCircle", "ChevronRight", "ClearAll", "Close", "Computer", "Delete", "Devices", "Download",
    "DownloadDone", "DownloadForOffline", "DragHandle", "ErrorOutline", "Favorite", "FavoriteBorder", "FilterAlt", "GraphicEq",
    "History", "Home", "Info", "KeyboardArrowDown", "LibraryMusic", "Lyrics", "MoreVert", "MusicNote", "NetworkCheck", "Palette",
    "Pause", "Person", "PhoneAndroid", "PlayArrow", "PlaylistAdd", "PlaylistAddCheck", "PlaylistPlay", "PlaylistRemove",
    "PushPin", "QueueMusic", "Refresh", "Remove", "RemoveCircleOutline", "Repeat", "RepeatOne", "RestartAlt", "Save", "Search",
    "SelectAll", "Settings", "SettingsBackupRestore", "Shuffle", "SkipNext", "SkipPrevious", "Sort", "Speaker", "Star",
    "StarOutline", "Stop", "Sync", "Tune", "Visibility", "VisibilityOff", "VolumeUp",
  ] },
  { path: ["Outlined"], fill: false, mirror: false, names: [
    "Album", "BarChart", "Category", "CloudOff", "Download", "FilterAlt", "History", "Home", "Info", "LibraryMusic", "MusicNote",
    "OfflineBolt", "Person", "PushPin", "Search", "StarOutline",
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

/**
 * Re-spells minified SVG path data with explicit separators ("M4 19v-9q0-.475.213-.9"
 * → "M 4 19 v -9 q 0 -0.475 0.213 -0.9"), so Compose's parser never has to split
 * run-together numbers. Arc flags are single digits that may run into the next number.
 */
function normalize(d) {
  const out = [];
  let cmd = "";
  let arg = 0;
  let i = 0;
  const num = /^[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/;
  while (i < d.length) {
    const c = d[i];
    if (/[\s,]/.test(c)) { i++; continue; }
    if (/[MmLlHhVvCcSsQqTtAaZz]/.test(c)) { out.push(c); cmd = c; arg = 0; i++; continue; }
    const flag = /[Aa]/.test(cmd) && (arg % 7 === 3 || arg % 7 === 4);
    const m = flag ? d.slice(i).match(/^[01]/) : d.slice(i).match(num);
    if (!m) throw new Error(`unparseable path data at ${i}: ${d.slice(i, i + 20)}`);
    out.push(flag ? m[0] : String(Number(m[0])));
    arg++;
    i += m[0].length;
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
