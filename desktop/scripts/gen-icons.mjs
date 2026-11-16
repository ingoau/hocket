// Generates src/renderer/components/icons.ts: the Phosphor glyphs the renderer
// uses, as SVG path bodies, from the Iconify dataset (@iconify-json/ph, MIT).
// Only the icons listed here are emitted, so the app doesn't ship the whole set.
//
//   node scripts/gen-icons.mjs
//
// Each renderer name maps to a Phosphor name; for each we emit the Regular
// weight (outlined) and the Fill weight (filled). Re-run after changing the
// list; the output is committed.
import { writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { pathBBox } from "./svg-bbox.mjs";

const require = createRequire(import.meta.url);
const set = require("@iconify-json/ph/icons.json");

/**
 * The part of Phosphor's 256 box Icon.tsx draws (a margin trimmed, so glyphs sit at Geist's optical
 * size). A glyph reaching into the trimmed margin is scaled about the centre to fit inside it.
 */
const TRIM = 8;
const BOX = 256;

/**
 * Names drawn outlined only: their Fill weight is a solid tile with the glyph knocked out, which
 * would turn an active state (shown by colour) into a block.
 */
const OUTLINE_ONLY = new Set([
  "equalizer", "transcode", "code", "sortAlpha", "sort", "queue", "lyrics", "check", "close", "plus", "remove", "minimize", "more", "moreVert",
  "autoplay", "grip", "dragHandle", "sidePanel", "sidebarClose", "sidebarOpen",
]);

/** Renderer name → Phosphor name (or `{ name, mirror }` for a glyph flipped left to right). */
const MAP = {
  play: "play", pause: "pause", stop: "stop", next: "skip-forward", previous: "skip-back", rewind: "rewind", forward: "fast-forward",
  shuffle: "shuffle", repeat: "repeat", repeatOne: "repeat-once", autoplay: "infinity", volume: "speaker-high", mute: "speaker-slash",
  heart: "heart", heartOff: "heart-break", star: "star", starOff: "x-circle",
  queue: "queue", lyrics: "microphone-stage", fullscreen: "corners-out", fullscreenExit: "corners-in", mini: "picture-in-picture", devices: "devices",
  search: "magnifying-glass", command: "command", settings: "gear-six",
  home: "house", library: "books", album: "vinyl-record", artist: "user", playlist: "playlist", playlistAdd: "list-plus", song: "music-note", genre: "shapes",
  download: "download-simple", downloadOff: "cloud-slash", cached: "hard-drive", filter: "funnel", filterAdd: "plus-square", stats: "chart-line-up", info: "info",
  remove: "minus", trash: "trash", edit: "pencil-simple", close: "x", check: "check",
  chevronDown: "caret-down", chevronRight: "caret-right", chevronLeft: "caret-left", chevronUp: "caret-up",
  arrowDown: "arrow-down", arrowUp: "arrow-up", back: "arrow-left", forwardArrow: "arrow-right", undo: "arrow-counter-clockwise", redo: "arrow-clockwise",
  playNext: "arrow-bend-down-right", playLater: "rows-plus-bottom", pin: "push-pin", pinOff: "push-pin-slash", restore: "clock-counter-clockwise", seek: "arrow-right", sleep: "moon", bug: "bug",
  selectAll: "selection-all", grip: "dots-six-vertical", dragHandle: "dots-six", more: "dots-three", moreVert: "dots-three-vertical", minimize: "minus", maximize: "square", restoreWin: "copy-simple",
  spinner: "circle-notch", offline: "wifi-slash", warn: "warning", battery: "battery-low", external: "arrow-square-out", folder: "folder",
  copy: "copy", qr: "qr-code", resume: "play-circle", music: "music-note", cloud: "cloud", sync: "arrows-clockwise", lan: "network", plus: "plus",
  // The song stash (#150).
  stash: "stack",
  // Now playing (#47): device kinds, the side panel toggle, the connect dropdown and the fullscreen glyphs.
  phone: "device-mobile", laptop: "laptop", sidePanel: { name: "sidebar-simple", mirror: true }, dropDown: "caret-down", openInFull: "arrows-out-simple", closeFullscreen: "arrows-in-simple",
  // Edit pins: a chip that says a pin opens when clicked.
  arrowOutward: "arrow-up-right",
  // The sidebar: its collapse toggle (top bar) and Your library's sort button.
  sidebarClose: "arrow-line-left", sidebarOpen: "arrow-line-right", sort: "sort-ascending",
  // Settings sections, setup and stats.
  visibility: "eye", visibilityOff: "eye-slash",
  tune: "sliders-horizontal", equalizer: "waveform", transcode: "swap", connectCast: "screencast", palette: "palette", keyboard: "keyboard", backup: "cloud-arrow-up", customise: "layout", headphones: "headphones", schedule: "clock", dns: "database", key: "key",
  // The library lists' sort popover: one leading icon per sort.
  sortAlpha: "text-aa", calendar: "calendar-blank", update: "clock-clockwise", timer: "timer", speed: "gauge",
  // Advanced settings and the developer diagnostics page.
  code: "code", share: "share-network", save: "floppy-disk", refresh: "arrows-clockwise",
};

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

/** The glyph's extent in its box. */
function extent(svg) {
  const b = { x0: Infinity, y0: Infinity, x1: -Infinity, y1: -Infinity };
  for (const m of svg.matchAll(/ d="([^"]+)"/g)) {
    const p = pathBBox(m[1]);
    b.x0 = Math.min(b.x0, p.x0); b.y0 = Math.min(b.y0, p.y0); b.x1 = Math.max(b.x1, p.x1); b.y1 = Math.max(b.y1, p.y1);
  }
  return b;
}

/** The body, mirrored if asked and scaled about the centre if it reaches into the trimmed margin. */
function fit(svg, mirror, label) {
  const c = BOX / 2;
  const e = extent(svg);
  const reach = Math.max(c - e.x0, e.x1 - c, c - e.y0, e.y1 - c);
  const k = reach > c - TRIM ? (c - TRIM) / reach : 1;
  const transforms = [];
  if (mirror) transforms.push(`matrix(-1 0 0 1 ${BOX} 0)`);
  if (k < 1) transforms.push(`translate(${c} ${c}) scale(${k.toFixed(4)}) translate(${-c} ${-c})`);
  // The check: the drawn glyph's edges (mirrored, scaled) all lie inside the trimmed box.
  const xs = [e.x0, e.x1].map((x) => c + ((mirror ? BOX - x : x) - c) * k);
  const ys = [e.y0, e.y1].map((y) => c + (y - c) * k);
  for (const v of [...xs, ...ys]) {
    if (v < TRIM - 0.01 || v > BOX - TRIM + 0.01) throw new Error(`${label} reaches ${v.toFixed(2)}, outside ${TRIM}–${BOX - TRIM}`);
  }
  return transforms.length ? `<g transform="${transforms.join(" ")}">${svg}</g>` : svg;
}

const out = {};
const missing = [];
const fitted = [];
for (const [local, entry] of Object.entries(MAP)) {
  const { name: ph, mirror = false } = typeof entry === "string" ? { name: entry } : entry;
  const outline = body(ph);
  const filled = body(`${ph}-fill`);
  if (!filled || !outline) { missing.push(`${local} → ${ph}`); continue; }
  const o = fit(outline, mirror, local);
  const f = fit(filled, mirror, `${local} (fill)`);
  if (o.includes("scale(") || f.includes("scale(")) fitted.push(local);
  out[local] = OUTLINE_ONLY.has(local) ? [o] : [o, f];
}
if (missing.length) {
  console.error(`missing Phosphor icons: ${missing.join(", ")}`);
  process.exit(1);
}
if (fitted.length) console.log(`[gen-icons] scaled to fit the trimmed box: ${fitted.join(", ")}`);

const target = fileURLToPath(new URL("../src/renderer/components/icons.ts", import.meta.url));
const lines = Object.entries(out).map(([k, v]) => `  ${k}: ${JSON.stringify(v)},`);
writeFileSync(target, `// Generated by scripts/gen-icons.mjs from @iconify-json/ph (Phosphor Icons, MIT,
// Regular and Fill weights). Do not edit; edit the script's map.
// Each entry: [outlined, filled?] SVG bodies for a ${BOX}×${BOX} viewBox (filled left out
// where Phosphor's Fill is a knockout tile).
export const ICON_SIZE = ${BOX};
/** The margin of the box Icon.tsx leaves out; every glyph fits inside the rest (checked here). */
export const ICON_TRIM = ${TRIM};
export const ICONS: Record<string, readonly [string, string?]> = {
${lines.join("\n")}
};
`);
console.log(`[gen-icons] ${Object.keys(out).length} icons → ${target}`);
