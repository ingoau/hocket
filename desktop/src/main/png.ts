// Minimal PNG encoder (RGBA, 8-bit, zlib via node). Used for the brand mark
// (brand.ts) and the fake core's generated artwork so no binary assets need to be committed.
import { deflateSync } from "node:zlib";

const CRC_TABLE = new Int32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c;
});

function crc32(buf: Uint8Array): number {
  let c = -1;
  for (let i = 0; i < buf.length; i++) c = (CRC_TABLE[(c ^ (buf[i] as number)) & 0xff] as number) ^ (c >>> 8);
  return (c ^ -1) >>> 0;
}

function chunk(type: string, data: Uint8Array): Buffer {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const typeBuf = Buffer.from(type, "ascii");
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([typeBuf, Buffer.from(data)])), 0);
  return Buffer.concat([len, typeBuf, Buffer.from(data), crc]);
}

/** Encode `rgba` (width*height*4 bytes) as a PNG. */
export function encodePng(width: number, height: number, rgba: Uint8Array): Buffer {
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (width * 4 + 1)] = 0; // filter: none
    raw.set(rgba.subarray(y * width * 4, (y + 1) * width * 4), y * (width * 4 + 1) + 1);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  ihdr[10] = 0;
  ihdr[11] = 0;
  ihdr[12] = 0;
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw)),
    chunk("IEND", new Uint8Array(0)),
  ]);
}

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

export function hslToRgb(h: number, s: number, l: number): Rgb {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  let r = 0;
  let g = 0;
  let b = 0;
  if (h < 60) [r, g, b] = [c, x, 0];
  else if (h < 120) [r, g, b] = [x, c, 0];
  else if (h < 180) [r, g, b] = [0, c, x];
  else if (h < 240) [r, g, b] = [0, x, c];
  else if (h < 300) [r, g, b] = [x, 0, c];
  else [r, g, b] = [c, 0, x];
  return { r: Math.round((r + m) * 255), g: Math.round((g + m) * 255), b: Math.round((b + m) * 255) };
}

/**
 * A deterministic "album cover": two-tone diagonal gradient with a circle,
 * coloured from the seed. Distinct enough that dynamic accent extraction has
 * something to work with.
 */
export function coverPng(seed: number, size: number): Buffer {
  const hue = seed % 360;
  const hue2 = (hue + 40 + (seed % 90)) % 360;
  const a = hslToRgb(hue, 0.55, 0.42);
  const b = hslToRgb(hue2, 0.6, 0.62);
  const dot = hslToRgb((hue + 180) % 360, 0.5, 0.85);
  const px = new Uint8Array(size * size * 4);
  const cx = size * (0.35 + ((seed >> 3) % 30) / 100);
  const cy = size * (0.35 + ((seed >> 7) % 30) / 100);
  const radius = size * 0.22;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const t = (x + y) / (2 * size);
      let r = a.r + (b.r - a.r) * t;
      let g = a.g + (b.g - a.g) * t;
      let bl = a.b + (b.b - a.b) * t;
      const d = Math.hypot(x - cx, y - cy);
      if (d < radius) {
        const k = Math.min(1, (radius - d) / 2);
        r = r + (dot.r - r) * k;
        g = g + (dot.g - g) * k;
        bl = bl + (dot.b - bl) * k;
      }
      const i = (y * size + x) * 4;
      px[i] = r;
      px[i + 1] = g;
      px[i + 2] = bl;
      px[i + 3] = 255;
    }
  }
  return encodePng(size, size, px);
}
