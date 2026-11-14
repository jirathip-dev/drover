// #440: the System sheet's own sections (serve.js opens it, indexes it and draws Hosts & health, Activity and
// Appearance): Agent providers and Notifications & devices from API v1's `host`, Fleets and Conductor from its `queue`.
// #923: the providers are the DeepSeek balance, read-only (lanes sign in with the host's own login); until #404
// Conductor names no defaults.
import type { HostTab, QueueTab } from '../../assets/types/api';
import { age, esc } from './events.ts';
import { form, hidden } from './host.ts';
import { sheet } from './queue.ts';

/** A section's head: its name and a muted summary (`t` turns it amber or red). */
const head = (name: string, meta: string, t = '') => `<h2 class=syh><span class=syt>${esc(name)}</span><small class="sym ${t}">${esc(meta)}</small></h2>`;
/** The config sheet's group box (`.cg`) with its hint. */
const group = (name: string, hint: string, body: string) => `<section class=cg><h3>${esc(name)}<span>${esc(hint)}</span></h3>${body}</section>`;
const dl = (rows: [string, string][]) => `<dl class=kv2>${rows.map(([k, v]) => `<dt>${esc(k)}</dt><dd>${v}</dd>`).join('')}</dl>`;

/** Agent providers: a card per provider (the host card's box), a key per row. */
export function providers(d: HostTab): string {
  const ds = d.deepseek, cards: string[] = [];
  if (ds) {
    const [t, s] = ds.balance == null ? ['a', ds.hint] : [ds.low ? 'a' : 'g', `✓ ${ds.balance}${ds.low ? ' · low' : ''}`];
    cards.push(`<article class=pvc data-p=deepseek><div class=pvch><b>DeepSeek</b><span class=m>API keys</span></div><div class=pvr data-t=${t}><p class=pvh><b class=pvn>balance</b><span class="pvs ${t}">${esc(s)}</span></p></div></article>`);
  }
  const bad = +(ds != null && (ds.balance == null || ds.low));
  const meta = !cards.length ? 'none set' : `${cards.length} provider${cards.length > 1 ? 's' : ''}${bad ? ` · ${bad} need${bad > 1 ? '' : 's'} you` : ' · all ok'}`;
  const body = cards.length ? `<div class=pvg>${cards.join('')}</div>` : '<p class=mt>none set: a DeepSeek key in fleet.toml</p>';
  return `${head('agent providers', meta, bad ? 'a' : '')}${body}<p class=pvf>secrets stay in the main host's keychain · remote lanes get theirs over ssh stdin · a conductor proposes, you tap</p>`;
}

/** Notifications & devices: push (this phone's [enable alerts]), alert devices, paired devices, notifiers. */
export function notifications(d: HostTab): string {
  const ago = (t: number) => age(Math.max(0, d.now - t));
  // #18: the button shows only in the Home Screen app (serve.js); endpoints and keys stay on the Mac.
  const me = d.live ? `<button id=push type=button hidden data-key=${d.key}>Enable alerts</button> <span class=mt>home-screen app only</span>` : '<span class=mt>home-screen app only</span>';
  const push = group('Push', 'web push to the home-screen app', dl([['push', `<b class=${d.push.tone}>${esc(d.push.detail)}</b>`], ['this phone', me]]));
  const subs = d.subs.map(x => `<form method=post action=/push/unsubscribe class=role2><input type=hidden name=id value=${x.id}><span>${esc(x.label)} <span class=mt>alerts on ${ago(x.created)} ago</span></span><button>Remove</button></form>`).join('');
  const devs = d.devices.map(x => `${form(d, ' class=role2')}${hidden('unpair', d.mtime)}<input type=hidden name=hash value=${esc(x.hash)}><span>${esc(x.label)} <span class=m>paired ${ago(x.created)} ago · used ${ago(x.used)} ago</span></span><button>Remove</button></form>`).join('');
  const how = 'tap [pair this phone], then <code>drover serve approve &lt;code&gt;</code> on the main host';
  const pair = `<p class=mt>${d.devices.length ? `to pair another device: open the board there, ${how}` : `no paired device yet: ${how}`}</p>`;
  const notify = d.notify.length ? dl(d.notify.map(n => [n.kind, esc(n.on)])) : '<p class=mt>none: <code>[[notify]]</code> in fleet.toml</p>';
  const body = push
    + group('Alert devices', `${d.subs.length} get alerts`, subs || '<p class=mt>none yet: [enable alerts] in the home-screen app</p>')
    + group('Paired devices', `${d.devices.length} paired`, devs + pair)
    + group('Notifiers', 'global · edit fleet.toml', notify);
  const meta = `push ${d.push.tone === 'g' ? 'on' : 'off'} · ${d.devices.length} device${d.devices.length === 1 ? '' : 's'}`;
  return `${head('notifications & devices', meta)}<div class=cs>${body}</div>`;
}

/** Fleets: a row per fleet in its colour, opening to its [config] content (#162); serve.js draws each pipeline. */
export function fleets(q: QueueTab): string {
  const rows = q.fleets.map(c => {
    const roles = c.config.roles.map(r => r.name).join(' · ');
    return `<details class="sfl k-${c.k}" data-f="${esc(c.name)}"><summary><i class=fq>■</i><b>${esc(c.name)}</b><span class=m>${esc([c.config.integration, roles].filter(Boolean).join(' · '))}</span><i class=cv></i></summary><div class=cs>${sheet(q, c.name)}</div></details>`;
  });
  const file = q.fleets[0]?.config.path;
  return `${head('fleets', `${q.fleets.length} repo${q.fleets.length === 1 ? '' : 's'}${file ? ` · config ${file}` : ''}`)}${q.error != null ? `<p class=r>${esc(q.error)}</p>` : rows.join('')}`;
}

/** Conductor: `[chat]`'s defaults for new threads (#404, unset until it lands), then each fleet's own (#401). */
export function conductor(q: QueueTab): string {
  const line = (c: QueueTab['fleets'][number]) => {
    const k = c.config.conductor;
    if (!k) return 'none';
    return k.default ? `default: impl · ${k.harness}` : ['set', k.harness, k.model].filter(Boolean).join(' · ');
  };
  const per = dl(q.fleets.map(c => [c.name, `${esc(line(c))} <a class=go href="#system/fleets/${encodeURIComponent(c.name)}">[edit]</a>`]));
  const chat = group('Chat defaults', "[chat] · unset → each fleet's conductor", "<p class=rk>a new thread starts with its fleet's conductor · a thread keeps its harness · its composer changes model and effort</p>");
  return `${head('conductor', "unset → each fleet's conductor")}<div class=cs>${chat}${group('Per fleet', 'in Fleets → Conductor', per)}</div>`;
}
