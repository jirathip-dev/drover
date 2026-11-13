// #731: the verify lane's browser evidence, for any repo's recipe. No deps: Chrome's --remote-debugging-pipe (fd 3 in,
// fd 4 out, as ui-close.mjs) and a PNG codec over node:zlib.
//   node verify-browser.mjs <url> <out dir> [--name home] [--recipe .drover/verify.md] [--approved <dir>]
//                           [--threshold 0.1] [--trace] [--wait 1000]
// drives <url> at 1440×900 and 390×844 and writes, in <out dir>: <name>-1440.png, <name>-390.png, <name>-console.log
// (errors and warnings), <name>-network.log (failed and 4xx/5xx requests), <name>-a11y.json (the accessibility tree's
// controls with no name), with --trace <name>-trace.json (a Chrome performance trace of the load), and per shot with a
// same-named PNG in --approved, <name>-<w>-diff.png (differing pixels red) and its share.
//   node verify-browser.mjs diff <shot.png> <approved.png> [threshold %]   (an iOS sim-task shot, say)
// Each blocking finding is a `FAIL <line>` on stdout: a console error, a failed request (unless the recipe's
// `## Known noise` bullets name a substring of it), a diff over the threshold (%, default 0.1). Exit 1 on any, 2 on a
// usage or Chrome error. CHROME names the browser (chrome-headless-shell).
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';
import { crc32, deflateSync, inflateSync } from 'node:zlib';

function decode(png) { // 8-bit RGB(A), non-interlaced → {w, h, px: RGB(A) rows, bpp}
  let p = 8, w = 0, h = 0, ct = 0;
  const idat = [];
  while (p < png.length) {
    const n = png.readUInt32BE(p), t = png.toString('ascii', p + 4, p + 8), d = png.subarray(p + 8, p + 8 + n);
    if (t === 'IHDR') { w = d.readUInt32BE(0); h = d.readUInt32BE(4); ct = d[9]; }
    if (t === 'IDAT') idat.push(d);
    p += 12 + n;
  }
  const bpp = ct === 6 ? 4 : 3, raw = inflateSync(Buffer.concat(idat)), row = w * bpp, px = Buffer.alloc(row * h);
  for (let y = 0; y < h; y++) {
    const f = raw[y * (row + 1)], src = raw.subarray(y * (row + 1) + 1, (y + 1) * (row + 1)), o = y * row;
    for (let x = 0; x < row; x++) {
      const a = x >= bpp ? px[o + x - bpp] : 0, b = y ? px[o - row + x] : 0, c = x >= bpp && y ? px[o - row + x - bpp] : 0;
      const pa = Math.abs(b - c), pb = Math.abs(a - c), pc = Math.abs(a + b - 2 * c);
      px[o + x] = src[x] + [0, a, b, (a + b) >> 1, pa <= pb && pa <= pc ? a : pb <= pc ? b : c][f];
    }
  }
  return { w, h, px, bpp };
}

function encode(w, h, rgb) { // RGB, filter 0
  const chunk = (t, d) => {
    const b = Buffer.alloc(12 + d.length);
    b.writeUInt32BE(d.length); b.write(t, 4, 'ascii'); d.copy(b, 8);
    b.writeUInt32BE(crc32(b.subarray(4, 8 + d.length)), 8 + d.length);
    return b;
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 2;
  const raw = Buffer.alloc((w * 3 + 1) * h);
  for (let y = 0; y < h; y++) rgb.copy(raw, y * (w * 3 + 1) + 1, y * w * 3, (y + 1) * w * 3);
  return Buffer.concat([Buffer.from('\x89PNG\r\n\x1a\n', 'latin1'), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
}

// The share (%) of pixels any channel of which is off by more than 8 (pixel-diff.py's rule); the diff PNG beside the
// shot: differing pixels red over the shot dimmed. Another size is 100 %.
function diff(shot, approved, limit) {
  const a = decode(readFileSync(shot)), b = decode(readFileSync(approved)), out = shot.replace(/\.png$/i, '-diff.png');
  let d = 100;
  if (a.w === b.w && a.h === b.h) {
    const rgb = Buffer.alloc(a.w * a.h * 3);
    let n = 0;
    for (let i = 0; i < a.w * a.h; i++) {
      const pa = i * a.bpp, pb = i * b.bpp;
      const off = [0, 1, 2].some(k => Math.abs(a.px[pa + k] - b.px[pb + k]) > 8);
      n += off;
      for (let k = 0; k < 3; k++) rgb[i * 3 + k] = off ? [255, 0, 0][k] : a.px[pa + k] >> 2;
    }
    writeFileSync(out, encode(a.w, a.h, rgb));
    d = (100 * n) / (a.w * a.h);
  }
  const line = `diff: ${shot} vs ${approved}: ${d.toFixed(3)} % of pixels differ${a.w === b.w && a.h === b.h ? ` (${out})` : `: ${a.w}x${a.h} vs ${b.w}x${b.h}`}`;
  return { line, bad: d > limit };
}

const args = process.argv.slice(2), opt = k => { const i = args.indexOf(`--${k}`); return i < 0 ? undefined : args[i + 1] ?? ''; };
const report = fails => { fails.forEach(l => console.log(`FAIL ${l}`)); process.exit(fails.length ? 1 : 0); };
if (args[0] === 'diff') {
  if (args.length < 3) { console.error('usage: verify-browser.mjs diff <shot.png> <approved.png> [threshold %]'); process.exit(2); }
  const r = diff(args[1], args[2], Number(args[3] ?? 0.1));
  console.log(r.line);
  report(r.bad ? [r.line] : []);
}
const [url, out] = args;
if (!url || !out || !process.env.CHROME) { console.error('usage: CHROME=<binary> verify-browser.mjs <url> <out dir> [--name n] [--recipe f] [--approved dir] [--threshold %] [--trace] [--wait ms]'); process.exit(2); }
const name = opt('name') || 'page', limit = Number(opt('threshold') ?? 0.1), settle = Number(opt('wait') ?? 1000);
// `## Known noise`: one substring per bullet, backticks dropped.
const recipe = opt('recipe') && existsSync(opt('recipe')) ? readFileSync(opt('recipe'), 'utf8') : '';
const sect = recipe.split(/^## /m).find(s => /^Known noise\b/i.test(s)) ?? '';
const noise = [...sect.matchAll(/^\s*[-*] +(.+)$/gm)].map(m => m[1].trim().replace(/^`(.*)`$/, '$1')).filter(Boolean);
const quiet = l => noise.some(n => l.includes(n));
mkdirSync(out, { recursive: true });

const prof = mkdtempSync(join(tmpdir(), 'verify-browser-'));
const c = spawn(process.env.CHROME, ['--headless', '--no-sandbox', '--hide-scrollbars', '--remote-debugging-pipe', `--user-data-dir=${prof}`,
  '--no-first-run', '--window-size=1440,900'], { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });
let id = 0, buf = '';
const wait = new Map(), on = [];
c.stdio[4].on('data', d => {
  buf += d;
  for (let i; (i = buf.indexOf('\0')) >= 0; buf = buf.slice(i + 1)) {
    const m = JSON.parse(buf.slice(0, i));
    if (wait.has(m.id)) { wait.get(m.id)(m); wait.delete(m.id); } else on.forEach(f => f(m));
  }
});
const send = (method, params = {}, sessionId) => new Promise((ok, no) => {
  wait.set(++id, m => m.error ? no(new Error(`${method}: ${m.error.message}`)) : ok(m.result));
  c.stdio[3].write(JSON.stringify({ id, method, params, sessionId }) + '\0');
});
const sleep = ms => new Promise(r => setTimeout(r, ms));
const die = msg => { console.error(`verify-browser: ${msg}`); c.kill(); process.exit(2); };
setTimeout(() => die('timed out after 90 s'), 90000).unref();

try {
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId: s } = await send('Target.attachToTarget', { targetId, flatten: true });
  const cmd = (m, p) => send(m, p, s);
  const consoleLog = [], netLog = [], reqs = new Map(), trace = [];
  const text = a => a.map(x => x.value ?? x.description ?? x.type).join(' ');
  on.push(({ method: m, params: p }) => {
    if (m === 'Runtime.consoleAPICalled' && ['error', 'warning', 'assert'].includes(p.type))
      consoleLog.push(`console ${p.type === 'warning' ? 'warning' : 'error'}: ${text(p.args)}`);
    if (m === 'Runtime.exceptionThrown')
      consoleLog.push(`console error: ${p.exceptionDetails.exception?.description ?? p.exceptionDetails.text}`);
    // Network's own log lines (`Failed to load resource`) are the network log's.
    if (m === 'Log.entryAdded' && p.entry.source !== 'network' && ['error', 'warning'].includes(p.entry.level))
      consoleLog.push(`console ${p.entry.level}: ${p.entry.text}${p.entry.url ? ` (${p.entry.url})` : ''}`);
    if (m === 'Network.requestWillBeSent') reqs.set(p.requestId, `${p.request.method} ${p.request.url}`);
    if (m === 'Network.responseReceived' && p.response.status >= 400)
      netLog.push(`network ${p.response.status}: ${reqs.get(p.requestId) ?? p.response.url}`);
    if (m === 'Network.loadingFailed' && !p.canceled)
      netLog.push(`network failed: ${reqs.get(p.requestId)} (${p.errorText})`);
    if (m === 'Tracing.dataCollected') trace.push(...p.value);
  });
  for (const d of ['Runtime', 'Log', 'Network', 'Page']) await cmd(`${d}.enable`);
  if (args.includes('--trace')) await send('Tracing.start', { transferMode: 'ReportEvents', categories: 'devtools.timeline,loading,blink.user_timing' });
  const view = (width, height) => cmd('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: width < 600 });
  await view(1440, 900);
  const loaded = new Promise(r => on.push(m => m.method === 'Page.loadEventFired' && r()));
  await cmd('Page.navigate', { url });
  await loaded;
  await sleep(settle);
  if (args.includes('--trace')) {
    const done = new Promise(r => on.push(m => m.method === 'Tracing.tracingComplete' && r()));
    await send('Tracing.end');
    await done;
    writeFileSync(join(out, `${name}-trace.json`), JSON.stringify({ traceEvents: trace }));
  }
  const shots = [];
  for (const [w, h] of [[1440, 900], [390, 844]]) {
    await view(w, h);
    await sleep(200);
    const f = join(out, `${name}-${w}.png`);
    writeFileSync(f, Buffer.from((await cmd('Page.captureScreenshot', { format: 'png' })).data, 'base64'));
    shots.push(f);
  }
  // Controls, images and fields with no accessible name.
  const { nodes } = await cmd('Accessibility.getFullAXTree');
  const roles = ['button', 'link', 'image', 'img', 'textbox', 'checkbox', 'radio', 'combobox', 'searchbox', 'switch', 'slider', 'menuitem', 'tab'];
  const issues = nodes.filter(n => !n.ignored && roles.includes(n.role?.value) && !String(n.name?.value ?? '').trim())
    .map(n => ({ role: n.role.value, issue: 'no accessible name', node: n.backendDOMNodeId }));
  writeFileSync(join(out, `${name}-a11y.json`), JSON.stringify({ url, nodes: nodes.length, issues }, null, 1) + '\n');
  c.kill();
  const tag = l => (quiet(l) ? `${l} [known noise]` : l);
  writeFileSync(join(out, `${name}-console.log`), consoleLog.map(tag).join('\n') + (consoleLog.length ? '\n' : ''));
  writeFileSync(join(out, `${name}-network.log`), netLog.map(tag).join('\n') + (netLog.length ? '\n' : ''));
  const fails = [...consoleLog.filter(l => l.startsWith('console error') && !quiet(l)), ...netLog.filter(l => !quiet(l))];
  const ap = opt('approved');
  for (const f of shots) {
    const a = ap && join(ap, basename(f));
    if (!a || !existsSync(a)) continue;
    const r = diff(f, a, limit);
    console.log(r.line);
    if (r.bad) fails.push(`${r.line}: over ${limit} %`);
  }
  console.log(`verify-browser: ${url}: ${consoleLog.length} console lines, ${netLog.length} failed requests, ${issues.length} a11y issues; ${shots.join(' ')}`);
  report(fails);
} catch (e) { die(e.message); }
