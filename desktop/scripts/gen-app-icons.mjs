// Renders the packaged app icons from the Crescendo mark:
//
//   build/icon.icon/Assets/note-{1..4}.svg   the four notes, as Icon Composer layers
//   build/icon.ico                           Windows, 16–256
//   build/icons/<n>x<n>.png                  Linux (AppImage, deb), 16–512
//
//   node scripts/gen-app-icons.mjs
//
// macOS only: `sips` (CoreSVG) rasterises the SVGs. The output is committed, so
// nothing else needs a Mac. build/icon.icon itself (background, glass, shadow) is
// edited in Icon Composer; electron-builder compiles it with actool and derives
// the flat .icns for macOS 15 and earlier from it. build/icon.svg is the flat
// master on Apple's grid that the in-app mark (src/main/brand.ts) matches.
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const build = join(dirname(fileURLToPath(import.meta.url)), "..", "build");

/** The mark on its 100-unit tile: [voice, path] in drawing order. */
const WAVE = [
  [1, "M16 48.5 C23 37.81 26 37.81 33 48.5"],
  [2, "M33 48.5 C40 63.2 43 63.2 50 48.5"],
  [1, "M50 48.5 C57 29.8 60 29.8 67 48.5"],
  [2, "M67 48.5 C74 71.21 77 71.21 84 48.5"],
];
/** Tonal Violet, dark tile. */
const INK = { 1: "#E4DFFF", 2: "#8F80F5" };
const BODY = ["#2A2447", "#1B1730"]; // a subtle vertical gradient around #221D3A, as in icon.svg

const wave = (notes = WAVE) =>
  `<g fill="none" stroke-width="12" stroke-linecap="round">${notes.map(([v, d]) => `<path stroke="${INK[v]}" d="${d}"/>`).join("")}</g>`;

const svg = (size, body) =>
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="${size}" height="${size}">${body}</svg>`; // no trailing newline: Icon Composer strips it when it saves

// Icon Composer layers: the full 1024 pt canvas is the icon shape, so the tile maps onto it
// one to one. One layer per note (half-cycle), not per voice: stacked in playing order, each
// note's round cap overlaps the one before it, as in the flat mark.
mkdirSync(join(build, "icon.icon", "Assets"), { recursive: true });
WAVE.forEach((note, i) => writeFileSync(join(build, "icon.icon", "Assets", `note-${i + 1}.svg`), svg(1024, wave([note]))));

// Windows and Linux: the tile, nearly full bleed (no Apple grid inset, no shadow), so the
// mark keeps as many pixels as it can at 16 px. Corner radius as on macOS (22.4%).
const tile = (size) =>
  svg(
    size,
    `<defs><linearGradient id="b" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${BODY[0]}"/><stop offset="1" stop-color="${BODY[1]}"/></linearGradient></defs>` +
      `<rect x="3" y="3" width="94" height="94" rx="21" fill="url(#b)"/>` +
      `<g transform="translate(3 3) scale(0.94)">${wave()}</g>`,
  );

const tmp = mkdtempSync(join(tmpdir(), "hocket-icons-"));
const render = (size) => {
  const src = join(tmp, `${size}.svg`);
  const out = join(tmp, `${size}.png`);
  writeFileSync(src, tile(size));
  execFileSync("sips", ["-s", "format", "png", src, "--out", out], { stdio: "ignore" });
  return readFileSync(out);
};

try {
  rmSync(join(build, "icons"), { recursive: true, force: true });
  mkdirSync(join(build, "icons"));
  for (const n of [16, 24, 32, 48, 64, 128, 256, 512]) writeFileSync(join(build, "icons", `${n}x${n}.png`), render(n));
  writeFileSync(join(build, "icon.ico"), ico([16, 24, 32, 48, 64, 128, 256].map((n) => [n, render(n)])));
} finally {
  rmSync(tmp, { recursive: true, force: true });
}

/** An .ico of PNG entries (Windows Vista and later read PNG-compressed entries at any size). */
function ico(images) {
  const head = Buffer.alloc(6 + 16 * images.length);
  head.writeUInt16LE(0, 0);
  head.writeUInt16LE(1, 2); // type: icon
  head.writeUInt16LE(images.length, 4);
  let offset = head.length;
  images.forEach(([n, png], i) => {
    const e = 6 + 16 * i;
    head[e] = n >= 256 ? 0 : n; // 0 means 256
    head[e + 1] = n >= 256 ? 0 : n;
    head.writeUInt16LE(1, e + 4); // colour planes
    head.writeUInt16LE(32, e + 6); // bits per pixel
    head.writeUInt32LE(png.length, e + 8);
    head.writeUInt32LE(offset, e + 12);
    offset += png.length;
  });
  return Buffer.concat([head, ...images.map(([, png]) => png)]);
}
