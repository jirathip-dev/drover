// Mockup only (#1091): `?view=` taps through the real page (web/dist/board.js) to one state, for the PNGs.
// menu: [⚙] open · hosts: the Hosts sheet, both rows tapped open · confirm: boat1's [Off] tapped · off: confirm ok'd.
// POST /action is answered here as serve would: the whole sheet again, boat1 now off.
(() => {
  const v = new URLSearchParams(location.search).get('view') || 'menu';
  const real = window.fetch.bind(window);
  window.fetch = async (u, o) => {
    if (o && o.method === 'POST' && /action$/.test(String(u))) {
      const screen = await (await real('screens/hosts-off/index.html')).json();
      return new Response(JSON.stringify({ ops: [{ op: 'screen', screen }], toast: 'boat1 off' }), { headers: { 'content-type': 'application/json' } });
    }
    if (o && o.method === 'POST') return new Response('{}', { status: 200 });
    return real(u, o);
  };
  const $ = s => document.querySelector(s);
  const until = f => new Promise(ok => { const t = setInterval(() => { const x = f(); if (x) { clearInterval(t); ok(x); } }, 50); });
  const tap = async s => (await until(() => (typeof s === 'string' ? $(s) : s()))).click();
  const row = label => () => [...document.querySelectorAll('#sheet .row')].find(r => r.textContent.includes(label));
  addEventListener('load', async () => {
    await until(() => $('main .scr'));
    await tap('#sysb');
    if (v === 'menu') return;
    await tap(row('Hosts'));
    await until(() => $('#sheet [data-id="h-boat1"]'));
    await tap('#sheet [data-id="h-mini"]');
    await tap('#sheet [data-id="h-boat1"]');
    if (v === 'hosts') return;
    await tap(() => [...document.querySelectorAll('#sheet .ra button')].find(b => b.textContent === 'Off'));
    if (v === 'confirm') return;
    await tap('#confirm .bad');
    await until(() => [...document.querySelectorAll('#sheet .ra button')].find(b => b.textContent === 'On'));
  });
})();
