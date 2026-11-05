//! #344: herdsman's state in one SQLite file, `state.db` beside the log path (`FLEET_STATE`), in WAL mode: the event
//! log, leases (every lock and marker), paired devices, push subscriptions and the snapshots (docs/architecture.md).
use crate::events::{self, Event};
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{de::DeserializeOwned, Serialize};
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
    json TEXT NOT NULL);";

pub fn file(state: &Path) -> PathBuf {
    state.with_file_name("state.db")
}

/// The database, created on first use; files from before #344 left beside it are imported first (`import`).
pub fn open(state: &Path) -> Result<Connection> {
    std::fs::create_dir_all(state.parent().unwrap_or(Path::new(".")))?;
    // Paired devices and push keys live here: owner-only, as their files were.
    let mut o = std::fs::OpenOptions::new();
    drop(std::os::unix::fs::OpenOptionsExt::mode(o.create(true).append(true), 0o600).open(file(state))?);
    let mut c = Connection::open(file(state)).with_context(|| format!("cannot open {}", file(state).display()))?;
    c.busy_timeout(std::time::Duration::from_secs(60))?;
    // Switching to WAL needs the file alone and SQLite does not wait for it: a new database's first openers retry.
    let mode = |c: &Connection| c.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0));
    for _ in 0..6000 {
        if mode(&c)? == "wal" || c.pragma_update(None, "journal_mode", "WAL").is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    anyhow::ensure!(mode(&c)? == "wal", "{} is not in WAL mode", file(state).display());
    c.execute_batch(SCHEMA)?;
    import(&mut c, state)?;
    Ok(c)
}

/// A reader's database: none yet, and nothing to import, reads as empty and creates nothing.
fn peek(state: &Path) -> Result<Option<Connection>> {
    let (logs, lists) = legacy(state);
    if !file(state).exists() && logs.is_empty() && !lists.iter().any(|p| p.exists()) {
        return Ok(None);
    }
    open(state).map(Some)
}

fn insert(c: &Connection, ev: &Event, seq: Option<u64>, archived: bool) -> Result<()> {
    let json = serde_json::to_string(&Event { seq: 0, ..ev.clone() })?;
    c.execute(
        "INSERT INTO events (seq, ts, fleet, issue, type, json, archived) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![seq, ev.ts, ev.fleet, ev.issue, ev.kind, json, archived],
    )?;
    Ok(())
}

/// One event, numbered next: a single insert, its own transaction.
pub fn append(state: &Path, ev: &Event) -> Result<()> {
    insert(&open(state)?, ev, None, false)
}

/// The events `filter` (an SQL condition over the columns, `?1` = `arg`) selects, by seq.
pub(crate) fn events(state: &Path, filter: &str, arg: u64) -> Result<Vec<Event>> {
    peek(state)?.map_or(Ok(vec![]), |c| rows(&c, filter, arg))
}

fn rows(c: &Connection, filter: &str, arg: u64) -> Result<Vec<Event>> {
    let mut q = c.prepare(&format!("SELECT seq, json FROM events WHERE {filter} ORDER BY seq"))?;
    let rows = q.query_map([arg], |r| Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?)))?;
    rows.map(|r| {
        let (seq, json) = r?;
        Ok(Event {
            seq,
            ..serde_json::from_str(&json)?
        })
    })
    .collect()
}

/// #343: the live log's size, and the rotation that moves events out of `read` (kept for `history`).
pub fn live_bytes(state: &Path) -> Result<u64> {
    let Some(c) = peek(state)? else { return Ok(0) };
    Ok(c.query_row(
        "SELECT COALESCE(SUM(LENGTH(json)), 0) FROM events WHERE archived = 0",
        [],
        |r| r.get(0),
    )?)
}

pub(crate) fn archive(state: &Path, seqs: &[u64]) -> Result<()> {
    let mut c = open(state)?;
    let tx = c.transaction()?;
    for s in seqs {
        tx.execute("UPDATE events SET archived = 1 WHERE seq = ?1", [s])?;
    }
    Ok(tx.commit()?)
}

/// A snapshot row (`board`: #337's reads; `states`: the fold's checkpoint).
pub fn get<T: DeserializeOwned>(state: &Path, key: &str) -> Option<(u64, T)> {
    let c = peek(state).ok()??;
    let q = "SELECT ts, json FROM snapshot WHERE key = ?1";
    let (ts, json) = c
        .query_row(q, [key], |r| Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?)))
        .ok()?;
    Some((ts, serde_json::from_str(&json).ok()?))
}

/// A snapshot row's version: a new one each `put`.
pub fn stamp(state: &Path, key: &str) -> Option<u64> {
    let q = "SELECT n FROM snapshot WHERE key = ?1";
    peek(state).ok()??.query_row(q, [key], |r| r.get(0)).ok()
}

/// The newest event's seq (0: none): the page's change check.
pub fn last_seq(state: &Path) -> u64 {
    let q = "SELECT COALESCE(MAX(seq), 0) FROM events";
    let c = peek(state).ok().flatten();
    c.and_then(|c| c.query_row(q, [], |r| r.get(0)).ok()).unwrap_or(0)
}

pub fn put(state: &Path, key: &str, ts: u64, v: &impl Serialize) -> Result<()> {
    let q = "INSERT OR REPLACE INTO snapshot (key, ts, json) VALUES (?1, ?2, ?3)";
    open(state)?.execute(q, params![key, ts, serde_json::to_string(v)?])?;
    Ok(())
}

/// The `devices` or `push` rows, in order.
pub fn list<T: DeserializeOwned>(state: &Path, table: &str) -> Vec<T> {
    let Ok(Some(c)) = peek(state) else { return vec![] };
    let Ok(mut q) = c.prepare(&format!("SELECT json FROM {table} ORDER BY n")) else {
        return vec![];
    };
    let rows = q.query_map([], |r| r.get::<_, String>(0));
    rows.into_iter()
        .flatten()
        .filter_map(|j| serde_json::from_str(&j.ok()?).ok())
        .collect()
}

/// Replace every row of `table` with `all`, in one transaction.
pub fn store<T: Serialize>(state: &Path, table: &str, all: &[T]) -> Result<()> {
    let mut c = open(state)?;
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
    let Ok(Some(c)) = peek(state) else { return vec![] };
    let q = format!("{LEASE} WHERE substr(key, 1, length(?1)) = ?1 ORDER BY key");
    let Ok(mut q) = c.prepare(&q) else { return vec![] };
    let rows = q.query_map([prefix], lease);
    rows.into_iter().flatten().flatten().collect()
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

/// Take `key` for `secs` from `now`, as this process: an atomic insert-if-free. Free: no lease, an expired one, or one
/// `dead` says has lost its holder. `None` while another holds it.
pub fn take(state: &Path, key: &str, now: u64, secs: u64, data: &str, dead: &dyn Fn(&Lease) -> bool) -> Option<Lease> {
    let mut c = open(state).ok()?;
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate).ok()?;
    let held = tx
        .query_row(&format!("{LEASE} WHERE key = ?1"), [key], lease)
        .optional()
        .ok()?;
    let held = held.filter(|l| l.expires_at > now && !dead(l));
    if held.is_some() {
        return None;
    }
    let l = Lease {
        key: key.into(),
        pid: std::process::id(),
        host: host(),
        at: now,
        expires_at: now + secs,
        data: data.into(),
    };
    let q = "INSERT OR REPLACE INTO leases (key, pid, host, at, expires_at, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)";
    tx.execute(q, params![l.key, l.pid, l.host, l.at, l.expires_at, l.data])
        .ok()?;
    tx.commit().ok()?;
    Some(l)
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
    open(state).is_ok_and(|c| c.execute(q, p).is_ok_and(|n| n > 0))
}

/// Drop `key`, whoever holds it (`Some(l)`: only while `l` does); true if a lease went.
pub fn release(state: &Path, key: &str, l: Option<&Lease>) -> bool {
    let Ok(c) = open(state) else { return false };
    let n = match l {
        None => c.execute("DELETE FROM leases WHERE key = ?1", [key]),
        Some(l) => c.execute(
            "DELETE FROM leases WHERE key = ?1 AND pid = ?2 AND host = ?3 AND at = ?4",
            params![key, l.pid, l.host, l.at],
        ),
    };
    n.is_ok_and(|n| n > 0)
}

/// A file from before #344, renamed out of the way once imported (the kept backup): `<name>.migrated`, or
/// `<name>.<k>.migrated` when an earlier one is there.
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

/// The files from before #344 still beside the database: the log, its archives, devices and push subscriptions.
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

/// `herdsman migrate` and every first open: the log (and its archives), devices and push subscriptions into the
/// database, in one transaction, checked by a full fold before and after; then each file goes to `*.migrated`.
/// Idempotent: a second run finds nothing left. Events keep their seq unless the database already has a later one.
fn import(c: &mut Connection, state: &Path) -> Result<()> {
    let (logs, lists) = legacy(state);
    if logs.is_empty() && !lists.iter().any(|p| p.exists()) {
        return Ok(());
    }
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (logs, lists) = legacy(state); // another process may have imported them meanwhile
    let mut done = vec![];
    if !logs.is_empty() {
        let live = events::read_file(state)?;
        let mut all = [events::archives(state)?, live.clone()].concat();
        all.sort_by_key(|e| e.seq);
        all.dedup_by_key(|e| e.seq); // a rotation that died between its archive and its rename
        let mut seen = rows(&tx, "seq > ?1", 0)?;
        let mut max = seen.last().map_or(0, |e| e.seq);
        for e in &all {
            let seq = if e.seq > max { e.seq } else { max + 1 };
            let archived = live.binary_search_by_key(&e.seq, |l| l.seq).is_err();
            insert(&tx, e, Some(seq), archived)?;
            max = seq;
        }
        seen.extend(all);
        let fold = |ev: &[Event]| serde_json::to_value(events::fold(ev).into_values().collect::<Vec<_>>()).ok();
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
    for p in &done {
        retire(p)?;
    }
    tx.commit().context("import commit")
}

/// `herdsman migrate`'s #344 step: import what is left, say what the database holds.
pub fn migrate(state: &Path, out: &mut dyn std::io::Write) -> Result<()> {
    let left = legacy(state).0.len();
    let c = open(state)?;
    let n: u64 = c.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?;
    writeln!(
        out,
        "imported {left} log file(s); {} holds {n} events",
        file(state).display()
    )?;
    Ok(())
}
