// #269: the [chat] tab (#386, docs/design/conductor-chat/), drawn from API v1's `chat` (pure: tests run it under
// node): per repo its conductor threads, then each thread as #138's chat blocks with choice rows, images and files.
import type { ChatTab, Chip, Link, Msg, ThreadPage, ThreadRow, ThreadView } from '../../assets/types/api';
import { age, esc } from './events.ts'; // node runs this file as is: the extension

const or = (c: boolean, a: string, b: string) => (c ? a : b);
/** `serve::enc`: every byte but an ASCII letter or digit as `%XX`. */
export const enc = (s: string) => [...new TextEncoder().encode(s)].map(b => (/[A-Za-z0-9]/.test(String.fromCharCode(b)) ? String.fromCharCode(b) : `%${b.toString(16).toUpperCase().padStart(2, '0')}`)).join('');

/** #618: the thread ids the tab knows (any repo, archived too), as of its last draw. */
let ids = new Set<string>();
/** #618: in escaped text, each whole-word `t<n>` naming a known thread a ref that opens it (serve.js); others stay text. */
export const refs = (s: string) => s.replace(/(?<![\w/.-])t\d+(?!\w)/g, t => (ids.has(t) ? `<span class=tl data-go="${t}">${t}</span>` : t));

/** Escaped prose: paragraphs at blank lines, `code`, plain links and (#618, outside `code`) thread refs. */
function paras(text: string): string {
  const link = (w: string) => {
    const url = w.replace(/[.,);:]+$/, '');
    return url.startsWith('https://') || url.startsWith('http://') ? `<a href="${url}">${url}</a>${w.slice(url.length)}` : refs(w);
  };
  const para = (p: string) => {
    const c = esc(p.trim()).split('`').map((s, i) => (i % 2 === 1 ? `<code>${s}</code>` : s.split(' ').map(link).join(' '))).join('');
    return `<p>${c.replace(/\n/g, '<br>')}</p>`;
  };
  return text.split('\n\n').filter(p => p.trim()).map(para).join('');
}

/** #409: prose; between fence lines (to the end if unclosed) a box with [copy] of the block's exact text. */
export function prose(text: string): string {
  const seg = [''];
  for (const l of text.split('\n')) {
    if (l.trimStart().startsWith('```')) seg.push('');
    else seg[seg.length - 1] += l + '\n';
  }
  return seg.map((s, i) => (i % 2 === 0 ? paras(s) : `<div class=cb><pre>${esc(s.endsWith('\n') ? s.slice(0, -1) : s)}</pre><button type=button class=cpy>copy</button></div>`)).join('');
}

function size(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1 << 20) return `${Math.floor(n / 1024)} KB`;
  return `${Math.floor(n / 1048576)}.${Math.floor(((n % 1048576) * 10) / 1048576)} MB`;
}

/** `serve::ago`: the age since `t`, none when `t` is unknown (0). */
const ago = (now: number, t: number) => (t > 0 ? age(Math.max(0, now - t)) : '');

/** A block's time: the age, which serve.js turns into the local clock time. */
const tm = (now: number, ts: number) => `<span class=ct data-ts=${ts}>${ago(now, ts)}</span>`;

/** #413: a linked row's chip (`from t5`, `continued in t6`); a tap opens that thread (serve.js). */
const chip = (l: Link) => `<span class=lc data-go="${esc(l.t)}">${l.w} ${esc(l.t)}</span>`;

/** A message block (#138's: `you` in the accent colour, the conductor purple), open. */
function block(now: number, k: string, who: string, pv: string, body: string, ts: number): string {
  return `<details class="blk ${k}" open><summary><span class=k>${who}</span><span class=pv>${esc(pv.split(/\r?\n/)[0] ?? '')}</span>${tm(now, ts)}<i class=cv></i></summary>${body}</details>`;
}

/** A message's files: an image inline at the bubble's width (#406: a tap opens serve.js's viewer), any other file a
 * row that opens `thread/<t>/<name>`. */
function files(id: string, m: Msg): string {
  return m.files.map(f => {
    const url = `thread/${id}/${enc(f.name)}`, n = size(f.size);
    if (['png', 'jpg', 'jpeg', 'gif', 'webp'].includes(f.name.split('.').pop()!.toLowerCase())) return `<figure class=im><img src="${url}" alt="${esc(f.name)}" loading=lazy><figcaption>${esc(f.name)} · ${n}</figcaption></figure>`;
    return `<a class=tf href="${url}"><span>▤ ${esc(f.name)}</span><span class=m>${n}</span></a>`;
  }).join('');
}

/** An ask's choice rows (recommended first; #492: a handoff card's past 5 behind `more`) and an "other" box. */
function choices(a: NonNullable<Msg['ask']>, dis: string): string {
  const rows = a.options.map((o, i) => [o, i + 1] as const).sort((x, y) => +(x[1] !== a.recommend) - +(y[1] !== a.recommend));
  const btn = ([o, n]: readonly [string, number]) => `<button type=button name=choice value=${n}${dis}><span>${n}. ${refs(esc(o))}${or(n === a.recommend, ' <small>(recommended)</small>', '')}</span></button>`;
  return `<form class=chq data-id="${esc(a.id)}" data-fleet="${esc(a.fleet)}">${rows.slice(0, 5).map(btn).join('')}${or(rows.length > 5, `<details class=more><summary>more (${rows.length - 5})</summary>${rows.slice(5).map(btn).join('')}</details>`, '')}<div class=oth><input name=text aria-label="other answer" placeholder="other: type an answer"${dis}><button${dis}>send</button></div></form>`;
}

/** An ask: its question, then its choice rows; answered, the ✓ line. */
function ask(now: number, m: Msg, dis: string): string {
  const a = m.ask!;
  let body = `<div class=md><p class=qid>${esc(a.id)}${or(m.routine, ' · routine', '')}</p>${prose(a.question)}</div>`;
  if (m.answer) {
    const d = m.answer, what = d.n == null ? `other: "${esc(d.choice)}"` : `${d.n}. ${esc(d.choice)}`;
    const who = d.by === 'default' ? 'by: default' : d.by == null || d.by === 'owner' ? 'you' : esc(d.by);
    body += `<p class=ans>✓ ${what} <span class=m>· ${who} · ${tm(now, d.ts)}</span></p>`;
  } else if (m.open) body += choices(a, dis);
  return block(now, 'ai', 'conductor', a.question, body, a.ts);
}

const cmd = (c: string) => `<details class="blk run"><summary><span class=k>$</span><span class=cmd>${esc(c)}</span><i class=cv></i></summary></details>`;

/** One message's HTML, tagged `data-n` (its event's seq) after its tag name. */
function msg(now: number, id: string, m: Msg, rw: boolean): string {
  const dis = or(rw, '', ' disabled'), lk = m.link ? chip(m.link) + ' ' : '';
  let b: string;
  switch (m.kind) {
    case 'note': {
      const r = m.retry ? ' r' : '', rt = m.retry ? ` <button type=button class=rty data-n=${m.seq}${dis}>retry</button>` : '';
      const si = m.signin == null ? '' : ` <button type=button data-su="${esc(m.signin)}" data-host="${esc(m.host)}"${dis}>sign in</button>`;
      const ch = m.ask && m.open ? `<details class=chg><summary>change</summary>${choices(m.ask, dis)}</details>` : ''; // #541: a conductor's answer, 30 min
      b = `<p class="qs note${r}">${lk}${refs(esc(m.text))}${si}${rt}</p>${ch}`;
      break;
    }
    case 'handoff': b = `<p class=qs>${lk}· ${refs(esc(m.text))}</p>`; break;
    case 'msg': {
      const [k, who] = m.from === 'conductor' ? ['ai', 'conductor'] : lk ? ['ai', lk.trimEnd()] : ['me', 'you'];
      b = block(now, k, who, m.text, `<div class=md data-md="${esc(m.text)}">${prose(m.text)}</div>${files(id, m)}`, m.ts); // #407: [copy as markdown]
      break;
    }
    case 'cmd': b = cmd(m.text); break;
    case 'fold': b = `<details class="blk fold"><summary><span class=pv>${m.cmds.length} command${or(m.cmds.length === 1, '', 's')}</span></summary>${m.cmds.map(cmd).join('')}</details>`; break; // #407
    case 'ask': b = ask(now, m, dis); break;
    case 'config': {
      const q = esc(m.from), btn = (v: string) => `<button name=act value=${v}${dis}>${v}</button>`;
      const act = m.open ? `<form class=chc data-id="${q}">${btn('apply')}${btn('discard')}</form>` : '<p class=m>stale: fleet.toml changed since</p>'; // #460: stale, it can no longer apply
      b = block(now, 'ai', 'conductor', `proposes config ${m.from}`, `<div class=md><p class=qid>config ${q}</p><pre class=df>${esc(m.text)}</pre></div>${act}`, m.ts);
      break;
    }
    case 'secret': {
      const n = esc(m.from);
      const f = rw ? `<form class=chs data-t="${id}" data-name="${n}"><input type=password name=value autocomplete=off aria-label="value of ${n}" placeholder="paste the value"><button name=act value=save>save</button><button name=act value=dismiss>dismiss</button></form>` : '<p class=m>only a paired owner device can enter it</p>';
      b = block(now, 'ai', 'conductor', `asks for secret ${m.from}`, `<div class=md><p class=qid>secret ${n} · stored in the keychain, never in the chat</p>${prose(m.text)}</div>${f}`, m.ts);
      break;
    }
    case 'run': { // #669: a run card, its own row: [approve]/[deny] while open, then its outcome
      const c = m.run!, q = esc(c.id), btn = (v: string) => `<button name=act value=${v}${dis}>${v}</button>`, who = esc(c.by ?? 'owner');
      const out: Record<string, string> = {
        open: `<form class=chr data-id="${q}" data-sha="${esc(c.sha.slice(0, 12))}">${btn('approve')}${btn('deny')}</form><p class=m>expires in ${age(Math.max(0, c.exp - now))}</p>`,
        running: `<p class=ans>✓ approved by ${who} · running…</p>`,
        approved: `<p class=ans>✓ approved by ${who} · exit ${c.code ?? '?'}</p><div class=cb><pre>${esc(c.tail)}</pre><button type=button class=cpy>copy</button></div>`,
        denied: `<p class=ans>✗ denied by ${who} · nothing ran</p>`,
        expired: '<p class=m>expired · nothing ran; a new run needs a new card</p>',
      };
      b = block(now, 'ai', 'conductor', `run card ${c.id}`, `<div class=md><p class=qid>run card ${q} · sha ${esc(c.sha.slice(0, 12))}</p><pre class=rc>${esc(c.cmd)}</pre><p>why: ${esc(c.why)}</p><p>touches: ${esc(c.blast)}</p><p>runs on: ${esc(c.host)} host, ${esc(c.dir)}</p></div>${out[c.state] ?? ''}`, m.ts);
      break;
    }
    case 'resumed': b = `<p class=qs>↻ resumed · same session (${[...esc(m.text)].slice(0, 4).join('')}…)</p>`; break;
    case 'sleep': b = '<p class=qs>☾ asleep after 30 min idle · writes wake it</p>'; break;
    default: return '';
  }
  const i = b.indexOf(' ');
  return `${b.slice(0, i)} data-n=${m.seq}${b.slice(i)}`;
}

/** #408: a page of a thread's messages under `thread started …` or, older ones left, the marker serve.js loads them
 * from (`older`, API v1 `thread/<t>?before=`). */
export function page(now: number, id: string, p: ThreadPage, rw: boolean): string {
  const top = p.older != null ? `<p class="qs tmo" data-b=${p.older}>↑ older messages</p>` : `<p class=qs>thread started by owner · ${tm(now, p.started)} · conductor in ${esc(p.dir)}</p>`;
  return top + p.msgs.map(m => msg(now, id, m, rw)).join('');
}

/** #404: a chip, a select of its choices (serve.js shows it as `[opus ▾]`); unset, `<name>: default` first. */
function chip1(c: Chip, dis: string): string {
  const opt = (v: string, l: string) => `<option value="${esc(v)}"${or((c.cur ?? '') === v, ' selected', '')}>${esc(l)}</option>`;
  return `<select class=cp name=${c.name} aria-label=${c.name}${dis}>${c.cur == null ? opt('', `${c.name}: default`) : ''}${c.all.map(v => opt(v, v)).join('')}</select>`;
}

/** The composer: [+] attaches files (#405: several, up to 10), the box, [send]; #407: the queued messages above;
 * #404: its chips below. */
const composer = (id: string, repo: string, qd: string, dis: string, chips: Chip[] = []) => `<form class=tw data-t="${id}"${repo}>${qd}<input type=file multiple hidden${dis}><button type=button class=add aria-label="attach images or files"${dis}>+</button><textarea name=text rows=1 aria-label=message placeholder="message the conductor"${dis}></textarea><button class=snd${dis}>send</button>${chips.length ? `<div class=cbar>${chips.map(c => chip1(c, dis)).join('')}</div>` : ''}</form>`;

/** #407: the queued messages above the box, each with [send now] (steer it in), edit and [remove]. */
function queue(t: ThreadView, dis: string): string {
  const b = (n: string, l: string) => `<button type=button name=${n}${dis}>${l}</button>`;
  return t.queued.map(q => `<div class=tq data-q=${q.q}><p title="${esc(q.text)}">${esc(q.text)}</p><span class=btns>${b('qn', 'send now')}${b('qe', 'edit')}${b('qx', 'remove')}</span></div>`).join('');
}

const LIVE: Record<string, string> = {
  choose: '<b>●</b> waiting for your answer · sleeps after 30 min idle',
  working: '<i class=tld aria-hidden=true></i> working', // #403: serve.js puts the theme's loading art in
  asleep: '☾ asleep · a message wakes it with this conversation',
  new: 'a message starts the conductor',
};

/** #460: `config c1 c2 waiting` while proposals wait for the owner's tap. */
const cw = (w: string[]) => (w.length ? `<b class="a cw">config ${esc(w.join(' '))} waiting</b>` : '');

/** One thread: the bar (#618: its id after the title), its last messages, the live line and the composer (inert off a
 * paired owner device; #618: and while archived, the line its [restore]). */
function thread(d: ChatTab, t: ThreadView): string {
  const dis = or(d.rw, '', ' disabled'), dot = or(['asleep', 'new'].includes(t.state), '', '<i class=dt></i>');
  const guest = or(d.rw, '', '<p class="ns m">pair this phone to answer or write</p>'), ro = t.page.archived;
  const live = ro ? `<p class=tlv>archived · read-only <button type=button class=rst data-t="${t.id}"${dis}>restore</button></p>` : `<p class=${t.since == null ? 'tlv' : `"tlv wk" data-since=${t.since}`}>${LIVE[t.state] ?? '<b class=g>●</b> awake · sleeps after 30 min idle'}</p>`; // #407: working, serve.js floats the line as a pill
  return `<div class=cvp data-t="${t.id}"><div class=th><button type=button class=bk aria-label="back to threads">‹</button><div><div class=tn><b>${esc(t.title)}</b><small class=tid>· ${t.id}</small></div><span class=ds>${dot}${esc(t.ds)}</span>${cw(t.waiting)}</div></div>`
    + `<div class=tc>${page(d.now, t.id, t.page, d.rw)}</div>${live}${guest}${composer(t.id, '', queue(t, dis), or(ro, ' disabled', dis), t.chips)}</div>`; // #407: working, serve.js floats the line as a pill
}

/** #405: [+ thread]'s empty thread `new-<repo>`: nothing is logged until its first message starts it. */
function draft(repo: string, dis: string, h: Chip): string {
  const r = esc(repo);
  return `<div class=cvp data-t="new-${r}"><div class=th><button type=button class=bk aria-label="back to threads">‹</button><div><b>new thread</b><span class=ds>${or(repo === 'all', 'all repos', r)}</span></div></div>`
    + `<div class=tc></div><p class=tlv>your first message starts it; its first line is the title</p>${composer(`new-${r}`, ` data-repo="${r}"`, '', dis, [h])}</div>`;
}

const ST: Record<string, string> = { choose: '<b class=r>! choose</b>', working: '<b class=a>✻ working</b>', asleep: '<b class=m>☾ asleep</b>' };

/** A thread's row: unread dot, title, age; then its links, state (unless plain awake) and the last message; #405: a
 * row swiped left shows [archive], an archived one [restore]. */
function row(r: ThreadRow, b: string, dis: string): string {
  const lk = r.links.map(l => chip(l) + ' ').join('');
  const a = `<a class="tr${or(r.unread, '', ' rd')}" href=#chat data-t="${r.id}"><i class=ud>${or(r.unread, '●', '')}</i><span class=tt><span>${esc(r.title)}</span><small class=tid>· ${r.id}</small></span><span class=ta>${r.age}</span><span class=tp>${lk}${cw(r.waiting)}${ST[r.state] ?? ''}${esc(r.line)}</span></a>`;
  return `<div class=sw>${a}<button type=button class=sb name=${b} data-t="${r.id}"${dis}>${b}</button></div>`;
}

/** The tab: "all repos", then each fleet by name, a card each with its 3 newest threads (the rest fold) and
 * [+ thread]; then every thread's view (serve.js shows the open one). */
export function tab(d: ChatTab): string {
  if (d.error != null) return `<p class=r>${esc(d.error)}</p>`;
  ids = new Set(d.threads.map(t => t.id));
  const dis = or(d.rw, '', ' disabled');
  let list = '', views = '';
  for (const c of d.repos) {
    const n = c.rows.length, rows = c.rows.map(r => row(r, 'archive', dis)), r = esc(c.repo);
    const count = n === 0 ? 'no threads' : `${n} thread${or(n === 1, '', 's')}`;
    const older = n > 3 ? `<details class=done><summary><span>${n - 3} older thread${or(n === 4, '', 's')}</span><i class=cv></i></summary>${rows.slice(3).join('')}</details>` : '';
    const arch = c.archived.length ? `<details class="done arc"><summary><span>${c.archived.length} archived</span><i class=cv></i></summary>${c.archived.map(x => row(x, 'restore', dis)).join('')}</details>` : '';
    const add = d.rw ? `<span class=btns><button type=button class=ntb data-repo="${r}">+ thread</button></span>` : '';
    list += `<div class="fc k-${c.k}"><div class=fh><span class=fn><b>${esc(or(c.repo === 'all', 'all repos', c.repo))}</b><span class=ds>${count}</span></span>${add}</div>${rows.slice(0, 3).join('')}${older}${arch}</div>`;
    views += draft(c.repo, dis, c.harness);
  }
  return `<div class=clp>${list}</div>${views}${d.threads.map(t => thread(d, t)).join('')}`;
}
