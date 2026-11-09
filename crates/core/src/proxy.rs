//! #637: the token proxy on the main host (docs/hosts.md). A remote ACP lane's adapter gets the proxy's URL (an ssh
//! tunnel back here) and a per-lane key; the proxy checks that key and forwards each model call to `[proxy] upstream`
//! with the role's account token added here, so no Claude token ever reaches a remote host. Keys are kept as their
//! sha256, live for `key_hours`, end with their lane, and `hosts keys revoke` ends one at once.
use crate::config::{write_atomic, Config};
use crate::events::{ends, fold, read, Event};
use crate::harness::{BETA_HEADER, OAUTH_BETA};
use crate::runner::{argv, ok, Runner};
use crate::Ctx;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

/// One lane key as kept (`<state dir>/proxy-keys.json`): never the key itself, only its sha256.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Key {
    pub id: String,
    pub sha: String,
    pub fleet: String,
    pub issue: u64,
    pub agent: String,
    pub host: String,
    /// The account token's secret name (`all`'s, in the store).
    pub account: String,
    pub created: u64,
    pub expires: u64,
    #[serde(default)]
    pub revoked: Option<u64>,
}

fn file(ctx: &Ctx) -> PathBuf {
    ctx.state.with_file_name("proxy-keys.json")
}

fn sha(key: &str) -> String {
    Sha256::digest(key.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn keys(ctx: &Ctx) -> Vec<Key> {
    let text = std::fs::read_to_string(file(ctx)).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

fn save(ctx: &Ctx, keys: &[Key]) -> Result<()> {
    write_atomic(&file(ctx), &serde_json::to_string_pretty(keys)?)
}

/// A fresh lane key (`hsk-` and 48 hex digits from /dev/urandom).
pub fn new_key() -> Result<String> {
    let b = crate::devices::random(24)?;
    Ok(format!(
        "hsk-{}",
        b.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}

/// Record `key` for lane `agent` of `fleet#issue` on `host`, signing with account secret `account`; the agent's older
/// keys are revoked (a new spawn supersedes them). Its id.
pub fn mint(
    ctx: &Ctx,
    cfg: &Config,
    key: &str,
    (fleet, issue, agent): (&str, u64, &str),
    (host, account): (&str, &str),
) -> Result<String> {
    let mut all = keys(ctx);
    for k in all.iter_mut().filter(|k| k.agent == agent && k.revoked.is_none()) {
        k.revoked = Some(ctx.now);
    }
    let sha = sha(key);
    let k = Key {
        id: sha[..8].into(),
        sha,
        fleet: fleet.into(),
        issue,
        agent: agent.into(),
        host: host.into(),
        account: account.into(),
        created: ctx.now,
        expires: ctx.now + cfg.proxy.key_hours * 3600,
        revoked: None,
    };
    let id = k.id.clone();
    all.push(k);
    save(ctx, &all)?;
    Ok(id)
}

/// `live`, or why the key no longer signs: `revoked`, `expired`, or `ended` (its issue merged or closed, its lane
/// closed, dead or spawned again).
pub fn state(now: u64, events: &[Event], k: &Key) -> &'static str {
    let mine = |e: &&Event| e.kind == "spawn" && e.str("agent") == Some(&k.agent) && e.ts >= k.created;
    let spawns: Vec<&Event> = events.iter().filter(mine).collect();
    let dead = |e: &Event| e.kind == "lane-dead" && e.str("key").is_some_and(|x| x.ends_with(&format!(":{}", k.agent)));
    let gone = spawns.first().is_some_and(|sp| {
        spawns.len() > 1
            || events
                .iter()
                .any(|e| e.ts >= sp.ts && (e.kind == "lane-closed" && ends(e, sp) || dead(e)))
    });
    let done = fold(events)
        .get(&(k.fleet.clone(), k.issue))
        .is_some_and(crate::events::IssueState::done);
    match () {
        () if k.revoked.is_some() => "revoked",
        () if now >= k.expires => "expired",
        () if gone || done => "ended",
        () => "live",
    }
}

/// `hosts keys`: every lane key, newest first, with its state.
pub fn list(ctx: &Ctx, out: &mut dyn Write) -> Result<()> {
    let events = read(&ctx.state).unwrap_or_default();
    for k in keys(ctx).iter().rev() {
        let st = state(ctx.now, &events, k);
        let left = k.expires.saturating_sub(ctx.now) / 60;
        writeln!(
            out,
            "{}  {st:<7} {} on {} · {}#{} · {left} min left",
            k.id, k.agent, k.host, k.fleet, k.issue
        )?;
    }
    Ok(())
}

/// `hosts keys revoke <id>`: that key stops signing at once; logs `proxy-key-revoked`.
pub fn revoke(ctx: &Ctx, cfg: &Config, id: &str, out: &mut dyn Write) -> Result<()> {
    let mut all = keys(ctx);
    let k = all
        .iter_mut()
        .find(|k| k.id == id)
        .with_context(|| format!("no lane key {id}"))?;
    k.revoked = Some(ctx.now);
    let d = serde_json::json!({"id": id, "agent": k.agent, "host": k.host});
    let e = Event::new(ctx.now, &k.fleet, Some(k.issue), "proxy-key-revoked", d);
    save(ctx, &all)?;
    crate::events::emit(&ctx.state, cfg, e)?;
    Ok(writeln!(out, "revoked {id}")?)
}

/// The account token `key` signs with, or why it is refused.
fn authorize(ctx: &Ctx, cfg: &Config, r: &dyn Runner, (key, now): (&str, u64)) -> std::result::Result<String, String> {
    let (all, h) = (keys(ctx), sha(key));
    let k = all.iter().find(|k| k.sha == h).ok_or("unknown lane key")?;
    match state(now, &read(&ctx.state).unwrap_or_default(), k) {
        "live" => {}
        st => return Err(format!("lane key {} {st}", k.id)),
    }
    let v = crate::secret::lookup(r, "all", &k.account).map_err(|e| format!("{e:#}"))?;
    v.or_else(|| cfg.legacy_secret(&k.account))
        .ok_or_else(|| format!("account secret {} is not in the store", k.account))
}

/// Headers never forwarded: hop-by-hop ones, the lane's own credentials, compression (curl passes bytes through).
const DROP: [&str; 9] = [
    "host",
    "connection",
    "content-length",
    "transfer-encoding",
    "keep-alive",
    "x-api-key",
    "authorization",
    "accept-encoding",
    "expect",
];

/// `herdsman proxy`: serve `[proxy] port` on 127.0.0.1 until killed, one thread per connection.
pub fn serve(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> Result<()> {
    let l =
        TcpListener::bind(("127.0.0.1", cfg.proxy.port)).with_context(|| format!("proxy port {}", cfg.proxy.port))?;
    let clock = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(ctx.now, |d| d.as_secs())
    };
    serve_on(ctx, cfg, r, &l, &clock)
}

/// `serve` on a bound listener (tests bind port 0), each request's key checked at `clock()`.
pub fn serve_on(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    l: &TcpListener,
    clock: &(dyn Fn() -> u64 + Sync),
) -> Result<()> {
    std::thread::scope(|s| {
        for c in l.incoming().flatten() {
            s.spawn(move || {
                if let Err(e) = handle(ctx, cfg, r, c, clock()) {
                    eprintln!("proxy: {e:#}");
                }
            });
        }
    });
    Ok(())
}

fn reply(c: &mut TcpStream, code: u16, msg: &str) -> Result<()> {
    let body = serde_json::json!({"type": "error", "error": {"type": "authentication_error", "message": msg}});
    let body = body.to_string();
    let head = format!(
        "HTTP/1.1 {code} Refused\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    Ok(c.write_all((head + &body).as_bytes())?)
}

/// One request: its lane key checked, then forwarded by curl (the token in its config on stdin, never argv); the
/// answer streamed back as it comes.
fn handle(ctx: &Ctx, cfg: &Config, r: &dyn Runner, mut c: TcpStream, now: u64) -> Result<()> {
    let mut rd = BufReader::new(c.try_clone()?);
    let mut line = String::new();
    rd.read_line(&mut line)?;
    let mut w = line.split_whitespace();
    let (method, path) = (
        w.next().unwrap_or("GET").to_string(),
        w.next().unwrap_or("/").to_string(),
    );
    let mut heads: Vec<(String, String)> = vec![];
    loop {
        let mut h = String::new();
        if rd.read_line(&mut h)? == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            heads.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let get = |k: &str| heads.iter().find(|h| h.0 == k).map(|h| h.1.as_str());
    let mut body = vec![0; get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0)];
    rd.read_exact(&mut body)?;
    // The path is appended to `upstream`: only an origin-form path, so it can never move the host (`@evil/`).
    // Checked once the request is fully read: an earlier reply leaves unread data and the client sees a reset.
    if !path.starts_with('/') || path.chars().any(|ch| "@\\#\"".contains(ch) || ch.is_control()) {
        return reply(&mut c, 400, "bad request path");
    }
    let bearer = get("authorization").and_then(|a| a.strip_prefix("Bearer "));
    let token = match authorize(ctx, cfg, r, (get("x-api-key").or(bearer).unwrap_or_default(), now)) {
        Ok(t) => t,
        Err(why) => return reply(&mut c, 401, &why),
    };
    let q = |v: &str| v.replace('\\', "\\\\").replace('"', "\\\"");
    let mut conf = format!(
        "url = \"{}{}\"\nheader = \"authorization: Bearer {}\"\nheader = \"Expect:\"\n",
        q(&cfg.proxy.upstream),
        q(&path),
        q(&token)
    );
    let mut beta: Vec<&str> = get(BETA_HEADER)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .collect();
    if !beta.contains(&OAUTH_BETA) {
        beta.push(OAUTH_BETA);
    }
    conf += &format!("header = \"{BETA_HEADER}: {}\"\n", beta.join(","));
    for (k, v) in heads
        .iter()
        .filter(|h| !DROP.contains(&h.0.as_str()) && h.0 != BETA_HEADER)
    {
        conf += &format!("header = \"{k}: {}\"\n", q(v));
    }
    let tmp = ctx.state.with_file_name(format!(
        "proxy-{}.body",
        crate::devices::random(6)?
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ));
    if !body.is_empty() {
        write_private(&tmp, &body)?;
        conf += &format!("data-binary = \"@{}\"\n", q(&tmp.to_string_lossy()));
    }
    let head = method == "HEAD";
    conf += &if head {
        "head\n".to_string()
    } else {
        format!("request = \"{}\"\ndump-header = \"-\"\n", q(&method))
    };
    let res = r
        .stream("curl", &argv(&["-sS", "-N", "-g", "--http1.1", "-K", "-"]), &conf)
        .and_then(|up| relay(&mut BufReader::new(up), &mut c));
    let _ = std::fs::remove_file(&tmp);
    if res.is_err() {
        return reply(&mut c, 502, "upstream did not answer");
    }
    Ok(())
}

/// curl's answer as the client's: its last status block (a `100 Continue` skipped) as HTTP/1.1 with `connection:
/// close`, then the body until curl ends.
fn relay(up: &mut impl BufRead, c: &mut TcpStream) -> Result<()> {
    let (mut status, mut heads) = (String::new(), vec![]);
    loop {
        let mut l = String::new();
        anyhow::ensure!(up.read_line(&mut l)? > 0, "no answer");
        let l = l.trim_end().to_string();
        if l.starts_with("HTTP/") {
            (status, heads) = (l.split_once(' ').map_or("502".into(), |x| x.1.to_string()), vec![]);
        } else if l.is_empty() && !status.starts_with('1') {
            break;
        } else if !l.is_empty() {
            heads.push(l);
        }
    }
    let skip = |h: &&String| {
        DROP.iter()
            .any(|d| h.to_ascii_lowercase().starts_with(&format!("{d}:")))
    };
    let mut out = format!("HTTP/1.1 {status}\r\n");
    heads
        .iter()
        .filter(|h| !skip(h))
        .for_each(|h| out += &format!("{h}\r\n"));
    c.write_all((out + "connection: close\r\n\r\n").as_bytes())?;
    let mut buf = [0; 16384];
    loop {
        match up.read(&mut buf)? {
            0 => return Ok(()),
            n => c.write_all(&buf[..n])?,
        }
    }
}

fn write_private(p: &std::path::Path, b: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(p)?;
    Ok(f.write_all(b)?)
}

/// Before a remote ACP lane starts (and each survey of a `shed-acp` host): the proxy answering here (else started
/// detached) and host `name`'s tunnel (`ssh -R`, the proxy's port there to here) alive (else started, its pid kept).
pub fn ensure(ctx: &Ctx, cfg: &Config, r: &dyn Runner, (name, machine): (&str, &str)) -> Result<()> {
    let port = cfg.proxy.port;
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    if TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(1)).is_err() {
        r.detach(&ctx.exe, &argv(&["proxy"]), &ctx.state.with_file_name("proxy.log"))?;
    }
    let pid = ctx.state.with_file_name(format!("tunnel-{name}.pid"));
    let was = std::fs::read_to_string(&pid).unwrap_or_default();
    if !was.trim().is_empty() && ok(r, "kill", &["-0", was.trim()]).is_ok() {
        return Ok(());
    }
    let fwd = format!("127.0.0.1:{port}:127.0.0.1:{port}");
    let a = argv(&[
        "-N",
        "-oBatchMode=yes",
        "-oExitOnForwardFailure=yes",
        "-oServerAliveInterval=30",
        "-oControlPath=none",
        "-R",
        &fwd,
        machine,
    ]);
    let n = r.detach("ssh", &a, &ctx.state.with_file_name(format!("tunnel-{name}.log")))?;
    Ok(std::fs::write(pid, n.to_string())?)
}

/// The proxy's URL as a lane on a tunnelled host reaches it.
pub fn url(cfg: &Config) -> String {
    format!("http://127.0.0.1:{}", cfg.proxy.port)
}
