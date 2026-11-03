// #244: the colours: every scheme that passes the gate (built-in and `<config dir>/schemes/`; SD from the head script),
// this phone's pick (localStorage) else its theme's default. Applied in place; the block is kept so the head script
// paints it first next time (no flash), and the status bar takes its --bg.
const SQ = new URLSearchParams(location.search).get('scheme');
function colours() {
  const h = document.documentElement, n = choose(SD.s, SQ || get('scheme'), SD.t[h.dataset.theme]), b = block(SD.s[n]);
  h.dataset.scheme = n;
  $('#sch').textContent = b;
  $('meta[name=theme-color]').content = SD.s[n].tokens['--bg'];
  put('sb', b);
  hostrow(document);
  if ($('.sh.look')) lsheet(); // #290: an open look sheet moves its ticks
}
// #244: the Host tab's `theme` row names this phone's theme and colours; under it, each refused scheme with why.
function hostrow(root) {
  const r = [...root.querySelectorAll('.gr')].find(p => p.firstChild.textContent === 'theme'), h = document.documentElement;
  if (!r) return;
  const w = r.querySelector('.w'), b = t => Object.assign(document.createElement('b'), { textContent: t });
  if (r.dataset.t === 'g') w.replaceChildren(b(h.dataset.theme));
  w.querySelectorAll('.sx').forEach(x => x.remove());
  w.append(...[Object.assign(document.createElement('span'), { textContent: ' · colours ' }), b(h.dataset.scheme)].map(x => (x.classList.add('sx'), x)));
  root.querySelectorAll('.gr.sx').forEach(x => x.remove());
  r.after(...Object.entries(SD.s).map(([n, s]) => [n, fails(s)]).filter(([, f]) => f.length).map(([n, f]) => {
    const p = Object.assign(document.createElement('p'), { className: 'gr sx', innerHTML: '<span class=k>colours</span><span class=w><b class=a></b> <span class=mt></span></span>' });
    p.dataset.t = 'a';
    p.querySelector('b').textContent = n;
    p.querySelector('.mt').textContent = `refused: ${f[0]}${f.length > 1 ? ` (+${f.length - 1} more)` : ''}; phones show their theme's default`;
    return p;
  }));
}
// #290: `[◐]` (every tab) opens the look sheet in the board's own sheet, not the system picker: THEME (each theme's 24 px
// impl row sprite, else its favicon) and COLOURS (each passing scheme's bg, fg, green, amber, red), each a listbox. A tap,
// or Enter on the focused row, applies it in place (#206: no reload) and the sheet stays open over a light backdrop, so
// the board behind it shows the change. Picking the theme's default colours forgets the pick: they follow the theme again.
const LS = {};
const lsprite = n => LS[n] || (LS[n] = Promise.all(['theme.json', 'sprites.svg'].map(f => fetch(`${R}themes/${n}/${f}`).then(r => r.ok ? r.text() : '').catch(() => ''))).then(([j, s]) => {
  let t = {};
  try { t = JSON.parse(j); } catch (e) {}
  const id = t.row_sprites && t.row_sprites.impl, g = id && new DOMParser().parseFromString(s, 'image/svg+xml').getElementById(id);
  if (g) { g.removeAttribute('id'); return `<svg viewBox="0 0 132 100">${g.outerHTML}</svg>`; }
  return `<img alt="" src="${R}${t.icons && t.icons.favicon ? `themes/${n}/${t.icons.favicon}` : 'icon-192.png'}">`;
}));
function lrow(k, n, on, art, def) {
  const o = el('div', 'lko');
  o.tabIndex = on ? 0 : -1;
  o.setAttribute('role', 'option');
  o.setAttribute('aria-selected', on);
  o.dataset[k] = n;
  o.append(art, el('span', 'ln', n));
  if (def) o.append(el('small', '', '(theme default)'));
  return o;
}
function lsheet(kb) {
  const h = document.documentElement, re = $('.sh.look'), f = re && document.activeElement.closest('.lko');
  const key = f && Object.entries(f.dataset)[0];
  agent = null;
  sh.className = 'sh lg look';
  $('.hd b').textContent = 'look';
  const grp = (k, title, hint, rows) => {
    const g = el('section', 'cg lkg'), t = el('h3', '', title), l = el('div', 'lkl');
    t.append(el('span', '', hint));
    l.setAttribute('role', 'listbox');
    l.setAttribute('aria-label', k);
    l.append(...rows);
    g.append(t, l);
    return g;
  };
  const th = Object.keys(SD.t).map(n => {
    const s = el('i', 'lka');
    lsprite(n).then(x => { s.innerHTML = x; });
    return lrow('t', n, n === h.dataset.theme, s);
  });
  const sw = s => { const i = el('i', 'lsw'); i.append(...['--bg', '--fg', '--g', '--a', '--r'].map(t => Object.assign(el('b'), { style: `background:${s.tokens[t]}` }))); return i; };
  const ok = Object.keys(SD.s).filter(n => !fails(SD.s[n]).length), no = Object.keys(SD.s).length - ok.length;
  const sc = ok.map(n => lrow('s', n, n === h.dataset.scheme, sw(SD.s[n]), n === choose(SD.s, null, SD.t[h.dataset.theme])));
  $('.top').replaceChildren(grp('theme', 'theme', 'sprites, scene, icon', th), grp('colours', 'colours', `${ok.length} pass the gate${no ? ` · ${no} refused: host` : ''}`, sc),
    el('p', 'lkn', 'this phone only · the home-screen icon keeps its install-time look'));
  if (key) $(`.lko[data-${key[0]}="${CSS.escape(key[1])}"]`).focus();
  else if (!re) {
    show(innerWidth >= 720 ? 'half' : 'mid');
    if (kb) $('.lko[aria-selected=true]').focus({ preventScroll: true }); // opened from the keyboard: on the current theme
  }
}
function lpick(o) {
  const h = document.documentElement;
  if (o.dataset.t) {
    h.dataset.theme = o.dataset.t;
    put('theme', o.dataset.t);
    theme(o.dataset.t);
  } else if (o.dataset.s === choose(SD.s, null, SD.t[h.dataset.theme])) {
    try { localStorage.removeItem('scheme'); } catch (e) {}
  } else put('scheme', o.dataset.s);
  colours();
}
document.addEventListener('click', e => {
  if (e.target.closest('#look')) return lsheet(e.detail === 0);
  const o = e.target.closest('.look .lko');
  if (o) lpick(o);
});
// Keyboard: ↑/↓ (Home/End) move within a list, Tab between the two lists, Enter or Space applies, Esc closes.
document.addEventListener('keydown', e => {
  const o = e.target.closest && e.target.closest('.look .lko');
  if (!o) return;
  if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); return lpick(o); }
  const all = [...o.parentNode.children], i = all.indexOf(o);
  const to = { ArrowDown: i + 1, ArrowUp: i - 1, Home: 0, End: all.length - 1 }[e.key];
  if (to === undefined || !all[to]) return;
  e.preventDefault();
  all.forEach(x => { x.tabIndex = -1; });
  all[to].tabIndex = 0;
  all[to].focus();
});
colours();
