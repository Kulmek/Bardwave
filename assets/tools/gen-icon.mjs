#!/usr/bin/env node
/**
 * SoundShared application icon generator.
 *
 * Single source of truth for the app mark. Everything the build ships is
 * produced from the numbers in this file, so the vector source, the PNG set,
 * the ICO container and the raw RGBA blob used by the egui window icon can
 * never drift apart.
 *
 * Design: a squircle of the "Red Noir" brand red (ROSE_500 -> ROSE_700 from
 * `src/gui/theme.rs`) carrying a near-black equaliser mark (#0A0A0A, the app
 * surface colour). No letters, so the same mark works for the working title
 * "SoundShared" and the published name "Bardwave".
 *
 * Outputs (all under ./assets):
 *   icon.svg                 vector source, 256x256 grid
 *   icons/icon-<n>.png       PNG ladder: 16, 20, 24, 32, 40, 48, 64, 128, 256, 512
 *   icon.png                 256px PNG (docs / store listings)
 *   icon.ico                 multi-resolution Windows icon 16..256
 *   icon-64.rgba             raw straight-alpha RGBA, 64x64 (window icon)
 *   icon-preview.png         contact sheet on dark and light backgrounds
 *
 * Usage: node assets/tools/gen-icon.mjs
 */

import { mkdirSync, writeFileSync } from 'node:fs';
import { deflateSync } from 'node:zlib';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const ASSETS = path.join(ROOT, 'assets');

// ---------------------------------------------------------------------------
// Brand
// ---------------------------------------------------------------------------

/** Gradient top: ROSE_500 lit slightly for pop at 16px. */
const TOP = [247, 61, 79];
/** Gradient bottom: ROSE_700 pushed darker so the squircle has depth. */
const BOTTOM = [154, 10, 40];
/** Bar ink: Palette::APP. */
const INK = [10, 10, 10];

// ---------------------------------------------------------------------------
// Geometry (normalised to the 0..1 icon square)
// ---------------------------------------------------------------------------

/** Corner radius of the squircle. */
const RADIUS = 0.224;
/** Same, but a touch tighter on the 16-24px cuts so it still reads as a square. */
const RADIUS_SMALL = 0.196;

/**
 * Small cuts are hand-snapped to whole pixels: proportional bars at 16px come
 * out 1.2px wide with 1.2px gaps, which sub-pixel antialiasing smears into one
 * grey blob. Widths/heights below are in pixels.
 */
const BAR_TABLE = {
  16: { w: 2, g: 2, h: [6, 10, 7] },
  20: { w: 2, g: 3, h: [7, 12, 8] },
  24: { w: 3, g: 3, h: [8, 15, 11] },
  32: { w: 3, g: 2, h: [8, 13, 19, 13, 8] },
  40: { w: 4, g: 3, h: [9, 16, 23, 16, 9] },
  48: { w: 4, g: 4, h: [11, 19, 28, 19, 11] },
  64: { w: 5, g: 3, h: [15, 25, 37, 25, 15] },
};

/** Proportional fallback for cuts big enough that antialiasing is invisible. */
const BARS_PROPORTIONAL = {
  width: 0.082,
  gap: 0.046,
  band: 0.58,
  heights: [0.4, 0.68, 1.0, 0.68, 0.4],
};

/** Sizes that go into the .ico container, smallest first. */
const ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256];
/** Extra ladder cuts written as plain PNGs (Windows/Store/README use). */
const PNG_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256, 512];

const clamp01 = (v) => (v < 0 ? 0 : v > 1 ? 1 : v);
const lerp = (a, b, t) => a + (b - a) * t;

/** Signed distance to a rounded rectangle centred on the origin. */
function sdRoundRect(px, py, hx, hy, r) {
  const qx = Math.abs(px) - hx + r;
  const qy = Math.abs(py) - hy + r;
  const ax = Math.max(qx, 0);
  const ay = Math.max(qy, 0);
  return Math.hypot(ax, ay) + Math.min(Math.max(qx, qy), 0) - r;
}

/** Bar rectangles for a size, in pixels: {x0,x1,y0,y1,r}. */
function barRects(size) {
  const spec = BAR_TABLE[size];
  if (spec) {
    const total = spec.h.length * spec.w + (spec.h.length - 1) * spec.g;
    const left = (size - total) / 2;
    return spec.h.map((hPx, i) => {
      const x0 = left + i * (spec.w + spec.g);
      const y0 = (size - hPx) / 2;
      return { x0, x1: x0 + spec.w, y0, y1: y0 + hPx, r: spec.w / 2 };
    });
  }

  const p = BARS_PROPORTIONAL;
  const n = p.heights.length;
  const total = n * p.width + (n - 1) * p.gap;
  const left = (1 - total) / 2;
  return p.heights.map((hf, i) => {
    const cx = left + i * (p.width + p.gap) + p.width / 2;
    const halfH = (hf * p.band) / 2;
    return {
      x0: (cx - p.width / 2) * size,
      x1: (cx + p.width / 2) * size,
      y0: (0.5 - halfH) * size,
      y1: (0.5 + halfH) * size,
      r: (p.width / 2) * size,
    };
  });
}

/** True when a point lands inside a bar (bars are rounded-ended capsules). */
function inBar(bar, x, y) {
  const cx = (bar.x0 + bar.x1) / 2;
  const cy = (bar.y0 + bar.y1) / 2;
  const halfW = Math.max((bar.x1 - bar.x0) / 2, bar.r);
  const halfH = Math.max((bar.y1 - bar.y0) / 2, bar.r);
  return sdRoundRect(x - cx, y - cy, halfW, halfH, bar.r) <= 0;
}

/** barRects is hit once per supersample; resolve it once per size. */
const BARS_CACHE = new Map();
function barsFor(size) {
  let bars = BARS_CACHE.get(size);
  if (!bars) {
    bars = barRects(size);
    BARS_CACHE.set(size, bars);
  }
  return bars;
}

// ---------------------------------------------------------------------------
// Rasteriser
// ---------------------------------------------------------------------------

/**
 * Colour of one sample point, in pixel coordinates.
 * Returns straight-alpha [r, g, b, a] in 0..255.
 * Layering: squircle gradient -> inner hairline -> equaliser ink.
 */
function sample(px, py, size) {
  const radius = size <= 24 ? RADIUS_SMALL : RADIUS;
  const sd = sdRoundRect(px - size / 2, py - size / 2, size / 2, size / 2, radius * size);
  if (sd > 0) return [0, 0, 0, 0];

  const x = px / size;
  const y = py / size;

  // Diagonal gradient, weighted towards vertical so the squircle reads solid
  // at small sizes where the horizontal spread is only a few pixels.
  const t = clamp01(x * 0.32 + y * 0.68);
  let r = lerp(TOP[0], BOTTOM[0], t);
  let g = lerp(TOP[1], BOTTOM[1], t);
  let b = lerp(TOP[2], BOTTOM[2], t);

  // Soft specular bloom, top-left, very restrained.
  const d = Math.hypot(x - 0.26, y - 0.16) / 0.95;
  if (d < 1) {
    const w = 0.14 * (1 - d) * (1 - d);
    r = lerp(r, 255, w);
    g = lerp(g, 255, w);
    b = lerp(b, 255, w);
  }

  // Hairline inside the edge gives the mark a defined silhouette on both dark
  // and light desktops. Skipped below 48px where it would just eat the shape.
  if (size >= 48 && sd > -1.3) {
    const k = 0.3;
    r = lerp(r, 60, k);
    g = lerp(g, 4, k);
    b = lerp(b, 16, k);
  }

  for (const bar of barsFor(size)) {
    if (inBar(bar, px, py)) return [INK[0], INK[1], INK[2], 255];
  }

  return [r, g, b, 255];
}

/** Renders one size to a straight-alpha RGBA buffer. */
function render(size) {
  const ss = size >= 48 ? 6 : 8; // supersamples per axis
  const out = Buffer.alloc(size * size * 4);
  const n = ss * ss;

  for (let py = 0; py < size; py++) {
    for (let px = 0; px < size; px++) {
      let ar = 0;
      let ag = 0;
      let ab = 0;
      let aa = 0;
      for (let sy = 0; sy < ss; sy++) {
        for (let sx = 0; sx < ss; sx++) {
          const c = sample(px + (sx + 0.5) / ss, py + (sy + 0.5) / ss, size);
          const a = c[3] / 255;
          ar += c[0] * a;
          ag += c[1] * a;
          ab += c[2] * a;
          aa += a;
        }
      }
      const i = (py * size + px) * 4;
      if (aa > 0) {
        out[i] = Math.round(clamp01(ar / aa / 255) * 255);
        out[i + 1] = Math.round(clamp01(ag / aa / 255) * 255);
        out[i + 2] = Math.round(clamp01(ab / aa / 255) * 255);
        out[i + 3] = Math.round((aa / n) * 255);
      }
    }
  }
  return out;
}

// ---------------------------------------------------------------------------
// PNG encoding (no dependencies: node's zlib + a CRC table)
// ---------------------------------------------------------------------------

const CRC_TABLE = (() => {
  const table = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c;
  }
  return table;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function pngChunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body), 0);
  return Buffer.concat([len, body, crc]);
}

function encodePNG(width, height, rgba) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // colour type: truecolour + alpha
  ihdr[10] = 0; // deflate
  ihdr[11] = 0; // adaptive filtering
  ihdr[12] = 0; // no interlace

  const stride = width * 4;
  const raw = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (stride + 1)] = 0; // filter: None
    rgba.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }

  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    pngChunk('IHDR', ihdr),
    pngChunk('IDAT', deflateSync(raw, { level: 9 })),
    pngChunk('IEND', Buffer.alloc(0)),
  ]);
}

// ---------------------------------------------------------------------------
// ICO container
// ---------------------------------------------------------------------------

/** 32bpp bottom-up DIB entry (BITMAPINFOHEADER + XOR bitmap + AND mask). */
function dibEntry(size, rgba) {
  const xorSize = size * size * 4;
  const maskStride = Math.ceil(size / 32) * 4;
  const andSize = maskStride * size;

  const header = Buffer.alloc(40);
  header.writeUInt32LE(40, 0); // biSize
  header.writeInt32LE(size, 4); // biWidth
  header.writeInt32LE(size * 2, 8); // biHeight: XOR + AND
  header.writeUInt16LE(1, 12); // biPlanes
  header.writeUInt16LE(32, 14); // biBitCount
  header.writeUInt32LE(0, 16); // BI_RGB
  header.writeUInt32LE(xorSize + andSize, 20); // biSizeImage
  header.writeInt32LE(0, 24);
  header.writeInt32LE(0, 28);
  header.writeUInt32LE(0, 32);
  header.writeUInt32LE(0, 36);

  const xor = Buffer.alloc(xorSize);
  for (let y = 0; y < size; y++) {
    const srcRow = size - 1 - y; // DIB rows are bottom-up
    for (let x = 0; x < size; x++) {
      const s = (srcRow * size + x) * 4;
      const d = (y * size + x) * 4;
      xor[d] = rgba[s + 2]; // B
      xor[d + 1] = rgba[s + 1]; // G
      xor[d + 2] = rgba[s]; // R
      xor[d + 3] = rgba[s + 3]; // A
    }
  }

  return Buffer.concat([header, xor, Buffer.alloc(andSize, 0)]);
}

function buildICO(entries) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0); // reserved
  header.writeUInt16LE(1, 2); // type: icon
  header.writeUInt16LE(entries.length, 4);

  const dir = Buffer.alloc(16 * entries.length);
  let offset = 6 + dir.length;

  entries.forEach((entry, i) => {
    const base = i * 16;
    dir[base] = entry.size >= 256 ? 0 : entry.size; // 0 means 256
    dir[base + 1] = entry.size >= 256 ? 0 : entry.size;
    dir[base + 2] = 0; // palette colours
    dir[base + 3] = 0; // reserved
    dir.writeUInt16LE(1, base + 4); // planes
    dir.writeUInt16LE(32, base + 6); // bit depth
    dir.writeUInt32LE(entry.data.length, base + 8);
    dir.writeUInt32LE(offset, base + 12);
    offset += entry.data.length;
  });

  return Buffer.concat([header, dir, ...entries.map((e) => e.data)]);
}

// ---------------------------------------------------------------------------
// SVG (same geometry, so the vector and raster cuts stay in sync)
// ---------------------------------------------------------------------------

const fmt = (v) => Number(v.toFixed(4));

function buildSVG(size = 256) {
  const radius = fmt(RADIUS * size);
  // Same geometry as the 64px raster cut, scaled up to the SVG grid.
  const k = size / 64;
  const bars = barRects(64).map((b) =>
    [
      `    <rect x="${fmt(b.x0 * k)}" y="${fmt(b.y0 * k)}"`,
      `width="${fmt((b.x1 - b.x0) * k)}" height="${fmt((b.y1 - b.y0) * k)}"`,
      `rx="${fmt(b.r * k)}" fill="#${INK.map((c) => c.toString(16).padStart(2, '0')).join('')}"/>`,
    ].join(' '),
  );

  const hex = (c) => `#${c.map((v) => v.toString(16).padStart(2, '0')).join('')}`;

  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${size} ${size}" role="img" aria-label="SoundShared">
  <title>SoundShared</title>
  <defs>
    <linearGradient id="brand" x1="0.16" y1="0" x2="0.84" y2="1">
      <stop offset="0" stop-color="${hex(TOP)}"/>
      <stop offset="1" stop-color="${hex(BOTTOM)}"/>
    </linearGradient>
    <radialGradient id="bloom" cx="0.26" cy="0.16" r="0.95" fx="0.26" fy="0.16">
      <stop offset="0" stop-color="#ffffff" stop-opacity="0.16"/>
      <stop offset="1" stop-color="#ffffff" stop-opacity="0"/>
    </radialGradient>
  </defs>
  <rect width="${size}" height="${size}" rx="${radius}" fill="url(#brand)"/>
  <rect width="${size}" height="${size}" rx="${radius}" fill="url(#bloom)"/>
  <rect x="1.3" y="1.3" width="${fmt(size - 2.6)}" height="${fmt(size - 2.6)}" rx="${fmt(radius - 1.3)}" fill="none" stroke="#3c0410" stroke-opacity="0.3" stroke-width="1.3"/>
${bars.join('\n')}
</svg>
`;
}

// ---------------------------------------------------------------------------
// Preview contact sheet
// ---------------------------------------------------------------------------

/** Blits an RGBA tile into a larger canvas with nearest-neighbour scaling. */
function blitTile(canvas, cw, ch, tile, tsize, atX, atY, scale) {
  for (let y = 0; y < tsize * scale; y++) {
    for (let x = 0; x < tsize * scale; x++) {
      const s = ((y / scale) | 0) * tsize + ((x / scale) | 0);
      const a = tile[s * 4 + 3] / 255;
      if (a === 0) continue;
      const d = ((atY + y) * cw + (atX + x)) * 4;
      canvas[d] = Math.round(tile[s * 4] * a + canvas[d] * (1 - a));
      canvas[d + 1] = Math.round(tile[s * 4 + 1] * a + canvas[d + 1] * (1 - a));
      canvas[d + 2] = Math.round(tile[s * 4 + 2] * a + canvas[d + 2] * (1 - a));
      canvas[d + 3] = 255;
    }
  }
}

/**
 * Contact sheet: native cuts on a dark desktop and a light desktop (left/right),
 * plus integer-zoomed enlargements of the small cuts so the bar geometry can be
 * judged. Purely a documentation artifact.
 */
function buildPreview(raster) {
  const cw = 1400;
  const ch = 580;
  const half = cw / 2;
  const canvas = Buffer.alloc(cw * ch * 4);

  for (let y = 0; y < ch; y++) {
    for (let x = 0; x < cw; x++) {
      const i = (y * cw + x) * 4;
      const v = x < half ? 24 : 236; // dark desktop | light desktop
      canvas[i] = v;
      canvas[i + 1] = v;
      canvas[i + 2] = v;
      canvas[i + 3] = 255;
    }
  }

  const tile = (size, atX, atY, scale = 1) =>
    blitTile(canvas, cw, ch, raster.get(size), size, atX, atY, scale);

  // --- dark column ---
  // 1. Native-size ladder, the way Explorer draws the file at 100%.
  let x = 36;
  for (const size of [16, 24, 32, 48, 64, 128]) {
    tile(size, x, 40 + (128 - size) / 2); // baselines aligned on the 128 row
    x += size + 24;
  }
  // 2. The two big cuts: 256 native, 128 at 2x.
  tile(256, 36, 190);
  tile(128, 36 + 256 + 26, 190, 2);
  // 3. Integer-zoomed small cuts, for judging bar geometry.
  x = 36;
  for (const [size, scale] of [
    [16, 5],
    [24, 4],
    [32, 3],
    [48, 2],
  ]) {
    tile(size, x, 460, scale);
    x += size * scale + 20;
  }

  // --- light column ---
  tile(256, half + 36, 40);
  tile(128, half + 36 + 256 + 30, 40, 2);
  let lx = half + 36;
  for (const [size, scale] of [
    [64, 3],
    [32, 4],
    [16, 6],
  ]) {
    tile(size, lx, 40 + 256 + 40, scale);
    lx += size * scale + 30;
  }

  return encodePNG(cw, ch, canvas);
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

function main() {
  mkdirSync(path.join(ASSETS, 'icons'), { recursive: true });

  const raster = new Map();

  for (const size of PNG_SIZES) {
    const rgba = render(size);
    raster.set(size, rgba);
    const png = encodePNG(size, size, rgba);
    writeFileSync(path.join(ASSETS, 'icons', `icon-${size}.png`), png);
    if (size === 256) writeFileSync(path.join(ASSETS, 'icon.png'), png);
    console.log(`  assets/icons/icon-${size}.png  ${png.length} B`);
  }

  // Icon container: every entry is a plain 32bpp bitmap. PNG-compressed
  // entries (the usual trick to keep the 256px cut small) are readable by the
  // Windows shell but not by GDI+/Java/Avalonia-style decoders, and this file
  // has to survive whoever opens the binary afterwards.
  const entries = ICO_SIZES.map((size) => ({
    size,
    data: dibEntry(size, raster.get(size) ?? render(size)),
  }));
  const ico = buildICO(entries);
  writeFileSync(path.join(ASSETS, 'icon.ico'), ico);
  console.log(`  assets/icon.ico  ${ico.length} B (${ICO_SIZES.length} resolutions)`);

  // Raw RGBA for `egui::IconData` at runtime (no PNG decoder in the app deps).
  const windowIcon = raster.get(64);
  writeFileSync(path.join(ASSETS, 'icon-64.rgba'), windowIcon);
  console.log(`  assets/icon-64.rgba  ${windowIcon.length} B (64x64 RGBA)`);

  const svg = buildSVG(256);
  writeFileSync(path.join(ASSETS, 'icon.svg'), svg);
  console.log(`  assets/icon.svg  ${Buffer.byteLength(svg)} B`);

  const preview = buildPreview(raster);
  writeFileSync(path.join(ASSETS, 'icon-preview.png'), preview);
  console.log(`  assets/icon-preview.png  ${preview.length} B`);
}

main();

// `node assets/tools/gen-icon.mjs --ascii` prints the small cuts as text so the bar
// geometry can be checked without opening an image viewer.
if (process.argv.includes('--ascii')) {
  for (const size of [16, 20, 24, 32]) {
    const rgba = render(size);
    console.log(`\n--- ${size}px ---`);
    for (let y = 0; y < size; y++) {
      let row = '';
      for (let x = 0; x < size; x++) {
        const i = (y * size + x) * 4;
        const a = rgba[i + 3] / 255;
        const lum = (rgba[i] * 0.299 + rgba[i + 1] * 0.587 + rgba[i + 2] * 0.114) / 255;
        if (a < 0.35) row += ' ';
        else if (a < 0.85) row += '+';
        else if (lum < 0.25) row += '#';
        else row += ':';
      }
      console.log(row);
    }
  }
}

