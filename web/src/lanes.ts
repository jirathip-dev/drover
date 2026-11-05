// #269: the Lanes tab, drawn from API v1's `lanes` (pure: tests run it under node): state sections, repo groups
// inside, a row per lane (its avatar in each mode, name, state, why, activity) with its hidden sheet card (serve.js).
import type { LaneRow, LanesTab } from '../../assets/types/api';
import { esc } from './events.ts'; // node runs this file as is: the extension
import { blocked } from './needs.ts';
import { prLine } from './queue.ts';

const GALLOP = ['    _/>\n~(___)\n /  \\', '    _/>\n~(___)\n  ><'], STAND = '    _/>\n~(___)\n  | |', REST = '\n    _/>\n~(___)_', REAR = '     />\n ~(_/\n  / \\';

/** The state as an avatar in each mode (CSS shows `body[data-mode]`'s), its sprite the role's action (#241); its coat
 * stable per name. */
export function avatar(a: Pick<LaneRow, 'state' | 'act' | 'atone' | 'coat'>): string {
  const [glyph, art] = a.state === 'working' ? ['▶', `<i class="asc run"><b>${GALLOP[0]}</b><b>${GALLOP[1]}</b></i>`]
    : a.state === 'done' ? ['✓', `<i class=asc><b>${REST}</b></i>`]
    : a.state === 'blocked' || a.state === 'dead' ? ['✗', `<i class=asc><b>${REAR}</b></i>`]
    : ['⏸', `<i class=asc><b>${STAND}</b></i>`];
  return `<span class="av ${a.atone} ${a.state}" data-act=${a.act}><i class=gl>${glyph}</i>${art}<svg class=px viewBox="0 0 132 100" style="${a.coat}"><use href=#ha-${a.act} /></svg></span>`;
}

/** #53: line 1 the name + `state age`; line 2 the reason, or `superseded by`; then the activity or #276's `now:` line. */
function row(d: LanesTab, l: LaneRow): string {
  const why = l.why == null ? '' : `<span class="why ${l.why_tone}">${esc(l.why)}</span>`;
  const now = l.now, parts = now && [[now.w, 'el'], [now.n, 'el'], [now.t, 't']].filter(p => p[0] != null).map(([x, c]) => `<span class=${c}>${esc(x!)}</span>`);
  const act = now && parts ? `<span class="act nw el now" title="${esc(now.title)}">${parts.join('<i> · </i>')}</span>` : `<span class=act>${esc(l.activity)}</span>`;
  const card = [esc(l.head), esc(l.shown), esc(l.facts)];
  if (l.pr != null && l.issue != null) card.push(prLine(l.pr, l.repo, l.issue));
  if (l.dead) card.push('<span class=r>⚠ log/pane mismatch: the log has this lane open, herdr has no pane</span>');
  const ps = card.filter(Boolean).map(x => `<p>${x}</p>`).join('') + (l.blocked ? blocked(d, l.blocked) : '');
  const th = l.thread == null ? '' : ` data-thread=${esc(l.thread)}`; // #422: a tap opens it
  return `<div class=lane data-agent="${esc(l.name)}" data-role=${esc(l.role)}${l.gone ? ' data-gone' : ''}${th}><button type=button class=lr>${avatar(l)}<span class="nw el">${esc(l.name)}</span><span class="nw ${l.tone}">${esc(l.state)} ${l.ago}</span>${why}${act}</button><div class=card hidden>${ps}</div></div>`;
}

export function tab(d: LanesTab): string {
  if (d.error != null) return `<p class=m>${esc(d.error)}</p>`;
  if (d.failed) return '<p class=r>herdr agent list failed</p>';
  const count = (n: number) => `(${n})`;
  const h = d.sections.map(s => `<h2><span class=${s.tone}>${s.name} ${count(s.repos.reduce((n, r) => n + r.lanes.length, 0))}</span></h2>`
    + s.repos.map(r => `<h3>${esc(r.name)} ${count(r.lanes.length)}</h3>${r.lanes.map(l => row(d, l)).join('')}`).join(''));
  return h.join('') || '<p class=m>no agents</p>';
}
