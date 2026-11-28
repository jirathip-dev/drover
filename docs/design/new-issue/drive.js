// Mockup only (#1783): `?view=` loads one state of the cards place and taps the real page (web/dist/board.js) to it.
// new: [New issue] open, acme-web picked, a line typed, a screenshot attached · scoping: the scope lane's question 2
// open, a second line for acme-web waiting · draft: the draft ask open, with approve, revise (a note) and drop.
(() => {
  const v = new URLSearchParams(location.search).get('view') || 'new';
  const real = window.fetch.bind(window);
  window.fetch = (u, o) => real(/screens\/cards\/index\.html/.test(String(u)) ? `screens/cards-${v}.json` : u, o);
  const $ = s => document.querySelector(s);
  const until = f => new Promise(ok => { const t = setInterval(() => { const x = f(); if (x) { clearInterval(t); ok(x); } }, 50); });
  const open = async id => (await until(() => $(`#cards [data-id="${id}"] > .fold`))).click();
  addEventListener('load', async () => {
    await until(() => $('main .scr'));
    if (v === 'new') {
      await open('new-issue');
      const ta = await until(() => $('#cards .cmp textarea'));
      ta.value = 'the orders table loses its sort when I press Back';
      ta.dispatchEvent(new Event('input'));
      const png = await (await real('orders-shot.png')).blob(), dt = new DataTransfer();
      dt.items.add(new File([png], 'orders.png', { type: 'image/png' }));
      const pick = $('#cards .cmp input[type=file]');
      pick.files = dt.files;
      pick.dispatchEvent(new Event('change'));
      await until(() => $('#cards .cmp .th img'));
    }
    if (v === 'scoping') { await open('ask-s2'); await open('new-issue'); }
    if (v === 'draft') await open('ask-s4');
    document.body.dataset.ready = '1';
  });
})();
