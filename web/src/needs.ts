// #269: the needs cards above every tab, drawn from API v1's `needs` (pure: tests run it under node): each open ask
// (options recommended first, a free-text box), each blocked lane's card, then the `needs-human` issues (#130).
import type { Blocked, NeedIssue, NeedsTab } from '../../assets/types/api';
import { enc } from './chat.ts';
import { esc } from './events.ts'; // node runs this file as is: the extension
import { answer, asked, form, hidden, prLine } from './queue.ts';

/** What a card's forms read: the board takes forms (`rw`), posts them (`live`), the device is paired, offers Pair. */
export type Flags = Pick<NeedsTab, 'now' | 'rw' | 'live' | 'paired' | 'pair'>;

const PAIRB = ' <button type=button class=pairb>Pair this phone</button>';

/** A blocked lane's card: its harness menu's options (`/answer`) under its title, reason, command and countdown, else
 * the pane's last lines, and a Reply button that opens the lane's sheet; unpaired, the activity and a pairing hint. */
export function blocked(d: Flags, b: Blocked): string {
  let body: string;
  if (b.activity != null) body = `<p class=m>${esc(b.activity)}</p><p class=m>pair this phone to answer${d.pair ? PAIRB : ''}</p>`;
  else if (b.menu != null) {
    const m = b.menu, n = m.countdown;
    const cd = n == null ? '' : `<p class=cd data-until=${d.now + n}>auto-denies in ${Math.floor(n / 60)}:${String(n % 60).padStart(2, '0')}</p>`;
    const opts = m.options.map((o, i) => `<button type=button class=pick value=${i + 1} data-h=${m.h}>${i + 1}. ${esc(o)}</button>`);
    body = `<p class=title>${esc(m.title)}</p><p class=reason>${esc(m.reason)}</p><p class=command>${esc(m.command)}</p>${cd}<p>${esc(m.question)}</p>${opts.join('')}`;
  } else body = `<pre>${esc(b.tail ?? '')}</pre>`;
  const reply = d.rw && d.paired ? '<button type=button class=rp>Reply</button><p class=as></p>' : '';
  return `<div class=ask data-ask="${esc(b.name)}" data-push=${b.push ? 1 : 0}><p><b>needs you</b> <span class=m>${esc(b.who)}</span></p>${body}${reply}</div>`;
}

/** #130: a `needs-human` issue's card: why, the last verdict, then (paired) merge anyway, one more round, park; #217:
 * one more round recorded with no lane yet is one muted line. */
function issue(d: NeedsTab, s: NeedIssue): string {
  const n = s.issue;
  if (s.pending) return `<div class=pend data-n=${n}><p class=m>⟳ one more round · starting a lane…</p></div>`;
  const line = prLine(s.pr, s.repo, n);
  const v = s.pass != null ? `PASS at ${s.pass.slice(0, 7)}${s.stale ? ' (not the head)' : ''} · ${line} · ${s.mergeable}` : `FAIL: ${esc(s.fail)} · ${line}`;
  const merge = s.merge ? `<button name=op value=override data-confirm="Merge #${n} past the round limit?">merge anyway</button>` : '';
  const f = d.rw && d.paired ? `${form(d)}${hidden('', s.fleet, d.mtime)}<input type=hidden name=issue value=${n}>${merge}<button name=op value=extend>one more round</button><button name=op value=park>park</button></form>` : '';
  // #417: the verdict's screenshots, inline, as `/shot/<fleet>/<n>/<i>` serves them; #731: its logs and reports as links.
  const shots = s.shots.map((x, i) => /\.png$/i.test(x) ? `<figure class=im><img src="shot/${enc(s.fleet)}/${n}/${i}" alt="${esc(x)}" loading=lazy><figcaption>${esc(x)}</figcaption></figure>` : `<p class=m><a href="shot/${enc(s.fleet)}/${n}/${i}">${esc(x)}</a></p>`).join('');
  return `<div class=ask data-n=${n}><p><b>needs you</b> <span class=m>${esc(s.who)}</span></p><p>#${n} ${esc(s.title)} · ${esc(s.why)}</p><p class=m>${v}</p>${shots}${f}</div>`;
}

/** The `#needs` strip: the snapshot's time (#450: serve.js folds its age into the strip), the cards, and `[chat·N]`'s count (#386). */
export function tab(d: NeedsTab): string {
  const age = `<i id=sts hidden data-ts=${d.snap}></i>`;
  const asks = d.asks.map(a => {
    const from = a.from == null ? '' : ` · imported from ${esc(a.from)}`, f = d.rw ? answer(d, a.ask, 'Send') : '';
    // #412: a design's PNGs, inline, as `/design/<fleet>/<n>/<i>` serves them.
    const pngs = a.pngs.map((n, i) => `<figure class=im><img src="design/${enc(a.ask.fleet)}/${a.ask.issue ?? 0}/${i}" alt="${esc(n)}" loading=lazy><figcaption>${esc(n)}</figcaption></figure>`);
    return `<div class=ask><p><b>needs you</b> <span class=m>${esc(a.who)}</span> <span class=m>· ${a.kind}${from}</span></p><p>${asked(a)}</p>${pngs.join('')}${f}</div>`;
  });
  return age + asks.join('') + d.blocked.map(b => blocked(d, b)).join('') + d.issues.map(s => issue(d, s)).join('') + `<i id=chn hidden data-n=${d.choosing}></i>`;
}
