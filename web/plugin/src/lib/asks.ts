// The drover-asks tab (herdsman#553 rows 63-71), drawn from API v1's `needs` as a tree of `h` nodes: pure, so the
// tests tap it under node. The count badge, then each open ask (its routine/owner line, a design's PNGs, options
// recommended first and a free-text box), each blocked lane's card (a permission prompt's reason, command, auto-deny
// countdown and menu), each `needs-human` issue (verdict, screenshots, merge anyway / one more round / park), the
// [later] fold, the answers herdsman logged `answer-undelivered`, and #17 the last asks it closed with their ending.
// An ask is open only while herdsman's `needs` lists it (herdsman#584): this tab keeps no open rule of its own.
import { events, image, needs, post, uniq } from "./api.ts";
import { h, type Kid, late, strip } from "./dom.ts";
import { threadHref } from "./links.ts";
import type { Blocked, Ev, NeedAsk, NeedIssue, NeedsTab, Reply } from "./types.ts";

/** #154: the cards hidden on this phone: key → until (s) and the card's content then (`sig`); a changed card shows. */
export type Later = Record<string, { until: number; sig: string }>;
export type Store = { get(): Later; set(l: Later): void };
type Img = [path: string, name: string, key: string];

/** The phone's localStorage when the frame has one (cooee's sandboxed frame has an opaque origin: then memory). */
export function phone(): Store {
  let mem: Later = {};
  return {
    get: () => {
      try {
        return JSON.parse(localStorage.getItem("drover-later") ?? "{}") as Later;
      } catch {
        return mem;
      }
    },
    set: (l) => {
      mem = l;
      try {
        localStorage.setItem("drover-later", JSON.stringify(l));
      } catch {
        // no storage in this frame: memory only
      }
    },
  };
}

const enc = encodeURIComponent;
const span = (cls: string, ...k: (Kid | false | null | undefined)[]) => h("span", { class: cls }, ...k);
const mmss = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;

/** #143: the answers whose last delivery try (per ask id) is `answer-undelivered`, in the last 24 h. */
export function undelivered(list: Ev[], now: number): Ev[] {
  const last = new Map<unknown, Ev>();
  for (const e of list) if (/^answer-(un)?delivered/.test(e.type)) last.set(e.data.id, e);
  return [...last.values()].filter((e) => e.type === "answer-undelivered" && e.ts + 86400 > now);
}

// #17 herdsman#584: an ending event's words (`herdsman asks --all`'s), by its type.
const END = new Map<string, (d: Record<string, unknown>) => string>([
  ["decision", (d) => (d.to ? `delivered to ${d.to}` : `answered by ${d.by}`)],
  ["ask-withdrawn", (d) => (d.for ? `withdrawn → ${d.for}` : "withdrawn")],
  ["ask-delivered", (d) => `delivered to ${d.to}`],
  ["ask-failed", () => "failed"],
  ["ask-expired", () => "expired"],
]);
const KEEP = /^(answer-|ask-(withdrawn|delivered|failed|expired)$|(needs-)?decision$)/;

/** #17: the asks herdsman closed in `list`, newest first: fleet, id, the ending, the question when read. */
export function ended(list: Ev[]): string[] {
  const q = new Map(
    list.filter((e) => e.type === "needs-decision").map((e) => [`${e.fleet} ${e.data.id}`, e.data.question]),
  );
  return [...list].reverse().flatMap((e) => {
    const end = END.get(e.type),
      s = q.get(`${e.fleet} ${e.data.id}`);
    return end ? [`${e.fleet} ${e.data.id} ${end(e.data)}${s ? ` · ${s}` : ""}`] : [];
  });
}

/**
 * The tab: `draw` gets the tree after every change; `clock` is the time in seconds, `store` where [later] keeps its
 * cards. `load` reads herdsman's needs and recent events; a tap posts a form op, shows herdsman's answer and reads
 * them again. `tick` redraws while a countdown runs; #17 `listen` reads them again on each herdsman event (`/live`).
 */
export function asksTab(draw: (kids: Kid[]) => void, clock = () => Date.now() / 1000, store = phone()) {
  let d: NeedsTab | null = null,
    err = "",
    at = 0,
    since = -1,
    tries: Ev[] = [],
    unfold = false,
    slow = false;
  const text: Record<string, string> = {},
    imgs: Record<string, string> = {};
  let msg: [string, string] = ["", ""];
  const pending = new Set<string>(),
    sure = new Set<string>();
  const redraw = () => draw([...strip(slow), ...(d ? view(d) : err ? [h("p", { class: "r" }, err)] : [])]);
  const load = async () => {
    const done = late((s) => {
      slow = s;
      if (s) redraw();
    });
    try {
      d = await needs();
      // #143: the answers' delivery tries (#17 and the asks' endings), from the newest 500 events on, then the new ones.
      if (since < 0) since = Math.max(0, (await events(0)).seq - 500);
      const e = await events(since);
      if (!e.refresh) {
        tries = uniq([...tries, ...e.list.filter((x) => KEEP.test(x.type))], (x) => x.seq);
        since = e.seq;
      }
      at = clock();
      // A recorded one more round is herdsman's line from now on.
      for (const k of pending)
        if (!d.issues.some((s) => `nh:${s.fleet}#${s.issue}` === k && !s.pending)) pending.delete(k);
    } catch (e) {
      d = null;
      err = String(e);
    }
    done();
    redraw();
    // #412 #417: each image once (per ask, per verdict), drawn when it comes.
    const want = d ? [...d.asks.flatMap(pngs), ...d.issues.flatMap(shots)].filter(([, , k]) => !(k in imgs)) : [];
    for (const [p, , k] of want) {
      imgs[k] = "";
      image(p).then((u) => {
        if (!u) return;
        imgs[k] = u;
        redraw();
      });
    }
  };
  const op = (key: string, name: string, f: Record<string, string>) => async () => {
    const r: Reply = await post(name, { op: name, mtime: d?.mtime ?? "", ...f });
    msg = [r.ok ? "g" : "r", r.ok ? r.message : `${r.code}: ${r.message}`];
    if (!r.ok) pending.delete(key);
    await load();
  };
  // An image: its path, name and cache key.
  const img = (kind: string, fleet: string, n: number, names: string[], v: string) =>
    names.map((name, i): Img => {
      const p = `/${kind}/${enc(fleet)}/${n}/${i}`;
      return [p, name, `${p} ${v}`];
    });
  const pngs = (a: NeedAsk) => img("design", a.ask.fleet, a.ask.issue ?? 0, a.pngs, a.ask.id);
  const shots = (s: NeedIssue) => img("shot", s.fleet, s.issue, s.shots, `${s.pass}${s.fail}`);
  const figs = (list: Img[]) =>
    list.map(([, n, k]) =>
      h("figure", {}, imgs[k] && h("img", { src: imgs[k], alt: n }), h("figcaption", { class: "m" }, n)),
    );

  const hide = (cards: Card[]) => () => {
    const l = store.get();
    for (const c of cards) l[c.key] = { until: clock() + 12 * 3600, sig: c.sig };
    store.set(l);
    unfold = false;
    redraw();
  };
  type Bit = Kid | false | null | undefined;
  type Card = { key: string; sig: string; body: Bit[] };
  const head = (who: string, ...k: (Kid | false | null | undefined)[]) =>
    h("p", {}, h("b", {}, "needs you "), span("m", who), ...k);

  // #40 #81 #99 #412: an ask: its kind (routine: defaults to N in Tm), a design's PNGs, options recommended first, free text.
  const ask = (a: NeedAsk): Card => {
    const x = a.ask,
      key = `ask:${x.id}`,
      go = (f: Record<string, string>) => op(key, "answer", { fleet: x.fleet, id: x.id, ...f });
    const opts = x.options
      .map((o, i) => [o, i + 1] as const)
      .sort((p, q) => +(p[1] !== x.recommend) - +(q[1] !== x.recommend));
    return {
      key,
      sig: JSON.stringify([x.question, x.options]),
      body: [
        head(
          a.who,
          " ",
          span(a.kind.startsWith("routine") ? "tag a" : "tag r", a.kind),
          a.from != null && span("m", ` · imported from ${a.from}`),
        ),
        h(
          "p",
          {},
          x.question,
          a.thread != null &&
            span(
              "m",
              " · ",
              h("a", { href: threadHref(a.thread), target: "_top" }, `asked in ${a.title || a.thread} · ${a.thread}`),
            ),
        ),
        ...figs(pngs(a)),
        d?.rw &&
          h(
            "p",
            { class: "btns" },
            ...opts.map(([o, n]) =>
              h("button", { on: go({ choice: String(n) }) }, `${n}. ${o}${n === x.recommend ? " (recommended)" : ""}`),
            ),
            h("input", {
              id: `t-${x.id}`,
              placeholder: "or type an answer",
              value: text[x.id] ?? "",
              oninput: (v) => (text[x.id] = v),
            }),
            h("button", { on: () => go({ text: text[x.id] ?? "" })() }, "Send"),
          ),
      ],
    };
  };
  // #42 #127: a blocked lane: a permission prompt's reason, command and auto-deny countdown, its menu by tap (`h`:
  // herdsman refuses a menu that changed since); else the pane's last lines; unpaired, its activity.
  const blocked = (b: Blocked): Card => {
    const key = `prompt:${b.name}`,
      m = b.menu;
    const body: Bit[] = [head(b.who)];
    if (b.activity != null)
      body.push(
        h("p", { class: "m" }, b.activity),
        h("p", { class: "m" }, "unpaired: the backend's HERDSMAN_COOKIE is no paired device, so it can't answer"),
      );
    else if (m != null) {
      const left = m.countdown == null ? null : Math.max(0, Math.round(m.countdown - (clock() - at)));
      body.push(
        h("p", {}, h("b", {}, m.title)),
        h("p", {}, m.reason),
        h("code", {}, m.command),
        left != null && h("p", { class: "r" }, `auto-denies in ${mmss(left)}`),
        h("p", {}, m.question),
        d?.rw &&
          h(
            "p",
            { class: "btns" },
            ...m.options.map((o, i) =>
              h(
                "button",
                { on: op(key, "answer", { agent: b.name, choice: String(i + 1), h: m.h }) },
                `${i + 1}. ${o}`,
              ),
            ),
          ),
      );
    } else body.push(h("pre", {}, b.tail ?? ""));
    return { key, sig: m?.h ?? b.tail ?? b.activity ?? "", body };
  };
  // #130 #151 #217 #309 #417: a needs-human issue: why, the last verdict and PR, the screenshots, then merge anyway
  // (a second tap confirms), one more round (its pending line at once), park.
  const issue = (s: NeedIssue): Card => {
    const n = s.issue,
      key = `nh:${s.fleet}#${n}`,
      f = { fleet: s.fleet, issue: String(n) };
    const sig = JSON.stringify([s.why, s.pass, s.fail, s.pr.verdict]);
    if (s.pending || pending.has(key))
      return { key, sig, body: [h("p", { class: "m" }, "⟳ one more round · starting a lane…")] };
    const p = s.pr,
      pr: Kid[] =
        p.pr != null && s.repo
          ? [h("a", { href: `https://github.com/${s.repo}/pull/${p.pr}`, target: "_top" }, `PR #${p.pr}`)]
          : ["no PR"];
    if (p.ci != null) pr.push(` · CI ${p.ci}`);
    const v =
      s.pass != null
        ? [`PASS at ${s.pass.slice(0, 7)}${s.stale ? " (not the head)" : ""} · `, ...pr, ` · ${s.mergeable}`]
        : [`FAIL: ${s.fail} · `, ...pr];
    const ask = sure.has(key);
    const merge = () => {
      if (ask) return op(key, "override", f)();
      sure.add(key);
      redraw();
    };
    const extend = () => {
      pending.add(key);
      redraw();
      return op(key, "extend", f)();
    };
    return {
      key,
      sig,
      body: [
        head(s.who),
        h("p", {}, `#${n} ${s.title} · ${s.why}`),
        h("p", { class: "m" }, ...v),
        ...figs(shots(s)),
        d?.rw &&
          d?.paired &&
          h(
            "p",
            { class: "btns" },
            s.merge && h("button", { on: merge }, ask ? `yes, merge #${n} past the round limit` : "merge anyway"),
            h("button", { on: extend }, "one more round"),
            h("button", { on: op(key, "park", f) }, "park"),
          ),
      ],
    };
  };
  const view = (d: NeedsTab): Kid[] => {
    const cards = [...d.asks.map(ask), ...d.blocked.map(blocked), ...d.issues.map(issue)];
    // #154: a card's key (its kind and who, less the age) and its content; hidden while both hold and 12 h last.
    // herdsman#447: the phone's store read once per draw, not once per card.
    const l = store.get(),
      hid = (c: Card) => {
        const e = l[c.key];
        return !!e && e.until > clock() && e.sig === c.sig;
      };
    const off = cards.filter(hid),
      on = cards.filter((c) => !hid(c));
    store.set(Object.fromEntries(Object.entries(l).filter(([k]) => off.some((c) => c.key === k))));
    const card = (c: Card) =>
      h("div", { class: "ask" }, ...c.body, h("button", { class: "lat", on: hide([c]) }, "later"));
    return h(
      "main",
      {},
      h(
        "p",
        { class: "bar" },
        h("b", {}, "needs you "),
        span(cards.length ? "badge on" : "badge", String(cards.length)),
      ),
      msg[1] && h("p", { class: msg[0] }, msg[1]),
      ...undelivered(tries, clock()).map((e) => h("p", { class: "r" }, `undelivered: ${e.plain}`)),
      off.length > 0 &&
        h(
          "p",
          { class: "r later" },
          h(
            "button",
            {
              class: "lk",
              on: () => {
                unfold = !unfold;
                redraw();
              },
            },
            `${off.length} need you ${unfold ? "▾" : "▸"}`,
          ),
          h("button", { on: hide(unfold ? cards : on) }, "later all"),
        ),
      off.length === 0 && on.length > 1 && h("p", {}, h("button", { on: hide(on) }, "later all")),
      ...(unfold ? cards : on).map(card),
      !cards.length && h("p", { class: "m" }, "nothing needs you"),
      ...ended(tries)
        .slice(0, 5)
        .map((l) => h("p", { class: "m" }, `ended: ${l}`)),
    ).k;
  };
  const tick = () => d?.blocked.some((b) => b.menu?.countdown != null) && redraw();
  // #17: one read at a time; events while it runs make one more.
  let busy: Promise<void> | null = null,
    again = false;
  const live = (): Promise<void> => {
    if (busy) {
      again = true;
      return busy;
    }
    busy = load().finally(() => {
      busy = null;
      if (again) {
        again = false;
        live();
      }
    });
    return busy;
  };
  const listen = () => cooee.events("/live", (e) => (e.event === "ev" || e.event === "refresh") && live());
  return { load, tick, listen };
}

export const CSS = `
body{font:15px system-ui,sans-serif;color:var(--fg);background:var(--bg);margin:8px}
a{color:var(--link)}button{font:inherit;color:var(--fg);background:var(--card);border:1px solid var(--edge);border-radius:6px;padding:2px 8px;margin:2px}
input{font:inherit;margin:2px}p{margin:4px 0}
.ask{background:var(--card);border:1px solid var(--edge);border-left:4px solid var(--r);border-radius:8px;padding:8px;margin:8px 0}
.badge{border-radius:9px;padding:0 7px;background:var(--edge)}.badge.on{background:var(--r);color:var(--bg)}
.tag{font-size:.85em}.lk{cursor:pointer}.lat{float:right}.later button{color:var(--r)}
code,pre{display:block;white-space:pre-wrap;font-size:.85em}figure{margin:6px 0}img{max-width:100%;border-radius:6px}
.g{color:var(--g)}.a{color:var(--a)}.r{color:var(--r)}.m{color:var(--mute)}
`;
