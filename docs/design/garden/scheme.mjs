// #423 mockup: apply a garden scheme's tokens to :root, as serve.js's head script does. `?s=light|dark` picks one (the
// shots); otherwise the page follows prefers-color-scheme live, the proposed `"scheme": {"light", "dark"}`.
const q = new URLSearchParams(location.search).get('s');
const S = Object.fromEntries(await Promise.all(['light', 'dark'].map(async k => [k, await fetch(`schemes/garden-${k}.json`).then(r => r.json())])));
const mq = matchMedia('(prefers-color-scheme: dark)');
export const mode = () => q || (mq.matches ? 'dark' : 'light');
export const scheme = k => S[k];
function apply() {
  const s = S[mode()], st = document.getElementById('sch') || document.head.appendChild(Object.assign(document.createElement('style'), { id: 'sch' }));
  st.textContent = `:root{${Object.entries(s.tokens).map(([k, v]) => `${k}:${v}`).join(';')};color-scheme:${s['color-scheme']}}`;
  document.documentElement.dataset.scheme = 'garden-' + mode();
}
apply();
mq.addEventListener('change', apply);
