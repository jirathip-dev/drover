// #1072 mockup only, not product code: what the built page does natively. The standalone shell adds the Threads place
// (absent from cooee-plugin.toml); a `form` block's `submit` draws its button; a screen's `focus` hides the cards place.
PL.splice(3, 0, { type: 'header-strip', id: 'threads', label: 'Threads', screen: '/screens/threads' });
addEventListener('DOMContentLoaded', () => {
  const H = document.documentElement, q = new URLSearchParams(location.search);
  document.querySelector('body>p.msg.a')?.remove();
  const SUB = { 'thread-t3': 'send', 'thread-t6': 'send', 'thread-new-acme-web': 'start' };
  new MutationObserver(() => {
    const s = document.querySelector('main .tab:not([hidden]) .scr');
    H.toggleAttribute('data-focus', !!s && s.dataset.id.startsWith('thread-'));
    const f = s && s.querySelector('.form[data-id=say]:not(.say)');
    if (f) {
      f.classList.add('say');
      const b = document.createElement('button');
      b.type = 'button';
      b.innerHTML = `<span>${SUB[s.dataset.id] ?? 'send'}</span>`;
      if (q.get('busy')) b.setAttribute('aria-busy', 'true');
      f.append(b);
    }
  }).observe(document.querySelector('main'), { childList: true, subtree: true });
  const open = q.get('open'), fold = q.get('fold'), swipe = q.get('swipe');
  const tick = setInterval(() => {
    if (!document.querySelector('main .tab:not([hidden]) .scr .list')) return;
    clearInterval(tick);
    for (const f of fold ? fold.split(',') : []) document.querySelector(`main [data-id="${f}"] .fold`)?.click();
    if (swipe) { const r = document.querySelector(`main .row[data-id="${swipe}"]`); r.nextElementSibling.hidden = false; }
    if (open) document.querySelector(`main [data-id="${open}"]`)?.click();
  }, 100);
});
