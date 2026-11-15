// #63 gate 2 (docs/ui.md), shipped only in the `serve --export --demo` page: `?audit=<view>` opens one view for
// scripts/ui-audit.sh; `&check=1` then measures it and writes {"view","controls","outliers","sprites"} into <pre id=audit>.
// #927: the views are the screens the page draws: each tab by its place id, `fleets` (the [⚙] screen), `lane` (a lane's
// sheet) and `card` (the first card opened). Controls are 28 px tall with a 44 px tap area; rows are 44 px tap rows.
(() => {
  const q = new URLSearchParams(location.search), V = q.get('audit');
  if (!V) return;
  const $ = s => document.querySelector(s), $$ = s => [...document.querySelectorAll(s)];
  const wait = ms => new Promise(r => setTimeout(r, ms));
  const until = async f => { for (let i = 0; i < 80; i++) { if (f()) return true; await wait(100); } return false; };
  const tab = PL.find(p => p.id === V && p.type === 'header-strip');
  history.replaceState(null, '', location.pathname + location.search + '#' + (tab ? V : PL.find(p => p.type === 'header-strip').id));
  const SETUP = {
    fleets: async () => { $('#sysb').click(); await until(() => $('#sheet .row')); $('#sheet .row').click(); await until(() => $('#sheet .fd')); },
    lane: async () => { location.hash = '#lanes'; await until(() => $('.row[data-id^="l-"]')); $('.row[data-id^="l-"]').click(); await until(() => $('#sheet .sb .sec')); },
    card: async () => { await until(() => $('#cards .fold')); $('#cards .fold .fl').click(); await until(() => $('#cards .choice, #cards .run')); },
  };
  const seen = e => e.checkVisibility({ visibilityProperty: true });
  const label = e => (e.getAttribute('aria-label') || e.textContent).trim().replace(/\s+/g, ' ').slice(0, 24) || e.tagName.toLowerCase();
  function measure() {
    const sheet = !$('#sheet').hidden, roots = sheet ? [$('#sheet .sp')] : [document.body], bad = [];
    const all = sel => roots.flatMap(r => [...r.querySelectorAll(sel)]).filter(seen);
    const ctl = all('button'), rows = all('.row.tap, .fold');
    for (const e of ctl) {
      const cs = getComputedStyle(e), lines = (e.offsetHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom)) / parseFloat(cs.lineHeight);
      if (!e.matches('.pick') && cs.height !== '28px') bad.push(`${label(e)}: control height ${cs.height}`);
      if (e.matches('.pick') && (e.offsetHeight < 28 || lines > 4.05)) bad.push(`${label(e)}: choice ${e.offsetHeight}px, ${lines.toFixed(1)} lines`);
    }
    for (const e of [...ctl, ...rows]) {
      e.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'instant' });
      const r = e.getBoundingClientRect(), cx = r.left + r.width / 2, cy = r.top + r.height / 2;
      for (const [x, y] of [[cx - 16, cy], [cx + 16, cy], [cx, cy - 16], [cx, cy + 16]]) {
        if (x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) continue;
        const h = document.elementFromPoint(x, y);
        if (!h || !e.contains(h)) { bad.push(`${label(e)}: tap area under 44 px, (${x | 0},${y | 0}) hits ${h ? h.outerHTML.slice(0, 40) : 'nothing'}`); break; }
      }
    }
    scrollTo(0, 0);
    for (const r of roots) {
      const w = document.createTreeWalker(r, NodeFilter.SHOW_TEXT);
      for (let t; (t = w.nextNode());) {
        const e = t.parentElement;
        if (!t.data.trim() || !seen(e) || e.closest('svg, script, style')) continue;
        const fs = getComputedStyle(e).fontSize, input = e.closest('input, select, option');
        if (input ? fs !== '16px' : !['11px', '13px'].includes(fs)) bad.push(`"${t.data.trim().slice(0, 20)}": font ${fs}`);
        let b = e;
        while (['inline', 'contents'].includes(getComputedStyle(b).display)) b = b.parentElement;
        const g = document.createRange();
        g.selectNodeContents(t);
        const tr = g.getBoundingClientRect(), br = b.getBoundingClientRect(), cs = getComputedStyle(b);
        if (!input && (tr.right > br.right + 1 || tr.left < br.left - 1) && cs.textOverflow !== 'ellipsis' && !/auto|scroll|hidden/.test(cs.overflowX)) bad.push(`"${t.data.trim().slice(0, 20)}": text overflows its box by ${Math.max(tr.right - br.right, br.left - tr.left) | 0}px`);
      }
      const ring = Object.assign(document.createElement('style'), { textContent: 'button::after{display:none!important}' });
      document.body.append(ring);
      for (const e of [r, ...r.querySelectorAll('*')]) {
        if (e.closest('svg') || !seen(e)) continue;
        const cs = getComputedStyle(e);
        if (cs.display === 'inline' || /auto|scroll/.test(cs.overflowX)) continue;
        if (e.scrollWidth > e.clientWidth + 1 && !(cs.textOverflow === 'ellipsis' && cs.overflowX === 'hidden')) bad.push(`${e.localName}.${e.className || ''} "${label(e)}": ${e.scrollWidth}px of content in ${e.clientWidth}px`);
        if (e.getBoundingClientRect().right > innerWidth + 1) bad.push(`${e.localName} "${label(e)}": right edge ${e.getBoundingClientRect().right | 0} past the screen's ${innerWidth}`);
      }
      ring.remove();
    }
    const se = document.scrollingElement;
    if (se.scrollWidth > se.clientWidth) bad.push(`page scrolls sideways: ${se.scrollWidth}>${se.clientWidth}`);
    return { controls: ctl.length, outliers: bad };
  }
  async function check() {
    const nav = $('nav').getBoundingClientRect().top;
    const sprites = $$('.rsp').filter(seen).map(e => e.getBoundingClientRect()).filter(r => r.top >= 0 && r.bottom <= nav && r.width).map(r => [r.left, r.top, r.width, r.height]);
    let res;
    try { res = measure(); } catch (e) { res = { controls: 0, outliers: ['audit error: ' + e.message] }; }
    for (const g of $$('.rsp').filter(seen)) { const s = g.getBoundingClientRect(); if (Math.round(s.width) !== 24 || Math.round(s.height) !== 24) bad(`row sprite ${s.width}×${s.height}, not 24×24`); }
    function bad(o) { res.outliers.push(o); }
    const W = +(q.get('w') || 390);
    if (innerWidth !== W) bad(`viewport ${innerWidth} px wide, not ${W}`);
    const vp = $('meta[name=viewport]').content, ta = getComputedStyle(document.documentElement).touchAction;
    if (vp !== 'width=device-width,initial-scale=1,maximum-scale=1,user-scalable=no,viewport-fit=cover') bad(`viewport meta "${vp}"`);
    if (ta !== 'manipulation') bad(`html touch-action ${ta}, not manipulation`);
    if (!$('main .scr, #sheet:not([hidden]) .sb > *')) bad('no screen drawn');
    document.body.append(Object.assign(document.createElement('pre'), { id: 'audit', textContent: JSON.stringify({ view: V, sprites, ...res }) }));
  }
  addEventListener('load', async () => {
    await until(() => $('main .scr'));
    await wait(300);
    await (SETUP[V] || (async () => {}))();
    await wait(300);
    if (q.has('check')) await check();
  });
})();
