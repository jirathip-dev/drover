import { type Ctx, R, el, kids } from './blocks';
import type { Blk } from './ops';
import { R as base, T, esc, get, put } from './theme';

const RANCH = `<svg viewBox="0 0 390 188" preserveAspectRatio="xMidYMax slice" aria-hidden=true><rect width="390" height="188" fill="#a9cbe0"/><path d="M0 104L60 82L130 100L210 72L290 96L390 78V188H0Z" fill="#8e9c58"/><path d="M0 128L90 112L180 126L270 108L390 124V188H0Z" fill="#6f8048"/><path d="M150 132L180 108L210 132V150H150Z" fill="#a8432f"/>${[...Array(9)].map((_, i) => `<rect x="${i * 48 + 8}" y="134" width="5" height="26" fill="#7a5a3a"/>`).join('')}<rect y="140" width="390" height="4" fill="#9a7a52"/><rect y="160" width="390" height="28" fill="#485e3b"/></svg>`;
const art = new Map<string, string>(), asked = new Map<string, Promise<string>>();
const scene = (t: string) => asked.get(t) ?? asked.set(t, fetch(`${base}themes/${t}/scene.svg`).then(r => (r.ok ? r.text() : '')).catch(() => '').then(s => {
  art.set(t, /^\s*<svg[\s>]/.test(s) ? s : RANCH);
  return art.get(t)!;
})).get(t)!;
let at = get('herd-repo') ?? '', night: boolean | null = get('herd-night') == null ? null : get('herd-night') === '1';
const dark = () => night ?? (h => h < 7 || h >= 19)(new Date().getHours());

R.scene = (b: Blk, c: Ctx) => {
  const repos: Blk[] = b.repos ?? [], i = Math.max(0, repos.findIndex(r => r.id === at)), r = repos[i];
  const s = el('div', `herd${dark() ? ' night' : ''}`), strip = el('div', 'hsc'), t = document.documentElement.dataset.theme ?? 'farm';
  if (T.scene?.weather !== false) s.dataset.sky = b.weather?.sky ?? 'clear';
  if (r?.color) s.style.setProperty('--fk', `var(--${r.color})`);
  if (art.has(t)) strip.innerHTML = art.get(t)!;
  else scene(t).then(svg => { strip.innerHTML = svg; });
  const redraw = () => s.dispatchEvent(new CustomEvent('redraw', { bubbles: true }));
  const btn = (label: string, aria: string, f: () => void) => {
    const x = el('button', 'hbt', `<span>${esc(label)}</span>`);
    x.type = 'button';
    x.setAttribute('aria-label', aria);
    x.onclick = () => { f(); redraw(); };
    return x;
  };
  const go = (k: number) => () => { if (repos.length) put('herd-repo', (at = repos[(i + k + repos.length) % repos.length].id)); };
  const bar = el('div', 'hbar', `<b class=hn>${esc(r?.title ?? 'no repos')}</b><span class="m hs">${esc(r?.subtitle ?? '')}</span>`);
  bar.prepend(btn('‹', 'previous repo', go(-1)));
  bar.append(btn('›', 'next repo', go(1)), btn(dark() ? '☀' : '☾', dark() ? 'day' : 'night', () => put('herd-night', (night = !dark()) ? '1' : '0')));
  s.append(strip, bar, el('p', 'm hw', esc(b.weather?.line ?? '')), ...(r ? kids(r.blocks, c) : [el('p', 'm', 'no lanes here')]));
  return s;
};
