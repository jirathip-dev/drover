//! The per-fleet work queue (`queue` events): `herdsman queue`, `next` and `board`.
use crate::config::{Fleet, Serial};
use crate::events::{age, fold, Event, IssueState, States};
use crate::overlap::Conflicts;
use anyhow::{anyhow, ensure, Result};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub issue: u64,
    pub blocked_by: Option<u64>,
    /// #294: the serial groups it is marked for (`queue add --serial [G]`, `queue set <n> serial G`; `""` = the first).
    pub serial: Vec<String>,
    /// #157: host tags its lanes need besides the fleet's and role's (`--needs`, `queue set <n> needs`).
    pub needs: Vec<String>,
}

/// A queue change, as logged: `op` is add | mv | rm | block | set (`needs`).
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
    let issues: Vec<u64> = d["issues"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .collect();
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
                    blocked_by: None,
                    serial: serial.clone(),
                    needs: strs(&d["needs"]),
                })
                .collect();
            for m in &mut moved {
                if let Some(old) = q.iter().find(|i| i.issue == m.issue) {
                    m.blocked_by = old.blocked_by;
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
        "block" => {
            let on = d["on"].as_u64().ok_or_else(|| anyhow!("block needs --on"))?;
            ensure!(on != issues[0], "#{on} cannot block itself");
            let i = pos(q, issues[0])?;
            q[i].blocked_by = Some(on);
        }
        "set" => {
            let i = pos(q, issues[0])?;
            if d.get("serial").is_some() {
                q[i].serial = strs(&d["serial"]);
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

/// #157: the queued issue's `needs` (its queue item's, merged ones included).
pub fn needs_of(events: &[Event], fleet: &str, issue: u64) -> Vec<String> {
    let q = replay(events, fleet).into_iter().find(|i| i.issue == issue);
    q.map(|i| i.needs).unwrap_or_default()
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
    open.map(|i| Item {
        blocked_by: i.blocked_by.filter(|b| state(st, fleet, *b) != "merged"),
        ..i
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
        Some(Item {
            issue,
            blocked_by: Some(b),
            ..
        }) => (2, format!("#{issue} is blocked by #{b}")),
        _ => (1, format!("{fleet}: queue empty")),
    }
}

/// Every unblocked queued issue with no lane, in queue order: drive tries each, `next` is the first.
pub fn startable(events: &[Event], fleet: &str) -> Vec<String> {
    let st = fold(events);
    let free = |i: &Item| state(&st, fleet, i.issue) == "queued" && i.blocked_by.is_none();
    let q = view(events, &st, fleet);
    q.into_iter().filter(free).map(|i| i.issue.to_string()).collect()
}

/// (code, why) of drive's refused start of queued issue `n` in the last hour: `skipped this pass` (status, board).
pub fn skipped<'a>(events: &'a [Event], st: &States, fleet: &str, n: u64, now: u64) -> Option<(&'a str, &'a str)> {
    let s = |e: &&Event| e.kind == "driven" && e.fleet == fleet && e.issue == Some(n) && e.str("step") == Some("start");
    (state(st, fleet, n) == "queued").then_some(())?;
    let e = events.iter().rev().find(s).filter(|e| now < e.ts + 3600)?;
    Some((e.str("code")?, e.str("result")?.split_once(": ").map_or("", |w| w.1)))
}

pub fn ls_json(events: &[Event], fleet: &str) -> Value {
    let st = fold(events);
    let row = |(k, i): (usize, &Item)| json!({"pos": k + 1, "issue": i.issue, "state": state(&st, fleet, i.issue), "blocked_by": i.blocked_by, "serial": !i.serial.is_empty(), "needs": i.needs});
    Value::Array(view(events, &st, fleet).iter().enumerate().map(row).collect())
}

pub const WIDTH: usize = 60;

/// One fleet's board, lines ≤ 60 cols: the queue, other open issues, issues merged in the last 24 h.
pub fn board(events: &[Event], fleet: &str, now: u64, cfg: Option<&crate::config::Config>, c: &Conflicts) -> String {
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
        let skip = skipped(events, &st, fleet, n, now).map(|(c, _)| format!("skip {c}"));
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
        let last = last.unwrap_or_else(|| if ev.is_empty() { "-".into() } else { ev.into() });
        format!(
            "{pos:>2} #{n:<5} {:<11} {lane:<20} {eff:<5} {a:<4} {last}{warn}",
            state(&st, fleet, n)
        )
    };
    for (k, i) in q.iter().enumerate() {
        lines.push(row(
            (k + 1).to_string(),
            i.issue,
            i.blocked_by.map(|b| format!("on #{b}")),
        ));
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
