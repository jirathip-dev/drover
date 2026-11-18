// Design tooling (#1305): `node gif.mjs <out.gif> <fps> <frame.png>…` encodes Chrome's RGBA PNG shots as one looping GIF
// with no dependency: a PNG reader (zlib inflate, the five row filters) and a GIF89a LZW writer over one shared palette
// (the 256 most-used 5-bit colours of all frames, each pixel to its nearest).
import { readFileSync, writeFileSync } from 'node:fs';
import { inflateSync } from 'node:zlib';

function png(path) {
  const b = readFileSync(path);
  let o = 8, w = 0, h = 0, ct = 0;
  const idat = [];
  while (o < b.length) {
    const n = b.readUInt32BE(o), t = b.toString('ascii', o + 4, o + 8), d = b.subarray(o + 8, o + 8 + n);
    if (t === 'IHDR') { w = d.readUInt32BE(0); h = d.readUInt32BE(4); ct = d[9]; }
    if (t === 'IDAT') idat.push(d);
    o += 12 + n;
  }
  const bpp = ct === 6 ? 4 : 3, raw = inflateSync(Buffer.concat(idat)), s = w * bpp, out = Buffer.alloc(w * h * 4);
  let prev = Buffer.alloc(s);
  for (let y = 0; y < h; y++) {
    const f = raw[y * (s + 1)], row = Buffer.from(raw.subarray(y * (s + 1) + 1, (y + 1) * (s + 1)));
    for (let x = 0; x < s; x++) {
      const a = x >= bpp ? row[x - bpp] : 0, u = prev[x], c = x >= bpp ? prev[x - bpp] : 0;
      const p = a + u - c, pa = Math.abs(p - a), pb = Math.abs(p - u), pc = Math.abs(p - c);
      row[x] = (row[x] + [0, a, u, (a + u) >> 1, pa <= pb && pa <= pc ? a : pb <= pc ? u : c][f]) & 255;
    }
    for (let x = 0; x < w; x++) for (let k = 0; k < 3; k++) out[(y * w + x) * 4 + k] = row[x * bpp + k];
    prev = row;
  }
  return { w, h, px: out };
}

function lzw(ix, min) {
  const out = [], clear = 1 << min, eoi = clear + 1;
  let size = min + 1, next = eoi + 1, dict = new Map(), cur = 0, bits = 0;
  const put = c => { cur |= c << bits; bits += size; while (bits >= 8) { out.push(cur & 255); cur >>= 8; bits -= 8; } };
  put(clear);
  let w = ix[0];
  for (let i = 1; i < ix.length; i++) {
    const k = ix[i], key = w * 4096 + k, got = dict.get(key);
    if (got !== undefined) { w = got; continue; }
    put(w);
    if (next < 4096) { dict.set(key, next++); if (next > 1 << size && size < 12) size++; }
    else { put(clear); dict = new Map(); size = min + 1; next = eoi + 1; }
    w = k;
  }
  put(w);
  put(eoi);
  if (bits) out.push(cur & 255);
  const blocks = [];
  for (let i = 0; i < out.length; i += 255) blocks.push(Math.min(255, out.length - i), ...out.slice(i, i + 255));
  return Buffer.from([min, ...blocks, 0]);
}

const [outPath, fps, ...files] = process.argv.slice(2), frames = files.map(png), { w, h } = frames[0];
const hist = new Map();
for (const f of frames) for (let i = 0; i < f.px.length; i += 16) { const k = (f.px[i] >> 3) << 10 | (f.px[i + 1] >> 3) << 5 | f.px[i + 2] >> 3; hist.set(k, (hist.get(k) || 0) + 1); }
const pal = [...hist.entries()].sort((a, b) => b[1] - a[1]).slice(0, 256).map(([k]) => [(k >> 10) << 3 | 4, ((k >> 5) & 31) << 3 | 4, (k & 31) << 3 | 4]);
while (pal.length < 256) pal.push([0, 0, 0]);
const near = new Map(), pick = k => {
  if (near.has(k)) return near.get(k);
  const r = (k >> 10) << 3, g = ((k >> 5) & 31) << 3, b = (k & 31) << 3;
  let best = 0, bd = Infinity;
  pal.forEach(([pr, pg, pb], i) => { const d = (pr - r) ** 2 + (pg - g) ** 2 + (pb - b) ** 2; if (d < bd) { bd = d; best = i; } });
  near.set(k, best);
  return best;
};
const parts = [Buffer.from('GIF89a'), Buffer.from([w & 255, w >> 8, h & 255, h >> 8, 0xf7, 0, 0]), Buffer.from(pal.flat()),
  Buffer.from([0x21, 0xff, 11, ...Buffer.from('NETSCAPE2.0'), 3, 1, 0, 0, 0])];
const delay = Math.round(100 / +fps);
for (const f of frames) {
  const ix = new Uint8Array(w * h);
  for (let i = 0; i < w * h; i++) ix[i] = pick((f.px[i * 4] >> 3) << 10 | (f.px[i * 4 + 1] >> 3) << 5 | f.px[i * 4 + 2] >> 3);
  parts.push(Buffer.from([0x21, 0xf9, 4, 0, delay & 255, delay >> 8, 0, 0]), Buffer.from([0x2c, 0, 0, 0, 0, w & 255, w >> 8, h & 255, h >> 8, 0]), lzw(ix, 8));
}
parts.push(Buffer.from([0x3b]));
writeFileSync(outPath, Buffer.concat(parts));
