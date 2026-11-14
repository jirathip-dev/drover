//! Paired phones for the page's reply box (docs/serve.md): only each token's SHA-256 is kept.
use crate::config::Config;
use crate::events::{append, emit, working, Event};
use crate::gate::refused;
use crate::Ctx;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

/// #59: a `code` is a phone's open pairing request (from `ip`, 10 min from `created`): its cookie works once `serve approve` clears it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Device {
    pub hash: String,
    pub label: String,
    pub created: u64,
    pub used: u64,
    pub code: Option<String>,
    pub ip: String,
}

/// Write `v` as JSON, mode 0600.
pub(crate) fn save(p: &Path, v: &impl Serialize) -> Result<()> {
    write0600(p, serde_json::to_string_pretty(v)?.as_bytes())
}

/// Replace `p` with `b`, mode 0600, atomically (#122): a temp file renamed over `p`; on any error `p` is left intact.
pub(crate) fn write0600(p: &Path, b: &[u8]) -> Result<()> {
    std::fs::create_dir_all(p.parent().unwrap_or(p))?;
    let tmp = tmp_name(p);
    let r = (|| {
        let mut o = std::fs::OpenOptions::new();
        let mut f = o.write(true).create_new(true).mode(0o600).open(&tmp)?;
        f.write_all(b)?;
        f.sync_all()?;
        std::fs::rename(&tmp, p)?;
        let _ = std::fs::File::open(p.parent().unwrap_or(p)).and_then(|d| d.sync_all());
        Ok::<_, io::Error>(())
    })();
    Ok(r.inspect_err(|_| drop(std::fs::remove_file(&tmp)))?)
}

/// #342: a temp name unique per call (pid + a process-wide counter): serve's threads write concurrently.
pub(crate) fn tmp_name(p: &Path) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    PathBuf::from(format!(
        "{}.tmp-{}-{}",
        p.display(),
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ))
}

/// #342: hold across a devices.json load → change → store, so concurrent updates don't overwrite each other.
pub fn lock() -> MutexGuard<'static, ()> {
    static L: Mutex<()> = Mutex::new(());
    L.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn random(n: usize) -> Result<Vec<u8>> {
    let mut b = vec![0; n];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b)
}

/// #344: the `devices` rows of the database.
pub fn load(ctx: &Ctx) -> Vec<Device> {
    crate::db::list(&ctx.state, "devices")
}

/// #122: a failed write logs `state-write-failed` (notified, path and error kind only), at most once an hour.
pub fn store(ctx: &Ctx, d: &[Device]) -> Result<()> {
    let fresh = |e: &Event| e.kind == "state-write-failed" && e.ts + 3600 > ctx.now;
    let p = crate::db::file(&ctx.state);
    let r = crate::db::store(&ctx.state, "devices", d);
    let seen = || working(&ctx.state).unwrap_or_default().iter().any(fresh);
    if let Some(e) = r.as_ref().err().filter(|_| !seen()) {
        let kind = e.downcast_ref().map_or(io::ErrorKind::Other, io::Error::kind);
        let d = json!({"path": p, "error": kind.to_string(), "reason": format!("{}: {kind}", p.display())});
        let ev = Event::new(ctx.now, "drover", None, "state-write-failed", d);
        let _ = emit(&ctx.state, &Config::load(&ctx.config).unwrap_or_default(), ev);
    }
    r
}

/// The open (unexpired) pairing requests.
pub fn pending(ctx: &Ctx) -> Vec<Device> {
    load(ctx)
        .into_iter()
        .filter(|d| d.code.is_some() && d.created + 600 > ctx.now)
        .collect()
}

/// A pairing request: (its code, unique among the open ones; the `hd=` token for the asking browser alone); None while 4 are open (#199: two phones behind one proxy), 2 from `ip`, or on a code clash.
pub fn request(ctx: &Ctx, label: &str, ip: &str) -> Result<Option<(String, String)>> {
    let _g = lock();
    let (mut all, b, token) = (
        load(ctx),
        random(4)?,
        random(32)?.iter().map(|b| format!("{b:02x}")).collect::<String>(),
    );
    all.retain(|d| d.code.is_none() || d.created + 3600 > ctx.now);
    let (open, code, hash) = (
        pending(ctx),
        format!("{:06}", u32::from_be_bytes([b[0], b[1], b[2], b[3]]) % 1_000_000),
        sha256(token.as_bytes()),
    );
    if open.len() >= 4
        || open.iter().filter(|d| d.ip == ip).count() >= 2
        || open.iter().any(|d| d.code.as_ref() == Some(&code))
    {
        return Ok(None);
    }
    all.push(Device {
        label: format!("{label} {}", &hash[..4]),
        hash,
        created: ctx.now,
        used: ctx.now,
        code: Some(code.clone()),
        ip: ip.into(),
    });
    store(ctx, &all)?;
    append(
        &ctx.state,
        &Event::new(ctx.now, "drover", None, "pair-request", json!({"device": label})),
    )?;
    Ok(Some((code, token)))
}

/// `serve approve <code>`: pair the request's browser (single use); its device label. Logged without code or token.
pub fn approve(ctx: &Ctx, code: &str) -> Result<String> {
    let _g = lock();
    let mut all = load(ctx);
    let Some(d) = all
        .iter_mut()
        .find(|d| d.code.as_deref().is_some_and(|c| same(c, code.trim())))
    else {
        return refused(
            "NO_SUCH_CODE",
            "no pairing request with that code: tap Pair on the phone",
        );
    };
    if d.created + 600 <= ctx.now {
        return refused("EXPIRED", "that pairing code expired: tap Pair on the phone again");
    }
    let (label, hash, ip) = {
        (d.code, d.created, d.used) = (None, ctx.now, ctx.now);
        (d.label.clone(), d.hash.clone(), d.ip.clone())
    };
    // #501: a browser pairing again replaces its older row (same label but the hash suffix, same address).
    let kind = |l: &str| l.rsplit_once(' ').map_or(l, |p| p.0).to_string();
    all.retain(|o| o.hash == hash || o.code.is_some() || o.ip != ip || kind(&o.label) != kind(&label));
    store(ctx, &all)?;
    append(
        &ctx.state,
        &Event::new(ctx.now, "drover", None, "pair-approve", json!({"device": label})),
    )?;
    Ok(label)
}

/// Constant-time string equality (codes, token hashes).
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |d, (x, y)| d | (x ^ y)) == 0
}

/// The `hd=` token's hash in a `Cookie` header.
fn token(cookie: &str) -> Option<String> {
    Some(sha256(
        cookie.split(';').find_map(|c| c.trim().strip_prefix("hd="))?.as_bytes(),
    ))
}

/// #199: drop this browser's open pairing request (its `hd=` cookie), on a Pair re-tap or `[cancel]`; a paired device stays.
pub fn cancel(ctx: &Ctx, cookie: &str) -> Result<()> {
    let _g = lock();
    let (Some(tok), mut all) = (token(cookie), load(ctx)) else {
        return Ok(());
    };
    let n = all.len();
    all.retain(|d| d.code.is_none() || !same(&d.hash, &tok));
    if all.len() < n {
        store(ctx, &all)?;
    }
    Ok(())
}

pub const YEAR: u64 = 365 * 86_400;
/// The paired (approved) device whose token the `Cookie` header carries (`hd=`), used within a `YEAR` (#288); marks it used at most hourly (#122).
pub fn find(ctx: &Ctx, cookie: &str) -> Option<Device> {
    check(ctx, cookie).ok()
}

/// #501: `find`, else why not: `no cookie`, `unknown token`, `expired`, or `db error: <kind>` (no devices read).
pub fn check(ctx: &Ctx, cookie: &str) -> Result<Device, String> {
    let tok = token(cookie).ok_or("no cookie")?;
    let _g = lock();
    let mut all =
        crate::db::try_list::<Device>(&ctx.state, "devices").map_err(|e| format!("db error: {}", kind(&e)))?;
    let i = all
        .iter()
        .position(|d| d.code.is_none() && same(&d.hash, &tok))
        .ok_or("unknown token")?;
    if all[i].used + YEAR <= ctx.now {
        return Err("expired".into());
    }
    let stale = std::mem::replace(&mut all[i].used, ctx.now) + 3600 <= ctx.now;
    let _ = stale.then(|| crate::db::store_now(&ctx.state, "devices", &all));
    Ok(all.swap_remove(i))
}

/// #501: one `serve.log` line per 401 or 503 the board answers: `auth <unix secs> <status> <method> <route>: <reason>`
/// (the route without its query; never a token).
pub fn denied(ctx: &Ctx, status: &str, method: &str, route: &str, why: &str) {
    let line = format!("auth {} {status} {method} {route}: {why}\n", ctx.now);
    let mut o = std::fs::OpenOptions::new();
    let f = o
        .create(true)
        .append(true)
        .mode(0o600)
        .open(ctx.state.with_file_name("serve.log"));
    let _ = f.and_then(|mut f| f.write_all(line.as_bytes()));
}

/// #501: the board's (401, 503) answers in `serve.log` over the last 24 h.
pub fn denials(ctx: &Ctx) -> (usize, usize) {
    let log = std::fs::read_to_string(ctx.state.with_file_name("serve.log")).unwrap_or_default();
    let codes: Vec<_> = (log.lines())
        .filter_map(|l| {
            let mut w = l.strip_prefix("auth ")?.split(' ');
            w.next()?.parse::<u64>().ok().filter(|t| t + 86_400 > ctx.now)?;
            w.next()
        })
        .collect();
    let n = |c| codes.iter().filter(|x| **x == c).count();
    (n("401"), n("503"))
}

/// A database error's kind: SQLite's code (`DatabaseBusy`, `NotADatabase`), else the I/O error's.
fn kind(e: &anyhow::Error) -> String {
    match e.downcast_ref::<rusqlite::Error>() {
        Some(rusqlite::Error::SqliteFailure(f, _)) => format!("{:?}", f.code),
        _ => e
            .downcast_ref::<io::Error>()
            .map_or("other".into(), |e| e.kind().to_string()),
    }
}

/// cooee's attach token (`cooee-token` beside fleet.toml): `drover serve` creates it once, mode 0600, never rewrites it.
pub fn cooee_token(ctx: &Ctx) -> Result<String> {
    let p = ctx.config.with_file_name("cooee-token");
    std::fs::create_dir_all(p.parent().unwrap_or(&p))?;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&p)
    {
        Ok(mut f) => {
            let t: String = random(32)?.iter().map(|b| format!("{b:02x}")).collect();
            f.write_all(t.as_bytes())?;
            Ok(t)
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(std::fs::read_to_string(&p)?.trim().into()),
        Err(e) => Err(e.into()),
    }
}

/// SHA-256, lowercase hex.
pub fn sha256(data: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}
