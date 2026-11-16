// #1091 mockup only, not product code: the Hosts menu row cooee-plugin.toml would declare, and the views the shots open
// (`?v=settings`: the [⚙] sheet; `?v=hosts`: the Hosts sheet, rows' actions open; `?v=confirm`: [off] on boat-1 tapped,
// the export's demo flag dropped so the confirm opens; nothing is posted).
if (new URLSearchParams(location.search).get('v') === 'confirm') { delete document.body.dataset.demo; delete document.body.dataset.ro; }
PL.push({ type: 'menu-row', label: 'Hosts', subtitle: 'on, off and reap per host', screen: '/screens/hosts' });
addEventListener('DOMContentLoaded', () => {
  document.querySelector('body>p.msg.a')?.remove();
  const v = new URLSearchParams(location.search).get('v'), $ = s => document.querySelector(s);
  const until = (s, f) => { const t = setInterval(() => { const e = $(s); if (e) { clearInterval(t); f(e); } }, 100); };
  until('main .tab .scr', () => {
    $('#sysb').click();
    if (v === 'settings') return;
    [...document.querySelectorAll('#sheet .row')].find(r => r.textContent.startsWith('Hosts')).click();
    until('#sheet .row[data-id=host-mini]', () => {
      for (const id of ['host-mini', 'host-air', 'host-boat-1']) $(`#sheet .row[data-id=${id}]`).click();
      if (v === 'confirm') $('#sheet .row[data-id=host-boat-1]').nextElementSibling.querySelector('button').click();
    });
  });
});
