//! The per-fleet work queue (`queue` events): `herdsman queue`, `next` and `board`.
use crate::config::{Fleet, Serial};
use crate::events::{age, fold, last_spawn, Event, IssueState, States};
use crate::overlap::Conflicts;
use anyhow::{anyhow, ensure, Result};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Default)]
#[allow(clippy::struct_excessive_bools)] // independent queue marks (`--design`, `--architect`, `--repro`, …)
pub struct Item {
    pub issue: u64,
    /// #505: every issue it waits on (`queue block --on A --on B`); a merged one drops out in `view`.
    pub blockers: Vec<u64>,
    /// The first of `blockers` still open, set by `view`: the frozen board (#552) and /api/v1 rows show one.
    pub blocked_by: Option<u64>,
    /// #667: blockers in another fleet or repo (`fleet#n`, `owner/repo#n`); one drops out in `view` once merged in its
    /// fleet or read closed on GitHub (`blocker-closed`).
    pub ext: Vec<String>,
    /// #294: the serial groups it is marked for (`queue add --serial [G]`, `queue set <n> serial G`; `""` = the first).
    pub serial: Vec<String>,
    /// #157: host tags its lanes need besides the fleet's and role's (`--needs`, `queue set <n> needs`).
    pub needs: Vec<String>,
    /// #365: queued from the board's backlog, its spec not checked yet (`needs-check`).
    pub check: bool,
    /// #412: a design lane first, impl only once the owner approves it (`--design`, `queue set <n> design on|off`).
    pub design: bool,
    /// #730: an architect lane first (before any design), same approval (`--architect`, `queue set <n> architect on|off`).
    pub architect: bool,
    /// #417: the verify stage forced on or off (`--verify [on|off]`, `queue set <n> verify on|off`); unset = by rule.
    pub verify: Option<bool>,
    /// #420: a bug fix that starts with a failing test (`--repro`, `queue set <n> repro on|off`).
    pub repro: bool,
    /// #734: `perf` or `refactor` (`--perf`, `--refactor`): the playbook its briefs and rev's PASS follow.
    pub playbook: Option<String>,
}

/// A queue change, as logged: `op` is add | mv | rm | block | unblock | set (`needs`).
pub fn op(op: &str, issues: &[u64], top: bool, before: Option<u64>, after: Option<u64>, on: Option<u64>) -> Value {
    let mut d = json!({"op": op, "issues": issues});
    for (k, v) in [("before", before), ("after", after), ("on", on)] {
        if let Some(v) = v {
            d[k] = json!(v);
        }
    }
    if top {
        d["top"] = json!(true);
    }
    d
}

/// Apply one change; Err leaves `q` untouched.
pub fn apply(q: &mut Vec<Item>, d: &Value) -> Result<()> {
    let issues = nums(&d["issues"]);
    ensure!(!issues.is_empty(), "no issue given");
    let pos = |q: &[Item], n: u64| {
        q.iter()
            .position(|i| i.issue == n)
            .ok_or_else(|| anyhow!("#{n} is not queued"))
    };
    match d["op"].as_str().unwrap_or_default() {
        op @ ("add" | "mv") => {
            if op == "mv" {
                pos(q, issues[0])?;
            }
            let anchor = d["before"].as_u64().or(d["after"].as_u64());
            if let Some(a) = anchor {
                ensure!(!issues.contains(&a), "#{a} cannot be placed relative to itself");
                pos(q, a)?;
            }
            let serial = strs(&d["serial"]);
            let mut moved: Vec<Item> = issues
                .iter()
                .map(|&n| Item {
                    issue: n,
                    blockers: vec![],
                    blocked_by: None,
                    ext: vec![],
                    serial: serial.clone(),
                    needs: strs(&d["needs"]),
                    check: d["check"] == true,
                    design: d["design"] == true,
                    architect: d["architect"] == true,
                    verify: onoff(&d["verify"]),
                    repro: d["repro"] == true,
                    playbook: d["playbook"].as_str().map(String::from),
                })
                .collect();
            for m in &mut moved {
                if let Some(old) = q.iter().find(|i| i.issue == m.issue) {
                    m.blockers.clone_from(&old.blockers);
                    m.ext.clone_from(&old.ext);
                    m.check |= old.check;
                    m.design |= old.design;
                    m.architect |= old.architect;
                    m.repro |= old.repro;
                    m.playbook = m.playbook.take().or_else(|| old.playbook.clone());
                    m.verify = m.verify.or(old.verify);
                    m.serial.extend(old.serial.iter().cloned());
                    m.needs = old.needs.iter().chain(&m.needs).cloned().collect();
                    m.needs.dedup();
                }
            }
            q.retain(|i| !issues.contains(&i.issue));
            let at = match (d["top"].as_bool(), d["before"].as_u64(), d["after"].as_u64()) {
                (Some(true), ..) => 0,
                (_, Some(b), _) => pos(q, b)?,
                (_, _, Some(a)) => pos(q, a)? + 1,
                _ => q.len(),
            };
            q.splice(at..at, moved);
        }
        "rm" => {
            q.remove(pos(q, issues[0])?);
        }
        // #505: `on` is a list (one number before #505); block adds to the set, unblock drops some or all.
        op @ ("block" | "unblock") => {
            let n = issues[0];
            let (on, ext) = (nums(&d["on"]), refs(&d["on"]));
            let i = pos(q, n)?;
            if op == "unblock" {
                let all = on.is_empty() && ext.is_empty();
                q[i].blockers.retain(|b| !all && !on.contains(b));
                q[i].ext.retain(|b| !all && !ext.contains(b));
                return Ok(());
            }
            ensure!(!on.is_empty() || !ext.is_empty(), "block needs --on");
            for b in ext {
                let ok = b
                    .split_once('#')
                    .is_some_and(|(r, m)| !r.is_empty() && m.parse::<u64>().is_ok());
                ensure!(ok, "`{b}` is no issue: `<n>`, `<fleet>#<n>` or `<owner/repo>#<n>`");
                if !q[i].ext.contains(&b) {
                    q[i].ext.push(b);
                }
            }
            for &b in &on {
                ensure!(b != n, "#{b} cannot block itself");
                let mut path = vec![b];
                while let Some(x) = path.pop() {
                    ensure!(x != n, "#{b} cannot block #{n}: #{b} already waits on #{n}");
                    path.extend(q.iter().filter(|i| i.issue == x).flat_map(|i| &i.blockers));
                }
                if !q[i].blockers.contains(&b) {
                    q[i].blockers.push(b);
                }
            }
        }
        "set" => {
            let i = pos(q, issues[0])?;
            if d.get("serial").is_some() {
                q[i].serial = strs(&d["serial"]);
            } else if d.get("design").is_some() {
                q[i].design = strs(&d["design"]) == ["on"];
            } else if d.get("architect").is_some() {
                q[i].architect = strs(&d["architect"]) == ["on"];
            } else if d.get("verify").is_some() {
                q[i].verify = onoff(&d["verify"]);
            } else if d.get("repro").is_some() {
                q[i].repro = strs(&d["repro"]) == ["on"];
            } else {
                q[i].needs = strs(&d["needs"]);
            }
        }
        o => anyhow::bail!("unknown queue op `{o}`"),
    }
    Ok(())
}

/// #157: a change's `needs` tags; #294: its `serial` marks (#115's `true`: `[""]`, the first group).
fn strs(v: &Value) -> Vec<String> {
    match v {
        Value::Bool(b) => vec![String::new(); usize::from(*b)],
        Value::String(g) => vec![g.clone()],
        v => v
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t.as_str().map(String::from))
            .collect(),
    }
}

/// #505: a list of issue numbers, or one number (a `block`'s `on` before #505).
pub fn nums(v: &Value) -> Vec<u64> {
    let all = v.as_array().into_iter().flatten().filter_map(Value::as_u64);
    v.as_u64().into_iter().chain(all).collect()
}

/// #667: a `block`'s `on` blockers in another fleet or repo (`fleet#n`, `owner/repo#n`).
pub fn refs(v: &Value) -> Vec<String> {
    let all = v.as_array().into_iter().flatten();
    all.filter_map(|b| b.as_str().map(String::from)).collect()
}

/// #667: blocker `b` (`fleet#n`, `owner/repo#n`) is merged in its fleet or read closed on GitHub.
pub fn released(events: &[Event], st: &States, b: &str) -> bool {
    let merged = b
        .split_once('#')
        .and_then(|(f, n)| Some(state(st, f, n.parse().ok()?) == "merged"));
    merged == Some(true)
        || events
            .iter()
            .any(|e| e.kind == "blocker-closed" && e.str("ref") == Some(b))
}

/// #667: `#11, herdsman#633, cooee#87`: the item's open blockers, another repo's by its name.
pub fn labels(i: &Item) -> String {
    let ext = i.ext.iter().map(|b| b.rsplit('/').next().unwrap_or(b).to_string());
    let all: Vec<String> = i.blockers.iter().map(|n| format!("#{n}")).chain(ext).collect();
    all.join(", ")
}

/// #417: `on`/`off` (or `["on"]`, as `queue set` logs it) as a forced verify stage.
fn onoff(v: &Value) -> Option<bool> {
    strs(v)
        .first()
        .and_then(|o| ["off", "on"].iter().position(|x| x == o))
        .map(|i| i == 1)
}

/// #157: the queued issue's `needs` (its queue item's, merged ones included).
pub fn needs_of(events: &[Event], fleet: &str, issue: u64) -> Vec<String> {
    let q = replay(events, fleet).into_iter().find(|i| i.issue == issue);
    q.map(|i| i.needs).unwrap_or_default()
}

/// #412: the pre-impl stage issue `n` is queued for that the owner has not approved yet: #730's `architect` first, then
/// `design` (an approval names its stage's `role`; none before #730: `design`).
pub fn wants_design(events: &[Event], fleet: &str, n: u64) -> Option<&'static str> {
    let q = replay(events, fleet).into_iter().find(|i| i.issue == n)?;
    let approved = |r| {
        events
            .iter()
            .any(|e| e.is("design-approved", fleet, n) && e.str("role").unwrap_or("design") == r)
    };
    [("architect", q.architect), ("design", q.design)]
        .into_iter()
        .find(|&(r, on)| on && !approved(r))
        .map(|(r, _)| r)
}

/// #420: issue `n` is queued `--repro`.
pub fn repro(events: &[Event], fleet: &str, n: u64) -> bool {
    replay(events, fleet).iter().any(|i| i.issue == n && i.repro)
}

/// #734: issue `n`'s playbook, `perf` or `refactor`.
pub fn playbook(events: &[Event], fleet: &str, n: u64) -> Option<String> {
    replay(events, fleet).into_iter().find(|i| i.issue == n)?.playbook
}

/// Replay the fleet's `queue` events; invalid ones (never logged by fleet) are skipped.
pub fn replay(events: &[Event], fleet: &str) -> Vec<Item> {
    let mut q = vec![];
    for e in events.iter().filter(|e| e.kind == "queue" && e.fleet == fleet) {
        let mut next = q.clone();
        if apply(&mut next, &Value::Object(e.data.clone())).is_ok() {
            q = next;
        }
    }
    q
}

fn state<'a>(st: &'a States, fleet: &str, n: u64) -> &'a str {
    st.get(&(fleet.to_string(), n)).map_or("queued", |s| s.state.as_str())
}

/// #294: the serial groups issue `n` is in, by name: marked in queue `q` (`""` = the first group) or joined by its diff
/// (`serial-joined`; #149's `serial-auto` = the first group), merged ones included.
pub fn groups<'a>(q: &[Item], events: &[Event], f: &'a Fleet, fleet: &str, n: u64) -> Vec<&'a Serial> {
    let marks = q.iter().filter(|i| i.issue == n).flat_map(|i| &i.serial);
    let joined = events.iter().filter(|e| e.fleet == fleet && e.issue == Some(n));
    let joined = joined.filter_map(|e| match e.kind.as_str() {
        "serial-joined" => e.str("group"),
        "serial-auto" => Some(""),
        _ => None,
    });
    let (all, first): (Vec<&str>, _) = (marks.map(String::as_str).chain(joined).collect(), f.serial.first());
    let is = |g: &&Serial| {
        all.iter()
            .any(|&a| a == g.name || a.is_empty() && first.is_some_and(|x| x.name == g.name))
    };
    let mut gs: Vec<_> = f.serial.iter().filter(is).collect();
    gs.sort_by(|a, b| a.name.cmp(&b.name));
    gs
}

/// In flight: a live impl/fix lane, or at review/ready.
pub fn in_flight(state: &str) -> bool {
    state == "impl" || state.starts_with("fix") || state.starts_with("review") || state == "ready"
}

/// #294: `n` waits for a slot in group `g`: its last `serial-held` / `serial-slot` there is a hold.
pub fn held(events: &[Event], fleet: &str, n: u64, g: &str) -> bool {
    let mine = |e: &&Event| e.fleet == fleet && e.issue == Some(n) && e.str("group") == Some(g);
    let e = events.iter().rev().filter(mine).find(|e| e.kind.starts_with("serial-"));
    e.is_some_and(|e| e.kind == "serial-held")
}

/// #294: the issues holding a slot in group `g`: in flight, in it and not held; over `max` only if grandfathered.
pub fn holders<'a>(events: &[Event], st: &'a States, f: &Fleet, fleet: &str, g: &str) -> Vec<&'a IssueState> {
    let q = replay(events, fleet);
    let isin = |s: &IssueState| groups(&q, events, f, fleet, s.issue).iter().any(|x| x.name == g);
    let holds = |s: &&IssueState| in_flight(&s.state) && !held(events, fleet, s.issue, g) && isin(s);
    st.values().filter(|s| s.fleet == fleet).filter(holds).collect()
}

/// #294: `ci: #2968 holds 1/1`, group `g`'s holders.
pub fn holds(g: &Serial, h: &[&IssueState]) -> String {
    let who: Vec<_> = h.iter().map(|s| format!("#{}", s.issue)).collect();
    let s = ["", "s"][usize::from(h.len() == 1)];
    format!("{}: {} hold{s} {}/{}", g.name, who.join(" "), h.len(), g.max)
}

/// `serial: db 3/10 · ci 1/1` for a fleet with serial groups: board, status and the Host tab.
pub fn serial_use(events: &[Event], st: &States, fleet: &str, cfg: Option<&crate::config::Config>) -> Option<String> {
    let f = cfg.and_then(|c| c.fleet.get(fleet)).filter(|f| !f.serial.is_empty())?;
    let one = |g: &Serial| format!("{} {}/{}", g.name, holders(events, st, f, fleet, &g.name).len(), g.max);
    let gs: Vec<_> = f.serial.iter().map(one).collect();
    Some(format!("serial: {}", gs.join(" · ")))
}

/// The queue as shown: merged issues drop out; a blocker counts until it is merged.
pub fn view(events: &[Event], st: &States, fleet: &str) -> Vec<Item> {
    let open = replay(events, fleet)
        .into_iter()
        .filter(|i| !matches!(state(st, fleet, i.issue), "merged" | "closed"));
    open.map(|mut i| {
        i.blockers.retain(|&b| state(st, fleet, b) != "merged");
        i.ext.retain(|b| !released(events, st, b));
        i.blocked_by = i.blockers.first().copied();
        i
    })
    .collect()
}

/// `herdsman next`: (exit code, line) for the first unblocked queued issue with no lane.
pub fn next(events: &[Event], fleet: &str) -> (i32, String) {
    let st = fold(events);
    let free: Vec<Item> = view(events, &st, fleet)
        .into_iter()
        .filter(|i| state(&st, fleet, i.issue) == "queued")
        .collect();
    if let Some(n) = startable(events, fleet).into_iter().next() {
        return (0, n);
    }
    match free.first() {
        Some(i) if !labels(i).is_empty() => (2, format!("#{} is blocked by {}", i.issue, labels(i))),
        _ => (1, format!("{fleet}: queue empty")),
    }
}

/// #505: `#3133, #3134`.
pub fn hashes(ns: &[u64]) -> String {
    ns.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(", ")
}

/// Every unblocked queued issue with no lane, in queue order: drive tries each, `next` is the first.
pub fn startable(events: &[Event], fleet: &str) -> Vec<String> {
    let st = fold(events);
    let free = |i: &Item| state(&st, fleet, i.issue) == "queued" && i.blockers.is_empty() && i.ext.is_empty();
    let q = view(events, &st, fleet);
    q.into_iter().filter(free).map(|i| i.issue.to_string()).collect()
}

/// Drive's refused start of queued issue `n` in the last hour as (board, status) text, `skipped this pass`; #572: not
/// once older than its last state change, or (other codes) than a merge in its serial groups. `SERIAL_BUSY` shows the
/// group's holders now, or its free slot.
pub fn skipped(
    events: &[Event],
    st: &States,
    f: Option<&Fleet>,
    fleet: &str,
    n: u64,
    now: u64,
) -> Option<(String, String)> {
    let s = |e: &&Event| e.kind == "driven" && e.fleet == fleet && e.issue == Some(n) && e.str("step") == Some("start");
    let is = st.get(&(fleet.to_string(), n)).filter(|s| s.state == "queued")?;
    let e = events
        .iter()
        .rev()
        .find(s)
        .filter(|e| now < e.ts + 3600 && e.ts >= is.last_ts)?;
    let (code, q) = (e.str("code")?, replay(events, fleet));
    let gs = f.map_or(vec![], |f| groups(&q, events, f, fleet, n));
    if let (Some(f), "SERIAL_BUSY") = (f, code) {
        for g in &gs {
            let h = holders(events, st, f, fleet, &g.name);
            if h.len() >= g.max {
                let who = holds(g, &h).split_off(g.name.len() + 2);
                return Some((format!("skip {code}"), format!("waiting for a {} slot ({who})", g.name)));
            }
        }
        let g = &gs.first()?.name;
        return Some((
            format!("{g} slot free"),
            crate::promise::Say::Starts(format!("{g} slot free")).line(),
        ));
    }
    let mine = |m: u64| {
        f.is_some_and(|f| {
            groups(&q, events, f, fleet, m)
                .iter()
                .any(|x| gs.iter().any(|g| g.name == x.name))
        })
    };
    let merged = |m: &Event| m.kind == "merged" && m.fleet == fleet && m.ts > e.ts && m.issue.is_some_and(mine);
    (!events.iter().any(merged)).then_some(())?;
    let why = e.str("result")?.split_once(": ").map_or("", |w| w.1);
    let ago = age(now.saturating_sub(e.ts));
    Some((format!("skip {code}"), format!("{code} ({why}) · {ago} ago")))
}

/// #712: a parked issue's `park --why`.
pub fn park_why<'a>(events: &'a [Event], st: &States, fleet: &str, n: u64) -> Option<&'a str> {
    let parked = |e: &&Event| e.is("parked", fleet, n);
    (state(st, fleet, n) == "parked").then(|| events.iter().rfind(parked)?.str("why"))?
}

pub fn ls_json(events: &[Event], st: &States, fleet: &str) -> Value {
    let row = |(k, i): (usize, &Item)| json!({"pos": k + 1, "issue": i.issue, "state": state(st, fleet, i.issue), "blocked_by": i.blockers, "blocked_ext": i.ext, "serial": !i.serial.is_empty(), "needs": i.needs, "design": i.design, "architect": i.architect, "verify": i.verify, "repro": i.repro, "playbook": i.playbook, "why": park_why(events, st, fleet, i.issue)});
    Value::Array(view(events, st, fleet).iter().enumerate().map(row).collect())
}

pub const WIDTH: usize = 60;

/// One fleet's board, lines ≤ 60 cols: the queue, other open issues, issues merged in the last 24 h.
pub fn board(
    ctx: &crate::Ctx,
    events: &[Event],
    fleet: &str,
    cfg: Option<&crate::config::Config>,
    c: &Conflicts,
    at: &std::collections::BTreeMap<String, String>,
) -> String {
    let now = ctx.now;
    let st = fold(events);
    let q = view(events, &st, fleet);
    let mine: Vec<&IssueState> = st.values().filter(|s| s.fleet == fleet).collect();
    let active = mine.iter().filter(|s| s.active_lane().is_some()).count();
    let head = format!(
        "{:>2} {:<6} {:<11} {:<20} {:<5} {:<4} LAST",
        "Q", "ISSUE", "STATE", "LANE", "EFF", "AGE"
    );
    let serial = serial_use(events, &st, fleet, cfg).map_or(String::new(), |s| format!(", {s}"));
    let mut lines = vec![format!("{fleet}: {} queued, {active} active{serial}", q.len()), head];
    let row = |pos: String, n: u64, last: Option<String>| {
        let s = st.get(&(fleet.to_string(), n));
        let f = cfg.and_then(|c| c.fleet.get(fleet));
        let skip = skipped(events, &st, f, fleet, n, now).map(|s| s.0);
        let lane: String = s
            .and_then(|s| s.lane.clone())
            .or(skip)
            .unwrap_or_else(|| "-".into())
            .chars()
            .take(20)
            .collect();
        let eff = s.and_then(|s| s.effort.as_deref()).unwrap_or("-");
        let (ts, ev) = s.map_or((0, ""), |s| (s.last_ts, s.last_event.as_str()));
        let a = if ts == 0 {
            "-".into()
        } else {
            age(now.saturating_sub(ts))
        };
        // #129: its own line, the row is full: `⚠ conflicts with #98 (serve.rs)`.
        let warn = c.get(&n).and_then(|c| c.first()).map_or(String::new(), |(o, files)| {
            let o = if *o == 0 { "integration".into() } else { format!("#{o}") };
            let names: Vec<_> = files.iter().map(|p| p.rsplit('/').next().unwrap_or_default()).collect();
            format!("\n         ⚠ conflicts with {o} ({})", names.join(", "))
        });
        // #373: its own line too, until a PASS: `✗ migration-order failed: rename needed`.
        let pass = |e: &&Event| e.is("verdict", fleet, n) && e.str("verdict") == Some("PASS");
        let hit = |e: &&Event| e.is("premerge-failed", fleet, n) || pass(e);
        let note = events.iter().rev().find(hit).and_then(|e| e.str("note"));
        let warn = note.map_or(String::new(), |t| format!("\n         ✗ {t}")) + &warn;
        // #478: its own line too: an active lane on a host the last look could not reach.
        let away = s.and_then(|s| crate::hosts::offline(ctx, last_spawn(events, s.active_lane()?.1)));
        let warn = away.map_or(String::new(), |h| format!("\n         host offline: {h}")) + &warn;
        let sp = s.and_then(|s| last_spawn(events, s.active_lane()?.1));
        let stale = sp.zip(cfg).is_some_and(|(e, c)| crate::hook::rules_stale(c, fleet, e));
        // #609: its own line too, the row is full: where the active lane runs (`on mini · shed`).
        let place = s.and_then(|s| at.get(s.active_lane()?.1)).filter(|p| !p.is_empty());
        let warn = place.map_or(String::new(), |p| format!("\n         on {p}")) + &warn;
        let warn = if stale {
            "\n         rules stale: respawn to apply".into()
        } else {
            String::new()
        } + &warn;
        let last = last.unwrap_or_else(|| if ev.is_empty() { "-".into() } else { ev.into() });
        format!(
            "{pos:>2} #{n:<5} {:<11} {lane:<20} {eff:<5} {a:<4} {last}{warn}",
            state(&st, fleet, n)
        )
    };
    for (k, i) in q.iter().enumerate() {
        // #505: several blockers get their own line, the row is full.
        let on = match (&i.blockers[..], i.ext.len()) {
            ([], 0) => None,
            ([b], 0) => Some(format!("on #{b}")),
            _ => Some(format!("on…\n         blocked by {}", labels(i))),
        };
        lines.push(row((k + 1).to_string(), i.issue, on));
    }
    let queued = |n: u64| q.iter().any(|i| i.issue == n);
    let rest = mine.iter().filter(|s| !queued(s.issue) && !s.done());
    let merged = mine
        .iter()
        .filter(|s| s.done() && now.saturating_sub(s.last_ts) < 86_400);
    for s in rest.chain(merged) {
        lines.push(row("-".into(), s.issue, None));
    }
    lines
        .iter()
        .flat_map(|l| l.lines())
        .map(|l| l.chars().take(WIDTH).collect::<String>().trim_end().to_string() + "\n")
        .collect()
}

/// Fleets with anything in the log, in name order.
pub fn fleets(events: &[Event]) -> Vec<String> {
    let set: std::collections::BTreeSet<&str> = events.iter().map(|e| e.fleet.as_str()).collect();
    set.into_iter().map(String::from).collect()
}
