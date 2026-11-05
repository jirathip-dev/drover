// #462 mockup: apply a bush scheme's tokens to :root, as serve.js's head script does (#423's `{light, dark}` pair).
// `?s=light|dark` pins one (the shots); otherwise the page follows prefers-color-scheme live. The scene's day/night
// (`night` on the Herd) follows the same pick.
const q = new URLSearchParams(location.search).get('s');
const S = Object.fromEntries(await Promise.all(['light', 'dark'].map(async k => [k, await fetch(`schemes/bush-${k}.json`).then(r => r.json())])));
const mq = matchMedia('(prefers-color-scheme: dark)');
export const mode = () => q || (mq.matches ? 'dark' : 'light');
export const scheme = k => S[k];
function apply() {
  const s = S[mode()], st = document.getElementById('sch') || document.head.appendChild(Object.assign(document.createElement('style'), { id: 'sch' }));
  st.textContent = `:root{${Object.entries(s.tokens).map(([k, v]) => `${k}:${v}`).join(';')};color-scheme:${s['color-scheme']}}`;
  document.documentElement.dataset.scheme = 'bush-' + mode();
  document.documentElement.classList.toggle('night', mode() === 'dark');
}
apply();
mq.addEventListener('change', apply);
