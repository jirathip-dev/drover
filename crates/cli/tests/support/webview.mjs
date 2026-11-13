// #862: Playwright's WebKit (Linux CI, iOS Safari's engine) renders a page at a phone width; prints `scrollWidth
// viewportWidth` of the body with every <details> open, and optionally saves a full-height PNG (with `js`: run it after
// the tab/mode switch and save one 844 px tall screen, e.g. an open sheet).
// Numbers the js leaves in `window.measure` are printed after the two widths (with a png, `window.late` instead if set).
// #200, with `close`: the page under a sheet as it closes. A baseline before `js` opens the sheet, then `close` and
// screenshots back to back for 400 ms; prints the frame count, the worst frame's share of pixels above the open sheet
// that are not the baseline dimmed evenly, no darker than under the open sheet (the backdrop fading), and how dimmed
// the last frame is (0: the page as it was); the worst frame goes to out.png.
// #206: `HERDSMAN_TAB` is the stored tab before the page loads; tab `-`: no tab()/mode() and no <details> opened.
// #334: `<page.html>` may be an http URL instead (a real `serve`), loaded with the `HERDSMAN_COOKIE` (`name=value`) set;
// `HERDSMAN_WAIT` is the wait (s) before `window.late` is read (default 0.5). `HERDSMAN_PRE`: js run at document start.
// #337 `HERDSMAN_WARM`: the URL loads twice in one WebKit (its caches and service worker warm).
// A file's page is served at `HERDSMAN_BASE` (serve's assets beside it). Playwright resolves through NODE_PATH.
// node webview.mjs <page.html | http URL> <width> <tab> <mode> [out.png] [js] [close]
import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { inflateSync } from "node:zlib";

const { webkit } = createRequire(import.meta.url)("playwright");
const [page0, w, tab, mode, png, extra = "", close] = process.argv.slice(2);
const env = process.env;
const url = page0.startsWith("http") ? page0 : null;
const width = Number(w);
const sleep = (s) => new Promise((r) => setTimeout(r, s * 1000));
const done = (code, ...line) => {
  if (line.length) console.log(line.join(" "));
  process.exit(code);
};
let at = "launch";
setTimeout(() => done(3, `timeout at ${at}`), 30_000);

// A PNG's RGBA bytes, row by row (8-bit RGB or RGBA, as Playwright writes them).
function rgba(buf) {
  const W = buf.readUInt32BE(16), H = buf.readUInt32BE(20), type = buf[25];
  const bpp = type === 6 ? 4 : 3;
  if (buf[24] !== 8 || (type !== 6 && type !== 2)) throw new Error(`png type ${type}/${buf[24]}`);
  const idat = [];
  for (let i = 8; i < buf.length; ) {
    const n = buf.readUInt32BE(i), t = buf.toString("ascii", i + 4, i + 8);
    if (t === "IDAT") idat.push(buf.subarray(i + 8, i + 8 + n));
    i += 12 + n;
  }
  const raw = inflateSync(Buffer.concat(idat)), row = W * bpp, px = Buffer.alloc(row * H);
  for (let y = 0; y < H; y++) {
    const f = raw[y * (row + 1)], src = raw.subarray(y * (row + 1) + 1, (y + 1) * (row + 1));
    for (let x = 0; x < row; x++) {
      const a = x >= bpp ? px[y * row + x - bpp] : 0, b = y ? px[(y - 1) * row + x] : 0;
      const c = x >= bpp && y ? px[(y - 1) * row + x - bpp] : 0;
      const p = a + b - c, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
      const pred = [0, a, b, (a + b) >> 1, pa <= pb && pa <= pc ? a : pb <= pc ? b : c][f];
      px[y * row + x] = (src[x] + pred) & 255;
    }
  }
  if (bpp === 4) return px;
  const out = Buffer.alloc(W * H * 4, 255);
  for (let i = 0; i < W * H; i++) px.copy(out, i * 4, i * 3, i * 3 + 3);
  return out;
}
// Over the rows above `top` (CSS px): the frame as the baseline times k (the black backdrop at some opacity), and the
// share of pixels off that by more than 12 on a channel; with `lo`, k under it (darker than the open sheet's backdrop,
// e.g. black) or over 1.02 (lighter than the page, e.g. grey) is all off.
function judge(b, f, top, lo = 0) {
  const px = Math.round(Math.sqrt(f.length / 4 / width / 844)), W = width * px;
  const rows = Math.min(Math.floor(Math.max(0, top)) * px, f.length / 4 / W), at = [];
  for (let y = 0; y < rows; y += 3) for (let x = 0; x < W; x += 3) at.push((y * W + x) * 4);
  let fb = 0, bb = 0;
  for (const i of at) for (let c = 0; c < 3; c++) (fb += f[i + c] * b[i + c]), (bb += b[i + c] * b[i + c]);
  if (bb <= 0) return { off: 0, k: 1 };
  const k = fb / bb;
  const off = at.filter((i) => [0, 1, 2].some((c) => Math.abs(f[i + c] - k * b[i + c]) > 12)).length;
  return { off: k < lo - 0.05 || k > 1.02 ? 1 : off / at.length, k };
}

const browser = await webkit.launch();
const ctx = await browser.newContext({ viewport: { width, height: 844 }, bypassCSP: true, serviceWorkers: "block" });
at = "newPage";
const page = await ctx.newPage();
for (const e of ["crash", "dialog", "framenavigated", "pageerror", "requestfailed"])
  page.on(e, (x) => console.error(`${at}: ${e} ${x?.url?.() ?? x?.message?.() ?? x ?? ""}`));
page.on('console', (m) => m.type() === 'log' && console.error('console: ' + m.text()));
const shot = () => page.screenshot({ caret: "initial" });
const run = async (js) => page.evaluate((s) => (0, eval)(s), js);
const body = async (js) => page.evaluate((s) => (0, eval)(`(async () => { ${s} })()`), js);
if (url) {
  await ctx.addInitScript(
    "window.errs = []; addEventListener('error', e => errs.push(String(e.message))); addEventListener('unhandledrejection', e => errs.push(String(e.reason))); const ce = console.error; console.error = (...m) => (errs.push(m.join(' ')), ce(...m))",
  );
  if (env.HERDSMAN_PRE) await ctx.addInitScript(env.HERDSMAN_PRE);
  const [name, ...v] = (env.HERDSMAN_COOKIE ?? "").split("=");
  if (v.length) await ctx.addCookies([{ name, value: v.join("="), url }]);
}
at = "load";
if (env.HERDSMAN_TAB) await ctx.addInitScript(`localStorage.tab = ${JSON.stringify(env.HERDSMAN_TAB)}`);
if (url) {
  await page.goto(url);
  if (env.HERDSMAN_WARM) await sleep(1).then(() => page.goto(url));
} else if (env.HERDSMAN_BASE) {
  const html = readFileSync(page0, "utf8"), base = env.HERDSMAN_BASE;
  await page.route(base, (r) => r.fulfill({ body: html, contentType: "text/html; charset=utf-8" }));
  await page.goto(base);
} else {
  await page.setContent(readFileSync(page0, "utf8"));
}
// #440: a file's page waits for herd.js's theme (its words and sprites), else they land mid-`js` on some runs.
at = "js";
const themed = url ? "" : "typeof themed === 'object' && await themed;";
if (close) {
  await body(`${themed} return 1`);
  await run(`tab('${tab}'); mode('${mode}'); 1`);
  await sleep(0.5);
  const b = rgba(await shot());
  await run(`${extra}; 1`).catch((e) => done(2, `js error: ${e.message}`));
  await sleep(0.7);
  // Above the open sheet: it only slides down from there.
  const top = await run("document.querySelector('.sh').getBoundingClientRect().top").catch(() => 0);
  const lo = judge(b, rgba(await shot()), top).k;
  await run(`${close}; 1`);
  at = "close";
const got = [];
  for (const t0 = Date.now(); Date.now() - t0 < 400; ) got.push(await shot());
  const r = got.map((f) => judge(b, rgba(f), top, lo));
  const worst = r.reduce((m, x, i) => (x.off > r[m].off ? i : m), 0);
  if (png) writeFileSync(png, got[worst]);
  done(0, r.length, r[worst].off, 1 - r.at(-1).k);
}
const js =
  tab === "-"
    ? `${extra}; [document.body.scrollWidth, document.documentElement.clientWidth, document.documentElement.scrollHeight, ...(window.measure || [])]`
    : `tab('${tab}'); mode('${mode}'); ${extra};
${png ? "document.querySelectorAll('section').forEach(s => { const d = s.querySelector('details'); if (d) d.open = true; });" : "document.querySelectorAll('details').forEach(d => d.open = true);"}
[document.body.scrollWidth, document.documentElement.clientWidth, document.documentElement.scrollHeight, ...(window.measure || [])]`;
// #269: on a real server the TypeScript board draws every tab from the API: the tab's first draw lands before `js`.
const drawn = tab === "-" ? "" : url ? `tab('${tab}'); await window.boardReady; await window.boardTab?.('${tab}');` : themed;
await body(`${drawn} return 1`).catch(() => {});
const nums = (v) => Array.isArray(v) && v.every((n) => typeof n === "number" || typeof n === "boolean");
const v = await run(js).catch((e) => done(2, `js error: ${e.message}`));
if (!nums(v)) done(2, `js error: ${JSON.stringify(v)}`);
const wide = [Math.trunc(v[0]), Math.trunc(v[1])];
if (!png) done(0, ...wide, ...v.slice(3).map(Number));
at = "shot";
await page.setViewportSize({ width, height: extra ? 844 : Math.ceil(v[2]) });
await sleep(Number(env.HERDSMAN_WAIT ?? 0.5));
// #315: numbers the js leaves in `window.late` are read after the wait (a sheet's poll has landed) instead.
const m = await run("window.late || null");
console.log([...wide, ...(nums(m) ? m : v.slice(3)).map(Number)].join(" "));
writeFileSync(png, await shot());
done(0);
