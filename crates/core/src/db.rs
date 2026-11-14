use crate::events::{self, Event};
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{de::DeserializeOwned, Serialize};
use std::cell::RefCell;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS events (seq INTEGER PRIMARY KEY, ts INTEGER NOT NULL, fleet TEXT NOT NULL, issue INTEGER,
    type TEXT NOT NULL, json TEXT NOT NULL, archived INTEGER NOT NULL DEFAULT 0);
CREATE INDEX IF NOT EXISTS events_issue ON events (fleet, issue);
CREATE INDEX IF NOT EXISTS events_type ON events (type);
CREATE TABLE IF NOT EXISTS leases (key TEXT PRIMARY KEY, pid INTEGER NOT NULL, host TEXT NOT NULL, at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS devices (n INTEGER PRIMARY KEY, json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS push (n INTEGER PRIMARY KEY, json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS snapshot (n INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT UNIQUE NOT NULL, ts INTEGER NOT NULL,
    json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS processed (id TEXT PRIMARY KEY, at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS issues (fleet TEXT NOT NULL, issue INTEGER NOT NULL, json TEXT NOT NULL,
    PRIMARY KEY (fleet, issue));
CREATE TABLE IF NOT EXISTS lanes (agent TEXT PRIMARY KEY, seq INTEGER NOT NULL, json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS asks (seq INTEGER PRIMARY KEY, fleet TEXT NOT NULL, id TEXT, issue INTEGER, agent TEXT,
    twin TEXT, json TEXT NOT NULL, rank INTEGER, closed TEXT, at INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS asks_twin ON asks (twin);
CREATE INDEX IF NOT EXISTS events_started ON events (json_extract(json, '$.agent'), json_extract(json, '$.pane'))
    WHERE type = 'spawn-started';";

pub fn file(state: &Path) -> PathBuf {
    SCRATCH
        .with_borrow(Clone::clone)
        .unwrap_or_else(|| state.with_file_name("state.db"))
}

thread_local! {
    static SCRATCH: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// #344: until dropped, this thread's database is a new temporary one; the files beside the log are imported into it once and left where they are.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn start() -> Scratch {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("drover-scratch-{}-{n}.db", std::process::id()));
        SCRATCH.set(Some(p.clone()));
        Scratch(p)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        SCRATCH.set(None);
        OPEN.with_borrow_mut(|v| v.retain(|x| x.0 != self.0));
        for x in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{x}", self.0.display()));
        }
    }
}

thread_local! {
    static OPEN: RefCell<Vec<(PathBuf, u64, Connection)>> = const { RefCell::new(Vec::new()) };
}

fn with<T>(state: &Path, create: bool, empty: T, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
    let (p, (logs, lists)) = (file(state), legacy(state));
    let ino = |p: &Path| std::fs::metadata(p).map(|m| m.ino()).ok();
    let old = !logs.is_empty() || lists.iter().any(|p| p.exists());
    if !create && ino(&p).is_none() && !old {
        return Ok(empty);
    }
    let scratch = SCRATCH.with_borrow(Option::is_some);
    anyhow::ensure!(
        !old || scratch || !live(state) || LIVE.load(std::sync::atomic::Ordering::Relaxed),
        "{} still has files from before #344: run `drover migrate --live` to import them",
        state.parent().unwrap_or(Path::new(".")).display()
    );
    let cached = OPEN.with_borrow_mut(|v| Some(v.swap_remove(v.iter().position(|x| x.0 == p)?)));
    let (mut c, fresh) = match cached {
        Some((_, i, c)) if ino(&p) == Some(i) => (c, false),
        _ => (open(state)?, true),
    };
    if (fresh || !scratch) && old {
        import(&mut c, state)?;
    }
    let r = f(&mut c);
    if let Some(i) = ino(&p) {
        OPEN.with_borrow_mut(|v| v.push((p, i, c)));
    }
    r
}

fn open(state: &Path) -> Result<Connection> {
    std::fs::create_dir_all(state.parent().unwrap_or(Path::new(".")))?;
    let mut o = std::fs::OpenOptions::new();
    drop(std::os::unix::fs::OpenOptionsExt::mode(o.create(true).append(true), 0o600).open(file(state))?);
    let c = Connection::open(file(state)).with_context(|| format!("cannot open {}", file(state).display()))?;
    let l = crate::config::limits();
    c.busy_timeout(std::time::Duration::from_secs(l.db_busy_secs))?;
    let mode = |c: &Connection| c.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0));
    for _ in 0..6000 {
        if mode(&c)? == "wal" || c.pragma_update(None, "journal_mode", "WAL").is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(l.db_retry_millis));
    }
    anyhow::ensure!(mode(&c)? == "wal", "{} is not in WAL mode", file(state).display());
    c.execute_batch(SCHEMA)?;
    let mut c = c;
    if c.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))? < 1 {
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))? < 1 {
            offload_all(&tx, state)?;
            rebuild_in(&tx, state)?;
            tx.pragma_update(None, "user_version", 1)?;
        }
        tx.commit()?;
    }
    Ok(c)
}

fn offload_all(c: &Connection, state: &Path) -> Result<()> {
    let big = rows(
        c,
        "type = 'build-cloned' AND json_extract(json, '$.errors[0]') IS NOT NULL AND seq > ?1",
        0,
    )?;
    for e in big {
        let data = events::offload(state, e.data)?;
        let json = serde_json::to_string(&Event { seq: 0, data, ..e })?;
        c.execute("UPDATE events SET json = ?2 WHERE seq = ?1", params![e.seq, json])?;
    }
    Ok(())
}

fn insert(c: &Connection, ev: &Event, seq: Option<u64>, archived: bool) -> Result<Event> {
    let mut ev = Event { seq: 0, ..ev.clone() };
    if let Some(id) = crate::orchestrator::acting() {
        ev.data.entry("orchestrator").or_insert(id.into());
    }
    let ev = if seq.is_none() { crate::events::capped(&ev) } else { ev };
    let json = serde_json::to_string(&ev)?;
    c.execute(
        "INSERT INTO events (seq, ts, fleet, issue, type, json, archived) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![seq, ev.ts, ev.fleet, ev.issue, ev.kind, json, archived],
    )?;
    Ok(Event {
        seq: c.last_insert_rowid().try_into()?,
        ..crate::events::aliased(ev)
    })
}

fn log(c: &Connection, ev: &Event) -> Result<()> {
    project(c, &insert(c, ev, None, false)?)
}

fn project(c: &Connection, ev: &Event) -> Result<()> {
    if let Some(n) = ev.issue {
        let q = "SELECT json FROM issues WHERE fleet = ?1 AND issue = ?2";
        let old: Option<String> = c.query_row(q, params![ev.fleet, n], |r| r.get(0)).optional()?;
        if old.is_none() || !events::PASSIVE.contains(&ev.kind.as_str()) {
            let key = (ev.fleet.clone(), n);
            let mut m = events::States::new();
            if let Some(j) = old {
                m.insert(key.clone(), serde_json::from_str(&j)?);
            }
            let s = serde_json::to_string(&events::fold_into(m, std::slice::from_ref(ev))[&key])?;
            let q = "INSERT OR REPLACE INTO issues (fleet, issue, json) VALUES (?1, ?2, ?3)";
            c.execute(q, params![ev.fleet, n, s])?;
        }
    }
    if let (true, Some(a)) = (ev.kind == "spawn", ev.str("agent")) {
        let q = "INSERT OR REPLACE INTO lanes (agent, seq, json) VALUES (?1, ?2, ?3)";
        c.execute(q, params![a, ev.seq, serde_json::to_string(ev)?])?;
    }
    project_ask(c, ev)
}

fn project_ask(c: &Connection, e: &Event) -> Result<()> {
    use rusqlite::types::Value as Sql;
    let text = |k: &str| e.str(k).map_or(Sql::Null, |v| Sql::Text(v.into()));
    if e.kind == "needs-decision" {
        let twin = events::twin(e);
        let q = "SELECT COUNT(*) FROM asks WHERE twin = ?1";
        let repeat = twin.is_some() && c.query_row(q, [&twin], |r| r.get::<_, u64>(0))? > 0;
        let q = "INSERT INTO asks (seq, fleet, id, issue, agent, twin, json, rank, closed, at) \
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?1)";
        let (rank, why) = (repeat.then_some(events::ORDER.len() - 1), repeat.then_some("repeat"));
        let row = (
            e.seq,
            &e.fleet,
            text("id"),
            e.issue,
            text("agent"),
            twin,
            serde_json::to_string(e)?,
            rank,
            why,
        );
        c.execute(q, row)?;
        return Ok(());
    }
    if !events::ends_asks(&e.kind) {
        return Ok(());
    }
    let issue = e.issue.and_then(|n| i64::try_from(n).ok()).map(Sql::Integer);
    let (col, key, floor) = match e.kind.as_str() {
        "decision" | "ask-withdrawn" | "ask-delivered" | "ask-failed" | "ask-expired" => ("id", text("id"), 0),
        "merged" | "closed" | "closed-other" | "clean" if issue.is_some() => ("issue", issue.unwrap_or(Sql::Null), 5),
        "spawn" | "lane-killed" => ("agent", text("agent"), 7),
        _ => return Ok(()),
    };
    let q = format!("SELECT seq, json, rank FROM asks WHERE fleet = ?1 AND {col} IS ?2 AND (rank IS NULL OR rank BETWEEN ?3 AND ?4)");
    let found: Vec<(u64, String, Option<usize>)> = {
        let mut q = c.prepare(&q)?;
        let all = q.query_map(params![e.fleet, key, floor + 1, events::ORDER.len() - 2], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        all.collect::<rusqlite::Result<_>>()?
    };
    let q = "SELECT MIN(seq) FROM events WHERE type = 'spawn-started' AND json_extract(json, '$.agent') IS ?1 \
        AND json_extract(json, '$.pane') IS ?2 AND seq < ?3";
    let first = || {
        c.query_row(q, params![text("agent"), text("pane"), e.seq], |r| {
            r.get::<_, Option<u64>>(0)
        })
    };
    for (seq, json, rank) in found {
        let a = numbered(seq, &json)?;
        let w = events::closes(&a, e, || first().ok().flatten().is_some_and(|f| f < seq));
        let Some(r) = events::ORDER.iter().position(|o| Some(*o) == w) else {
            continue;
        };
        if r < rank.unwrap_or(events::ORDER.len()) {
            let row = events::ending_row(events::ORDER[r], e);
            let set = "UPDATE asks SET rank = ?2, closed = ?3, at = ?4 WHERE seq = ?1";
            c.execute(set, params![seq, r, row, e.seq])?;
        }
    }
    Ok(())
}

fn numbered(seq: u64, json: &str) -> Result<Event> {
    Ok(Event {
        seq,
        ..serde_json::from_str(json)?
    })
}

fn projected(evs: &[Event]) -> Result<Connection> {
    let c = Connection::open_in_memory()?;
    c.execute_batch(SCHEMA)?;
    let tx = c.unchecked_transaction()?;
    for e in evs
        .iter()
        .filter(|e| e.kind == "spawn-started" || events::ends_asks(&e.kind))
    {
        project_ask(&tx, &insert(&tx, e, Some(e.seq), false)?)?;
    }
    tx.commit()?;
    Ok(c)
}

fn rebuild_in(c: &Connection, state: &Path) -> Result<()> {
    let mut all = [archived(state)?, rows(c, "seq > ?1", 0)?].concat();
    all.sort_by_key(|e| e.seq);
    all.dedup_by_key(|e| e.seq);
    c.execute_batch("DELETE FROM issues; DELETE FROM lanes; DELETE FROM asks;")?;
    let q = "INSERT INTO issues (fleet, issue, json) VALUES (?1, ?2, ?3)";
    for s in events::fold_into(events::States::new(), &all).into_values() {
        c.execute(q, params![s.fleet, s.issue, serde_json::to_string(&s)?])?;
    }
    let q = "INSERT OR REPLACE INTO lanes (agent, seq, json) VALUES (?1, ?2, ?3)";
    for e in all.iter().filter(|e| e.kind == "spawn") {
        if let Some(a) = e.str("agent") {
            c.execute(q, params![a, e.seq, serde_json::to_string(e)?])?;
        }
    }
    let m = projected(&all)?;
    let mut q = m.prepare("SELECT * FROM asks")?;
    let mut ins = c.prepare("INSERT INTO asks VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)")?;
    let mut rs = q.query([])?;
    while let Some(r) = rs.next()? {
        let v = (0..10).map(|i| r.get::<_, rusqlite::types::Value>(i));
        ins.execute(rusqlite::params_from_iter(v.collect::<rusqlite::Result<Vec<_>>>()?))?;
    }
    Ok(())
}

/// #978: `drover compact --rebuild`: the current-state rows made again from every event, in one transaction.
pub fn rebuild(state: &Path) -> Result<()> {
    with(state, true, (), |c| {
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        rebuild_in(&tx, state)?;
        Ok(tx.commit()?)
    })
}

/// #978: the current per-issue state, the `issues` rows.
pub fn issues(state: &Path) -> Result<events::States> {
    since(state, i64::MAX as u64, true).map(|t| t.1)
}

/// #978: an ask's ending as its `asks` row holds it: (`ORDER` rank, `drover asks`' text); `None` while open.
pub type Ending = Option<(usize, String)>;
pub type Endings = std::collections::BTreeMap<u64, Ending>;

pub fn endings(evs: &[Event]) -> Result<Endings> {
    let evs: Vec<Event> = evs.iter().zip(1..).map(|(e, seq)| Event { seq, ..e.clone() }).collect();
    asks_since(&projected(&evs)?, 0)
}

fn asks_since(c: &Connection, at: u64) -> Result<Endings> {
    let mut q = c.prepare("SELECT seq, rank, closed FROM asks WHERE at > ?1")?;
    let rows = q.query_map([at], |r| {
        let (rank, why) = (r.get::<_, Option<usize>>(1)?, r.get::<_, Option<String>>(2)?);
        Ok((r.get::<_, u64>(0)?, rank.zip(why)))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn since(state: &Path, seq: u64, fresh: bool) -> Result<(Vec<Event>, events::States, Endings)> {
    with(state, false, Default::default(), |c| {
        let tx = c.transaction()?;
        let evs = rows(&tx, "archived = 0 AND seq > ?1", seq)?;
        let q =
            "SELECT json FROM issues WHERE ?1 OR (fleet, issue) IN (SELECT fleet, issue FROM events WHERE seq > ?2)";
        let mut q = tx.prepare(q)?;
        let st = q.query_map(params![fresh, seq], |r| r.get::<_, String>(0))?.map(|j| {
            let s: events::IssueState = serde_json::from_str(&j?)?;
            Ok(((s.fleet.clone(), s.issue), s))
        });
        let st = st.collect::<Result<events::States>>()?;
        drop(q);
        let asks = asks_since(&tx, if fresh { 0 } else { seq })?;
        Ok((evs, st, asks))
    })
}

/// #978: `drover asks`' rows: every ask (`fleet`'s) with its ending, oldest first.
pub fn asks(state: &Path, fleet: Option<&str>) -> Result<Vec<(Event, Ending)>> {
    with(state, false, vec![], |c| {
        let q = "SELECT seq, json, rank, closed FROM asks WHERE ?1 IS NULL OR fleet = ?1 ORDER BY seq";
        let mut q = c.prepare(q)?;
        let rows = q.query_map([fleet], |r| {
            let end = r.get::<_, Option<usize>>(2)?.zip(r.get::<_, Option<String>>(3)?);
            Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?, end))
        })?;
        rows.map(|r| {
            r.map_err(Into::into)
                .and_then(|(seq, j, end)| Ok((numbered(seq, &j)?, end)))
        })
        .collect()
    })
}

const HEARTBEAT_SECS: u64 = 2 * 86_400;
const HOT_SECS: u64 = 14 * 86_400;

pub fn compact(state: &Path, now: u64) -> Result<(usize, usize)> {
    let dir = state.with_file_name("archive");
    std::fs::create_dir_all(&dir)?;
    let done = "ts < ?1 AND EXISTS (SELECT 1 FROM issues i WHERE i.fleet = events.fleet AND i.issue = events.issue \
        AND json_extract(i.json, '$.state') IN ('merged', 'closed'))";
    let n = with(state, true, (0, 0), |c| {
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let q = "DELETE FROM events WHERE type IN ('drive-pass', 'driven', 'kick') AND ts < ?1";
        let gone = tx.execute(q, [now.saturating_sub(HEARTBEAT_SECS)])?;
        let cut = now.saturating_sub(HOT_SECS);
        let old = rows(&tx, done, cut)?;
        let mut months = std::collections::BTreeMap::<String, String>::new();
        for e in &old {
            let m = crate::lesson::ymd(e.ts / 86_400)[..7].to_string();
            *months.entry(m).or_default() += &(serde_json::to_string(e)? + "\n");
        }
        for (m, text) in months {
            zstd_append(&dir.join(format!("events-{m}.jsonl.zst")), &text)?;
        }
        tx.execute(&format!("DELETE FROM events WHERE {done}"), [cut])?;
        let q = "INSERT OR REPLACE INTO snapshot (key, ts, json) VALUES ('archived', ?1, '{}')";
        tx.execute(q, [now])?;
        tx.commit()?;
        if gone + old.len() > 0 {
            c.execute_batch("VACUUM")?;
        }
        c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok((gone, old.len()))
    })?;
    Ok(n)
}

fn zstd_append(file: &Path, text: &str) -> Result<()> {
    use std::io::Write as _;
    let (src, part) = (file.with_extension("new"), file.with_extension("part"));
    std::fs::write(&src, text)?;
    let (s, p) = (src.to_string_lossy(), part.to_string_lossy());
    crate::runner::ok(&crate::runner::Real, "zstd", &["-qf", "--rm", &s, "-o", &p])?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(file)?;
    f.write_all(&std::fs::read(&part)?)?;
    Ok(std::fs::remove_file(part)?)
}

/// #978: every event `compact` moved to `archive/`, by seq.
pub fn archived(state: &Path) -> Result<Vec<Event>> {
    let dir = std::fs::read_dir(state.with_file_name("archive"));
    let mut files: Vec<PathBuf> = dir.into_iter().flatten().filter_map(|e| Some(e.ok()?.path())).collect();
    files.retain(|p| p.extension().is_some_and(|x| x == "zst"));
    files.sort();
    let mut all = vec![];
    for f in files {
        let text = crate::runner::ok(&crate::runner::Real, "zstd", &["-dcq", &f.to_string_lossy()])?;
        all.extend(events::parse(&text, &f));
    }
    all.sort_by_key(|e| e.seq);
    Ok(all)
}

/// One event, numbered next, with the rows it changes: one transaction.
pub fn append(state: &Path, ev: &Event) -> Result<()> {
    crate::sim::point(&crate::sim::Point::Append(ev));
    with(state, true, (), |c| {
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        log(&tx, ev)?;
        Ok(tx.commit()?)
    })
}

/// #900, #906: `evs` in one transaction, each `needs-decision` under the highest `q<n>` logged plus 1; none if an ask keyed `once` is logged.
pub fn ask(state: &Path, mut evs: Vec<Event>, once: Option<&str>) -> Result<Option<Vec<Event>>> {
    for ev in &evs {
        crate::sim::point(&crate::sim::Point::Append(ev));
    }
    with(state, true, None, |c| {
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let count = |w: &str, p: &[&str]| {
            let q = format!("SELECT COUNT(*) FROM events WHERE type = 'needs-decision'{w}");
            tx.query_row(&q, rusqlite::params_from_iter(p), |r| r.get::<_, u64>(0))
        };
        if let Some(k) = once {
            if count(" AND json_extract(json, '$.once') = ?1", &[k])? > 0 {
                return Ok(None);
            }
        }
        let q = "SELECT COALESCE(MAX(CAST(SUBSTR(json_extract(json, '$.id'), 2) AS INTEGER)), 0) FROM events \
            WHERE type = 'needs-decision' AND json_extract(json, '$.id') GLOB 'q[0-9]*'";
        let mut n: u64 = tx.query_row(q, [], |r| r.get(0))?;
        for ev in evs.iter_mut().filter(|e| e.kind == "needs-decision") {
            n += 1;
            ev.data.insert("id".into(), format!("q{n}").into());
            if let Some(k) = once {
                ev.data.insert("once".into(), k.into());
            }
        }
        for ev in &evs {
            log(&tx, ev)?;
        }
        tx.commit()?;
        Ok(Some(evs))
    })
}

/// #935: fleet `old`'s events moved to `new` and `ev` logged, after a copy to `bak`; committed only once `then` succeeds.
pub fn rename_fleet(
    state: &Path,
    (old, new): (&str, &str),
    ev: &Event,
    bak: &Path,
    then: impl FnOnce() -> Result<()>,
) -> Result<usize> {
    with(state, true, 0, |c| {
        c.execute("VACUUM INTO ?1", [bak.to_string_lossy()])?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let q = "UPDATE events SET fleet = ?2, json = json_set(json, '$.fleet', ?2) WHERE fleet = ?1";
        let n = tx.execute(q, params![old, new])?;
        for t in ["issues", "asks"] {
            let q = format!("UPDATE {t} SET fleet = ?2, json = json_set(json, '$.fleet', ?2) WHERE fleet = ?1");
            tx.execute(&q, params![old, new])?;
        }
        let q = "UPDATE lanes SET json = json_set(json, '$.fleet', ?2) WHERE json_extract(json, '$.fleet') = ?1";
        tx.execute(q, params![old, new])?;
        tx.execute("DELETE FROM snapshot WHERE key IN ('states', 'board')", [])?;
        tx.execute(
            "INSERT OR REPLACE INTO snapshot (key, ts, json) VALUES ('archived', 0, '{}')",
            [],
        )?;
        log(&tx, ev)?;
        then()?;
        tx.commit()?;
        Ok(n)
    })
}

pub(crate) fn events(state: &Path, filter: &str, arg: u64) -> Result<Vec<Event>> {
    with(state, false, vec![], |c| rows(c, filter, arg))
}

/// #701: event rows read from the database (a full read's cost is the log's length).
pub static ROWS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn rows(c: &Connection, filter: &str, arg: u64) -> Result<Vec<Event>> {
    let mut q = c.prepare(&format!("SELECT seq, json FROM events WHERE {filter} ORDER BY seq"))?;
    let rows = q.query_map([arg], |r| Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?)))?;
    rows.map(|r| {
        let (seq, json) = r?;
        ROWS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(crate::events::aliased(numbered(seq, &json)?))
    })
    .collect()
}

/// #509: in one transaction, the latest `intent`/`done`/`failed` of `action_id` `id`; `evs` appended when `take` accepts it. Returns it and whether they were.
pub fn claim(
    state: &Path,
    evs: &[Event],
    id: &str,
    take: &dyn Fn(Option<&Event>) -> bool,
) -> Result<(Option<Event>, bool)> {
    crate::sim::point(&crate::sim::Point::Lease(id));
    with(state, true, (None, false), |c| {
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let q = "type IN ('intent', 'done', 'failed') AND json_extract(json, '$.action_id') = ?1";
        let mut q = tx.prepare(&format!(
            "SELECT seq, json FROM events WHERE {q} ORDER BY seq DESC LIMIT 1"
        ))?;
        let last = q
            .query_row([id], |r| Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?)))
            .optional()?;
        drop(q);
        let last = last
            .map(|(seq, j)| {
                anyhow::Ok(Event {
                    seq,
                    ..serde_json::from_str(&j)?
                })
            })
            .transpose()?;
        let took = take(last.as_ref());
        for ev in evs.iter().filter(|_| took) {
            log(&tx, ev)?;
        }
        tx.commit()?;
        Ok((last, took))
    })
}

/// #509: mark trigger `id` processed; false if it already was.
pub fn processed(state: &Path, id: &str, now: u64) -> Result<bool> {
    let q = "INSERT OR IGNORE INTO processed (id, at) VALUES (?1, ?2)";
    with(state, true, 0, |c| Ok(c.execute(q, params![id, now])?)).map(|n| n > 0)
}

/// A snapshot row (`board`: #337's reads).
pub fn get<T: DeserializeOwned>(state: &Path, key: &str) -> Option<(u64, T)> {
    let q = "SELECT ts, json FROM snapshot WHERE key = ?1";
    let row = |r: &rusqlite::Row| Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?));
    let (ts, json) = with(state, false, None, |c| Ok(c.query_row(q, [key], row).optional()?)).ok()??;
    Some((ts, serde_json::from_str(&json).ok()?))
}

/// A snapshot row's version: a new one each `put`.
pub fn stamp(state: &Path, key: &str) -> Option<u64> {
    let q = "SELECT n FROM snapshot WHERE key = ?1";
    with(state, false, None, |c| {
        Ok(c.query_row(q, [key], |r| r.get(0)).optional()?)
    })
    .ok()?
}

/// #560: the database file's inode and its last compaction's stamp: a change makes `events::shared` read afresh.
pub fn version(state: &Path) -> (Option<u64>, Option<u64>) {
    let ino = std::fs::metadata(file(state)).map(|m| m.ino()).ok();
    (ino, ino.and_then(|_| stamp(state, "archived")))
}

/// The newest event's seq (0: none): the page's change check.
pub fn last_seq(state: &Path) -> u64 {
    let q = "SELECT COALESCE(MAX(seq), 0) FROM events";
    with(state, false, 0, |c| Ok(c.query_row(q, [], |r| r.get(0))?)).unwrap_or(0)
}

pub fn put(state: &Path, key: &str, ts: u64, v: &impl Serialize) -> Result<()> {
    let q = "INSERT OR REPLACE INTO snapshot (key, ts, json) VALUES (?1, ?2, ?3)";
    let json = serde_json::to_string(v)?;
    with(state, true, 0, |c| Ok(c.execute(q, params![key, ts, json])?)).map(|_| ())
}

/// The `devices` or `push` rows, in order.
pub fn list<T: DeserializeOwned>(state: &Path, table: &str) -> Vec<T> {
    try_list(state, table).unwrap_or_default()
}

/// #501: `list`, its database error kept (a busy or unreadable `state.db` is no empty table).
pub fn try_list<T: DeserializeOwned>(state: &Path, table: &str) -> Result<Vec<T>> {
    let all = with(state, false, vec![], |c| {
        let mut q = c.prepare(&format!("SELECT json FROM {table} ORDER BY n"))?;
        let rows = q.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    })?;
    Ok(all.into_iter().filter_map(|j| serde_json::from_str(&j).ok()).collect())
}

/// Replace every row of `table` with `all`, in one transaction.
pub fn store<T: Serialize>(state: &Path, table: &str, all: &[T]) -> Result<()> {
    with(state, true, (), |c| replace(c, table, all))
}

/// #893: `store`, failing at once while another connection holds the write lock (never waiting for it).
pub fn store_now<T: Serialize>(state: &Path, table: &str, all: &[T]) -> Result<()> {
    with(state, true, (), |c| {
        c.busy_timeout(std::time::Duration::ZERO)?;
        let r = replace(c, table, all);
        c.busy_timeout(std::time::Duration::from_secs(crate::config::limits().db_busy_secs))?;
        r
    })
}

fn replace<T: Serialize>(c: &mut Connection, table: &str, all: &[T]) -> Result<()> {
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(&format!("DELETE FROM {table}"), [])?;
    for v in all {
        tx.execute(
            &format!("INSERT INTO {table} (json) VALUES (?1)"),
            [serde_json::to_string(v)?],
        )?;
    }
    Ok(tx.commit()?)
}

/// A lease: who holds `key` (pid on host), since `at`, until `expires_at`, with the holder's `data`.
#[derive(Debug, Clone)]
pub struct Lease {
    pub key: String,
    pub pid: u32,
    pub host: String,
    pub at: u64,
    pub expires_at: u64,
    pub data: String,
}

/// This machine's name, as leases record their holder.
pub fn host() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| {
            std::fs::read_to_string("/etc/hostname").map_or_else(|_| "localhost".into(), |h| h.trim().into())
        })
}

/// Every lease whose key starts with `prefix`.
pub fn leases(state: &Path, prefix: &str) -> Vec<Lease> {
    let q = format!("{LEASE} WHERE substr(key, 1, length(?1)) = ?1 ORDER BY key");
    let all = with(state, false, vec![], |c| {
        let mut q = c.prepare(&q)?;
        let rows = q.query_map([prefix], lease)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    });
    all.unwrap_or_default()
}

const LEASE: &str = "SELECT key, pid, host, at, expires_at, data FROM leases";

fn lease(r: &rusqlite::Row) -> rusqlite::Result<Lease> {
    Ok(Lease {
        key: r.get(0)?,
        pid: r.get(1)?,
        host: r.get(2)?,
        at: r.get(3)?,
        expires_at: r.get(4)?,
        data: r.get(5)?,
    })
}

/// Take `key` for `secs` from `now`, as this process: an atomic insert-if-free. Free: no lease, an expired one, or one `dead` says has lost its holder. `None` while another holds it.
pub fn take(state: &Path, key: &str, now: u64, secs: u64, data: &str, dead: &dyn Fn(&Lease) -> bool) -> Option<Lease> {
    crate::sim::point(&crate::sim::Point::Lease(key));
    let l = Lease {
        key: key.into(),
        pid: crate::sim::pid(),
        host: host(),
        at: now,
        expires_at: now + secs,
        data: data.into(),
    };
    let q = "INSERT OR REPLACE INTO leases (key, pid, host, at, expires_at, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)";
    let took = with(state, true, false, |c| {
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let held = tx
            .query_row(&format!("{LEASE} WHERE key = ?1"), [key], lease)
            .optional()?;
        if held.is_some_and(|h| h.expires_at > now && !dead(&h)) {
            return Ok(false);
        }
        tx.execute(q, params![l.key, l.pid, l.host, l.at, l.expires_at, l.data])?;
        Ok(tx.commit().map(|()| true)?)
    });
    took.unwrap_or(false).then_some(l)
}

/// Hand lease `old` on as `new` (a later expiry, another pid or data); false if `old` no longer holds it.
pub fn renew(state: &Path, old: &Lease, new: &Lease) -> bool {
    let q = "UPDATE leases SET pid = ?1, at = ?2, expires_at = ?3, data = ?4 WHERE key = ?5 AND pid = ?6 AND host = ?7 AND at = ?8";
    let p = params![
        new.pid,
        new.at,
        new.expires_at,
        new.data,
        old.key,
        old.pid,
        old.host,
        old.at
    ];
    with(state, false, 0, |c| Ok(c.execute(q, p)?)).is_ok_and(|n| n > 0)
}

/// Drop `key`, whoever holds it (`Some(l)`: only while `l` does); true if a lease went.
pub fn release(state: &Path, key: &str, l: Option<&Lease>) -> bool {
    crate::sim::point(&crate::sim::Point::Lease(key));
    let n = with(state, false, 0, |c| {
        Ok(match l {
            None => c.execute("DELETE FROM leases WHERE key = ?1", [key])?,
            Some(l) => c.execute(
                "DELETE FROM leases WHERE key = ?1 AND pid = ?2 AND host = ?3 AND at = ?4",
                params![key, l.pid, l.host, l.at],
            )?,
        })
    });
    n.is_ok_and(|n| n > 0)
}

fn retire(p: &Path) -> Result<()> {
    let name = p.file_name().unwrap_or_default().to_string_lossy();
    let free = (1..u32::MAX)
        .map(|k| match k {
            1 => p.with_file_name(format!("{name}.migrated")),
            k => p.with_file_name(format!("{name}.{k}.migrated")),
        })
        .find(|t| !t.exists())
        .unwrap_or_default();
    Ok(std::fs::rename(p, free)?)
}

fn legacy(state: &Path) -> (Vec<PathBuf>, [PathBuf; 2]) {
    let dir = state.parent().unwrap_or(Path::new("."));
    let mut logs: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    logs.retain(|p| {
        p.file_name()
            .is_some_and(|n| n.to_string_lossy().ends_with(".jsonl.gz"))
            || p == state
    });
    (logs, [dir.join("serve/devices.json"), dir.join("webpush/subs.json")])
}

/// Set by `drover migrate --live`: the only way files in the owner's own state dir (`live`) are imported.
pub static LIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The owner's default state dir, `$HOME/.local/state/drover` (or its pre-rename `herdsman`): never imported on its own, so a test build run without `FLEET_STATE` cannot take the installed build's log away.
pub fn live(state: &Path) -> bool {
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let dir = canon(state.parent().unwrap_or(Path::new(".")));
    let root = crate::config::home().join(".local/state");
    ["drover", "herdsman"].iter().any(|d| canon(&root.join(d)) == dir)
}

fn import(c: &mut Connection, state: &Path) -> Result<()> {
    let (logs, lists) = legacy(state);
    if logs.is_empty() && !lists.iter().any(|p| p.exists()) {
        return Ok(());
    }
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (logs, lists) = legacy(state);
    let mut done = vec![];
    if !logs.is_empty() {
        let live = events::read_file(state)?;
        let mut all = [events::archives(state)?, live.clone()].concat();
        all.sort_by_key(|e| e.seq);
        all.dedup_by_key(|e| e.seq);
        let mut seen = rows(&tx, "seq > ?1", 0)?;
        let mut max = seen.last().map_or(0, |e| e.seq);
        for e in &all {
            let seq = if e.seq > max { e.seq } else { max + 1 };
            let archived = live.binary_search_by_key(&e.seq, |l| l.seq).is_err();
            insert(&tx, e, Some(seq), archived)?;
            max = seq;
        }
        seen.extend(all);
        let fold = |ev: &[Event]| {
            serde_json::to_value(
                events::fold_into(events::States::new(), ev)
                    .into_values()
                    .collect::<Vec<_>>(),
            )
            .ok()
        };
        anyhow::ensure!(
            fold(&seen) == fold(&rows(&tx, "seq > ?1", 0)?),
            "import of {} changes the fold; nothing imported",
            state.display()
        );
        done.extend(logs);
    }
    for (p, table) in lists.iter().zip(["devices", "push"]) {
        let Ok(text) = std::fs::read_to_string(p) else { continue };
        let all: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap_or_default();
        for v in all {
            tx.execute(&format!("INSERT INTO {table} (json) VALUES (?1)"), [v.to_string()])?;
        }
        done.push(p.clone());
    }
    for p in done.iter().filter(|_| SCRATCH.with_borrow(Option::is_none)) {
        retire(p)?;
    }
    rebuild_in(&tx, state)?;
    tx.commit().context("import commit")
}

/// `drover migrate`'s #344 step: import what is left, say what the database holds.
pub fn migrate(state: &Path, out: &mut dyn std::io::Write) -> Result<()> {
    let left = legacy(state).0.len();
    let n: u64 = with(state, true, 0, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?)
    })?;
    writeln!(
        out,
        "imported {left} log file(s); {} holds {n} events",
        file(state).display()
    )?;
    Ok(())
}
