// Generates app icons (PNG 32/128/256 + PNG-based ICO) with zero dependencies.
// Run: node scripts/gen-icons.mjs
import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";

const W = 256;

// CRC32 (PNG chunk checksum)
const crcTable = new Int32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c;
});
const crc32 = (buf) => {
  let c = -1;
  for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
};

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const typeBuf = Buffer.from(type, "ascii");
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([typeBuf, data])));
  return Buffer.concat([len, typeBuf, data, crc]);
}

function png(size) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  ihdr[10] = 0;
  ihdr[11] = 0;
  ihdr[12] = 0;
  const raw = Buffer.alloc((size * 4 + 1) * size);
  const cx = size / 2;
  const cy = size / 2;
  const rOuter = size * 0.46;
  const rInner = size * 0.30;
  for (let y = 0; y < size; y++) {
    raw[y * (size * 4 + 1)] = 0; // filter none
    for (let x = 0; x < size; x++) {
      const dx = x - cx;
      const dy = y - cy;
      const d = Math.sqrt(dx * dx + dy * dy);
      const row = y * (size * 4 + 1) + 1 + x * 4;
      // mic glyph: rounded capsule + stand
      const inMic =
        Math.abs(dx) < size * 0.09 && dy > -size * 0.30 && dy < size * 0.06;
      const inArc =
        d > rInner && d < rInner + size * 0.022 && dy > -size * 0.06 && dy < size * 0.14;
      const inStem = Math.abs(dx) < size * 0.02 && dy >= size * 0.14 && dy < size * 0.26;
      const inBase = Math.abs(dx) < size * 0.10 && dy >= size * 0.24 && dy < size * 0.30;
      const inCircle = d < rOuter;
      let r = 0, g = 0, b = 0, a = 0;
      if (inCircle) {
        r = 27; g = 32; b = 44; a = 255; // dark slate disc
      }
      if (inMic || inStem || inBase || inArc) {
        r = 245; g = 248; b = 252; a = 255; // mic glyph
      }
      raw[row] = r;
      raw[row + 1] = g;
      raw[row + 2] = b;
      raw[row + 3] = a;
    }
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

function ico(pngBuf, size) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2); // icon
  header.writeUInt16LE(1, 4); // count
  const entry = Buffer.alloc(16);
  entry[0] = size >= 256 ? 0 : size; // 0 means 256
  entry[1] = size >= 256 ? 0 : size;
  entry[2] = 0;
  entry[3] = 0;
  entry.writeUInt16LE(1, 4); // planes
  entry.writeUInt16LE(32, 6); // bpp
  entry.writeUInt32LE(pngBuf.length, 8);
  entry.writeUInt32LE(6 + 16, 12);
  return Buffer.concat([header, entry, pngBuf]);
}

const dir = "apps/desktop/src-tauri/icons";
mkdirSync(dir, { recursive: true });
const big = png(256);
writeFileSync(`${dir}/128x128.png`, png(128));
writeFileSync(`${dir}/128x128@2x.png`, png(256));
writeFileSync(`${dir}/32x32.png`, png(32));
writeFileSync(`${dir}/icon.png`, big);
writeFileSync(`${dir}/icon.ico`, ico(big, 256));
console.log("icons written");
