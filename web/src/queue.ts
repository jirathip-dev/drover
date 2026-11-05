// #268: the Queue tab (#162), drawn from API v1's `queue` (pure: tests/queue_board.rs runs it under node). A card per
// fleet, needing you first: its drive line, rows (stage chips, lane, PR line, notes, [change], ▲/▼, the lanes list),
// done today, the backlog; and each fleet's [config] sheet. serve.js then groups, graphs and folds the rows.
import type { Ask, ConfigSheet, Drive, PrLine, QueueCard, QueueLane, QueueRow, QueueTab, RoleRow } from '../../assets/types/api';
import { esc } from './events.ts'; // node runs this file as is (tests/queue_board.rs): the extension

/** A form: a live board's posts; an export's are inert. */
export const form = (d: { live: boolean }, rest = '') => `<form ${d.live ? 'method=post' : 'inert'}${rest}>`;
export const hidden = (op: string, fleet: string, mt: string) => `<input type=hidden name=op value=${op}><input type=hidden name=fleet value="${esc(fleet)}"><input type=hidden name=mtime value=${mt}>`;
const none = '<span class=m>none</span>', all = '<span class="tag m">all fleets</span>';
const list = (v: string[], sep = ', ') => (v.length ? esc(v.join(sep)) : none);

/** Ask `a`'s answer form: option buttons (recommended first) and a free-text box. */
export function answer(d: { live: boolean; mtime: string }, a: Ask, send: string): string {
  const opts = a.options.map((o, i) => [o, i + 1] as const).sort((x, y) => +(x[1] !== a.recommend) - +(y[1] !== a.recommend));
  const btns = opts.map(([o, n]) => `<button name=choice value=${n}>${n}. ${esc(o)}${n === a.recommend ? ' <small>(recommended)</small>' : ''}</button>`).join('');
  return `${form(d)}${hidden('answer', a.fleet, d.mtime)}<input type=hidden name=id value="${esc(a.id)}"><input name=text aria-label=answer placeholder="or type an answer"><button>${send}</button>${btns}</form>`;
}

/** #166: the drive line; a tap lists the meta lines, then each action in words, a tap on one its raw code. */
function drive(v: Drive): string {
  const acts = v.acts.map(a => `<li><details><summary>${esc(a.who)}: ${esc(a.words)}</summary><code>${esc(a.code)}</code></details></li>`);
  const items = !acts.length && v.pass ? '<li>nothing was due</li>' : acts.join('');
  return `<details class="dp ${v.tone}"><summary><i>●</i><span>${esc(v.line)}</span><i class=cv></i></summary><ul>${v.meta.map(m => `<li>${esc(m)}</li>`).join('')}${items}</ul></details>`;
}

/** #169: `[issue] [PR #11] [checks] · CI ✓ · rev PASS r1`; #269: no `repo` (a lane's fleet not configured), no links. */
export function prLine(p: PrLine, repo: string | null | undefined, n: number): string {
  const gh = `https://github.com/${esc(repo ?? '')}`, a = (u: string, t: string) => `[<a href="${u}">${t}</a>]`;
  const l = [a(`${gh}/issues/${n}`, 'issue')];
  if (p.pr != null) l.push(a(`${gh}/pull/${p.pr}`, `PR #${p.pr}`), a(`${gh}/pull/${p.pr}/checks`, 'checks'));
  else if (p.run != null) l.push(a(esc(p.run), 'checks'));
  const parts = repo == null ? [] : [l.join(' ')];
  if (p.ci == null) parts.push('no PR read yet');
  else parts.push(...(repo == null ? [`PR #${p.pr ?? 0}`] : []), `CI <span class=${p.ci === 'failed' ? 'r' : p.ci === 'green' ? 'g' : 'a'}>${p.ci === 'green' ? '✓' : p.ci === 'failed' ? '✗' : '…'}</span>`);
  if (p.verdict != null) {
    parts.push(`rev <span class=${p.vtone}>${esc(p.verdict)}</span> r${p.rounds}`);
    if (p.form != null) parts.push(`<span class=a>not counted · form ${p.form}</span>`);
  }
  return parts.join(' · ');
}

/** #205: a lane opens its Lanes sheet (`.ql`); #218: a gone one its stored transcript (`data-gone`), else muted text. */
function link(name: string, how: string, text: string): string {
  if (!how) return `<span class="ql nw el m">${text}</span>`;
  return `<a href=#lanes class="ql nw el" data-agent="${esc(name)}"${how === 'gone' ? ' data-gone' : ''}>${text}</a>`;
}
const qlane = (l: QueueLane) => link(l.name, l.link, `<span class=${l.tone}>${l.state === 'working' ? '●' : '○'}</span> <span class=qn>${esc(l.name)}</span> <span class=qa>${esc(l.state)} ${l.ago}</span>`);

/** A row: `#N`, state, age (counting from `data-t`), title; open: its card lines, then the lanes newest first. */
function row(d: QueueTab, c: QueueCard, r: QueueRow): string {
  const n = r.issue;
  // #173: `○ ─ ▲ impl ─ ◆ rev ─ ◫ gates ─ ✓ merge`, the current stage lit.
  const chip = (s: QueueRow['stage'][number]) => `<span class="${s.at < 0 ? 'pt dn' : s.at > 0 ? 'pt' : `pt on k${s.k}`}"><i class="kd ${s.k}">${s.g}</i>${esc(s.name)}${s.ok ? '<b class=g>✓</b>' : ''}</span>`;
  const ends = ['<span class="pt dn"><i class=kd>○</i></span>', '<span class=pt><i class="kd m">✓</i>merge</span>'];
  const stage = r.stage.length ? `<span class=ps>${[ends[0], ...r.stage.map(chip), ends[1]].join('<i class=sx>─</i>')}</span>` : '';
  const lines = [stage, r.live ? link(r.lane, 'live', esc(r.lane)) : esc(r.lane), prLine(r.pr, c.config.repo, n)];
  if (r.dead) lines.push('<span class=r>dead: the log has this lane open, herdr has no such agent</span>');
  if (r.blocked_by != null) lines.push(`<span class=r>blocked by #${r.blocked_by}</span>`);
  if (r.check) lines.push('<span class=m>needs-check: from the backlog, spec not checked</span>');
  lines.push(...r.notes.map(x => `<span class=r>${esc(x)}</span>`));
  if (r.decided != null) lines.push(`<span>decided by default: ${esc(r.decided)}</span>${r.ask ? answer(d, r.ask, 'change') : ''}`);
  if (r.mv) {
    const mv = (label: string, key: string, to: number) => `${form(d)}${hidden('mv', c.name, d.mtime)}<input type=hidden name=issue value=${n}><input type=hidden name=${key} value=${to}><button>${label}</button></form>`;
    lines.push(`<span class=btns>${r.up == null ? '' : mv('▲ up', 'before', r.up)}${r.down == null ? '' : mv('▼ down', 'after', r.down)}</span>`);
  }
  const card = lines.filter(Boolean).map(l => `<p>${l}</p>`).join('');
  const ls = r.lanes.map(qlane), more = ls.length > 5 ? `<details class=qm><summary>+${ls.length - 5} older</summary>${ls.slice(5).join('')}</details>` : '';
  const qls = ls.length ? `<div class=qls>${ls.slice(0, 5).join('')}${more}</div>` : '';
  const data = `${r.queued ? ' data-q' : ''}${r.check ? ' data-ck' : ''}${r.blocked_by == null ? '' : ` data-b=${r.blocked_by}`}${r.groups.length ? ` data-s="${esc(r.groups.join(' '))}"` : ''} data-act=${r.act}${r.coat ? ` style="${r.coat}"` : ''}`;
  const title = r.title == null ? '' : `<span class=tl title="${esc(r.title)}">${esc(r.title)}</span>`;
  return `<details class="q"${data}><summary><span><a href="https://github.com/${esc(c.config.repo)}/issues/${n}">#${n}</a></span><span class=${r.tone}>${esc(r.state)}</span><span class=m data-t=${r.t}>${esc(r.age)}</span>${title}</summary>${card}${qls}</details>`;
}

/** #162: a fleet's card in its colour; a paused fleet is one line. */
function card(d: QueueTab, c: QueueCard): string {
  const fe = esc(c.name), sp = esc(c.paths.join(' | '));
  const sa = sp ? ` data-s="${esc(c.serial.join(' '))}" data-sp="${sp}"` : '';
  const button = (op: string, label: string) => (d.rw ? `${form(d)}${hidden(op, c.name, d.mtime)}<button>${label}</button></form>` : '');
  const need = c.need ? `<span class="tag r">! ${c.need} need you</span>` : '';
  let body = c.rows.map(r => row(d, c, r)).join('');
  if (!c.rows.length) body += '<p class=m>no open work</p>';
  if (c.done_n) {
    const pr = (p: string | null | undefined) => (p == null ? '' : ` <a href="https://github.com/${esc(c.config.repo)}/pull/${esc(p)}">PR #${esc(p)}</a>`);
    const rows = c.done.map(x => `<p>#${x.issue} <span class=g>${esc(x.note)}</span> ${x.age}${pr(x.pr)}</p>`).join('');
    body += `<details class=done><summary><i class=cv></i><span>done today: ${c.done_n}</span></summary>${rows}</details>`;
  }
  // #365: serve.js draws, filters and ticks (`bqueue`) the backlog.
  const b = c.backlog;
  if (b) body += `<details class=bl data-bl="${esc(JSON.stringify(b.issues))}"><summary><i class=cv></i><span>backlog</span><span class=m></span></summary><div class=bls></div><p class=m>read ${b.ago} ago · [Drive now] reads it again</p>${form(d, ' class=bbar hidden')}${hidden('bqueue', c.name, d.mtime)}<input type=hidden name=issues><button>queue</button></form></details>`;
  if (c.paused != null) return `<details class="fc off k-${c.k}"${sa}><summary><i class=cv></i><span class=fn><b>${fe}</b> <i class=m>●</i> ${esc(c.paused)}</span>${need}${button('resume', 'Resume')}</summary>${body}</details>`;
  const bad = c.notes.map(x => `<p class=r>${esc(x)}</p>`).join('');
  return `<div class="fc k-${c.k}"${sa}><div class=fh><span class=fn><b>${fe}</b>${need}</span><span class=btns>${button('drive', 'Drive now')}${button('pause', 'Pause')}<button type=button class=cfg data-f="${fe}">Config</button></span></div>${drive(c.drive)}${bad}${body}</div>`;
}

/** The tab's HTML. */
export function tab(d: QueueTab): string {
  return d.error != null ? `<p class=r>${esc(d.error)}</p>` : d.fleets.map(c => card(d, c)).join('');
}

/** A role's pickers (`rw`): harness, model (disabled while `models` is unset), effort, through the `set` op; else text. */
function role(d: QueueTab, c: QueueCard, r: RoleRow): string {
  const g = c.config, names = (v: string[]) => (v.length ? esc(v.join(', ')) : 'none');
  const head = `<p class=rh><b>${esc(r.name)}</b><span class=m>${esc(r.profile)}</span></p>`;
  const rest = `<p class=rk>accounts ${names(r.accounts)} · mcp ${names(r.mcp)}</p>`;
  if (!d.rw) return `<div class=role>${head}<p>${esc([r.harness, r.model ?? '', r.effort ?? ''].filter(Boolean).join(' · '))}</p>${rest}</div>`;
  const select = (name: string, cur: string | null | undefined, def: string, opts: string[], attr: string) => {
    const first = cur == null || !opts.includes(cur) ? `<option value="" selected>${esc(cur ?? `default (${def})`)}</option>` : '';
    return `<select name=${name} aria-label=${name}${attr}>${first}${opts.map(o => `<option${cur === o ? ' selected' : ''}>${esc(o)}</option>`).join('')}</select>`;
  };
  // No `models`: the picker shows the current model, disabled (`set` refuses any other), with the fix.
  const [off, hint] = g.models.length ? ['', ''] : [' disabled data-off', '<p class=rk>add models = [...] to fleet.toml</p>'];
  // The effort list follows the harness (serve.js swaps it from data-e).
  const harness = g.harnesses.map(h => `<option${h.name === r.harness ? ' selected' : ''} data-e="${h.efforts.join(' ')}">${h.name}</option>`).join('');
  return `${form(d, ' class=role')}${hidden('set', c.name, d.mtime)}<input type=hidden name=role value=${esc(r.name)}>${head}<span class=rp><select name=harness aria-label=harness>${harness}</select>${select('model', r.model, r.model_default, g.models, off)}${select('effort', r.effort, r.effort_default, r.efforts, '')}<button>Set</button></span>${hint}${rest}</form>`;
}

/** #162: a fleet's [config] sheet: GENERAL, Pipeline (serve.js draws `data-pipe`), LIMITS, ROLES, Conductor, ALERTS. */
export function sheet(d: QueueTab, fleet: string): string | null {
  const c = d.fleets.find(x => x.name === fleet);
  if (!c) return null;
  const g: ConfigSheet = c.config, edit = `edit ${esc(g.file)}`;
  const group = (name: string, hint: string, rows: [string, string][]) => `<section class=cg><h3>${name}<span>${hint}</span></h3><dl class=kv2>${rows.map(([k, v]) => `<dt>${k}</dt><dd>${v}</dd>`).join('')}</dl></section>`;
  const cap = (n: number | null | undefined) => (n == null ? '<span class=m>no cap</span>' : `${n}`);
  const rounds = (n: number, on: string) => (g.builtin ? `${n} <span class=m>→ ${on}, see Pipeline</span>${all}` : `<span class=m>n/a (pipeline)</span>${all}`);
  const general = group('General', edit, [
    ['repo', esc(g.repo)],
    ['local', esc(g.local)],
    ['integration', esc(g.integration)],
    ['driver', 'herdsman <span class=m>(herdsman drive)</span>'],
    ['default effort', `${g.effort == null ? '<span class=m>unset → none</span>' : esc(g.effort)}${all}`],
    ['colour', `<span class=${c.k}>■</span> --${c.k} <span class=m>(${g.color_set ? 'set' : 'auto'})</span>`],
    ['env', list(g.env)],
    ['node_modules', g.node_modules ? 'cloned' : '<span class=m>not cloned</span>'],
  ]);
  const limits = group('Limits', edit, [
    ['review rounds', rounds(g.review_rounds, "rev's rounds")],
    ['update rounds', rounds(g.update_rounds, "gates' rounds")],
    ['stall after', `${g.stall_minutes} min${all}`],
    ['ask default', `${g.ask_minutes} min${all}`],
    ['max lanes', `impl ${cap(g.max_impl)} · rev ${cap(g.max_rev)}`],
    ['line cap', g.line_cap == null ? '<span class=m>none in ci/cap.toml</span>' : `${g.line_cap} <span class=m>(ci/cap.toml in the repo)</span>`],
    ['serial', list(g.serial)],
    ['deny commands', list(g.deny)],
    ['required checks', g.required.length ? list(g.required) : '<span class=m>all</span>'],
    ['merge label', g.merge_label == null ? none : esc(g.merge_label)],
  ]);
  const alerts = group('Alerts', edit, [['notify', g.notify.length ? list(g.notify, '; ') : '<span class=m>global only</span>'], ['global', list(g.global, '; ')]]);
  // #401: a tap on a harness, model or effort choice writes it (`cond`, a paired device only; disabled without `rw`).
  const k = g.conductor;
  const crow = (key: string, cur: string | null | undefined, opts: string[]) => `${form(d, ` class=chips aria-label=${key}`)}${hidden('cond', fleet, d.mtime)}<b class=m>${key}</b>${opts.map(o => `<button name=${key} value="${esc(o)}" aria-pressed=${cur === o}${d.rw ? '' : ' disabled'}>${esc(o)}</button>`).join('')}</form>`;
  const cond = k ? `<section class=cg><h3>Conductor<span>${k.default ? 'default: impl' : 'set'}</span></h3><p class=rh><span class=m>${esc(k.profile)}</span></p>${crow('harness', k.harness, g.harnesses.map(h => h.name))}${crow('model', k.model, k.models)}${crow('effort', k.effort, k.efforts)}</section>` : '';
  return `<p class=cp><span class=m>config</span> ${esc(g.path)}</p>${general}<section class="cg pp" data-pipe="${esc(JSON.stringify(g.pipe))}"><h3>Pipeline</h3></section>${limits}<section class=cg><h3>Roles<span>set: harness · model · effort</span></h3>${g.roles.map(r => role(d, c, r)).join('')}<p class=rk>profile, accounts, mcp: ${edit}</p></section>${cond}${alerts}<section class=cg><h3>Hosts / needs<span>after #157</span></h3><p class=soon>Which hosts this fleet's lanes may run on, matched by host tags against the issue's needs. Shown here once #157 lands.</p></section>`;
}
