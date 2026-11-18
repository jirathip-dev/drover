use crate::config::{write_atomic, Config};
use crate::events::{ends, fold, working, Event};
use crate::runner::{argv, ok, Runner};
use crate::Ctx;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::MetadataExt;
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

/// Record `key` for lane `agent` of `fleet#issue` on `host`; the agent's older keys end only once this one's lane logs its `spawn` (see [`state`]). Its id.
pub fn mint(
    ctx: &Ctx,
    cfg: &Config,
    key: &str,
    (fleet, issue, agent): (&str, u64, &str),
    host: &str,
) -> Result<String> {
    let mut all = keys(ctx);
    let sha = sha(key);
    let k = Key {
        id: sha[..8].into(),
        sha,
        fleet: fleet.into(),
        issue,
        agent: agent.into(),
        host: host.into(),
        created: ctx.now,
        expires: ctx.now + cfg.proxy.key_hours * 3600,
        revoked: None,
    };
    let id = k.id.clone();
    all.push(k);
    save(ctx, &all)?;
    Ok(id)
}

/// `live`, or why the key no longer signs: `revoked`, `expired`, or `ended` (its issue merged or closed, its lane closed, dead or spawned again).
pub fn state(now: u64, events: &[Event], k: &Key) -> &'static str {
    let mine = |e: &&Event| e.kind == "spawn" && e.str("agent") == Some(&k.agent) && e.ts >= k.created;
    let spawns: Vec<&Event> = events.iter().filter(mine).collect();
    let back = |d: &Event| {
        let key = format!("alive:{}", d.str("key").unwrap_or_default());
        events
            .iter()
            .any(|e| e.kind == "lane-alive" && e.str("key") == Some(&key))
    };
    let dead = |e: &Event| {
        e.kind == "lane-dead" && e.str("key").is_some_and(|x| x.ends_with(&format!(":{}", k.agent))) && !back(e)
    };
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
    let events = working(&ctx.state).unwrap_or_default();
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

/// The token the proxy signs with, `[proxy] token-file` (0600), or why there is none and how to make it.
pub fn token(cfg: &Config) -> std::result::Result<String, String> {
    let (path, _) = token_file(cfg);
    let f = cfg.proxy.token_file.as_deref().map(crate::config::expand);
    let v = f.map(|f| crate::secret::file(&f).map_err(|e| format!("{e:#}")));
    v.transpose()?
        .flatten()
        .ok_or_else(|| format!("proxy token missing: {}", fix(cfg, &path)))
}

/// The token file's state: missing (or `[proxy] token-file` unset), readable by others, or saved at its mtime.
pub enum Token {
    Missing,
    Open,
    Saved(u64),
}

/// `[proxy] token-file` as written (else the docs' path) and its state.
pub fn token_file(cfg: &Config) -> (String, Token) {
    let raw = cfg.proxy.token_file.clone();
    let shown = raw.clone().unwrap_or_else(|| "~/.config/drover/tokens/proxy".into());
    let meta = raw.and_then(|f| std::fs::metadata(crate::config::expand(&f)).ok());
    let t = match meta.filter(|m| m.len() > 0) {
        None => Token::Missing,
        Some(m) if m.mode() & 0o077 != 0 => Token::Open,
        Some(m) => Token::Saved(u64::try_from(m.mtime()).unwrap_or(0)),
    };
    (shown, t)
}

fn days_left(now: u64, at: u64) -> u64 {
    (at + 365 * 86_400).saturating_sub(now) / 86_400
}

fn fix(cfg: &Config, path: &str) -> String {
    let setup = crate::providers::proxied().map_or("", |p| p.setup);
    let unset = cfg.proxy.token_file.is_none();
    let set = unset.then_some(" and set [proxy] token-file to it");
    format!("run {setup}, save it 0600 at {path}{}", set.unwrap_or_default())
}

/// With a `shed-acp` host, doctor's and the Hosts card's proxy facts: the token's `✓`/`!`/`✗` line, the proxy's port, each host's tunnel, the live lane keys.
pub fn status(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> Option<Vec<String>> {
    let acp: Vec<&String> = cfg.hosts.keys().filter(|h| cfg.host_acp(h).is_some()).collect();
    acp.first()?;
    let (path, t) = token_file(cfg);
    let head = match t {
        Token::Missing => format!("✗ proxy token missing: {}", fix(cfg, &path)),
        Token::Open => format!("✗ proxy token readable by others: chmod 600 {path}"),
        Token::Saved(at) => {
            let (left, day) = (days_left(ctx.now, at), crate::lesson::ymd(at / 86_400));
            let again = format!(": {} again", fix(cfg, &path));
            let (mark, warn) = if left > 30 { ('✓', "") } else { ('!', &*again) };
            format!("{mark} proxy token set (saved {day}, ~{left} d left){warn}")
        }
    };
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], cfg.proxy.port));
    let wait = std::time::Duration::from_secs(cfg.limits.proxy_connect_secs);
    let up = TcpStream::connect_timeout(&addr, wait).map_or("down", |_| "answering");
    let mut facts = vec![head, format!("proxy {up} on :{}", cfg.proxy.port)];
    let seen = crate::hosts::cached(ctx);
    let down = |h: &str| {
        seen.get(h)
            .is_some_and(|s| s.health.iter().any(|w| w.starts_with(DOWN)))
    };
    for h in acp {
        let up = tunnel(ctx, r, h) && !down(h);
        facts.push(format!("tunnel {h} {}", if up { "up" } else { "down" }));
    }
    let ev = working(&ctx.state).unwrap_or_default();
    let live = keys(ctx).iter().filter(|k| state(ctx.now, &ev, k) == "live").count();
    facts.push(format!("{live} live lane keys"));
    Some(facts)
}

static ONCE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn alert(ctx: &Ctx, cfg: &Config, (kind, key, why): (&str, &str, &str)) -> Result<()> {
    let _once = ONCE.lock();
    let dup = |e: &Event| e.kind == kind && e.val("key") == key;
    if working(&ctx.state)?.iter().any(dup) {
        return Ok(());
    }
    let d = serde_json::json!({"key": key, "reason": why});
    crate::events::emit(&ctx.state, cfg, Event::new(ctx.now, "drover", None, kind, d).push())
}

fn rejected(ctx: &Ctx, cfg: &Config) -> Result<()> {
    let (path, Token::Saved(at)) = token_file(cfg) else {
        return Ok(());
    };
    let why = format!("upstream refused the proxy token (401): {}", fix(cfg, &path));
    alert(ctx, cfg, ("proxy-token-rejected", &at.to_string(), &why))
}

fn authorize(ctx: &Ctx, cfg: &Config, (key, now): (&str, u64)) -> std::result::Result<(String, Key), String> {
    let (all, h) = (keys(ctx), sha(key));
    let k = all.into_iter().find(|k| k.sha == h).ok_or("unknown lane key")?;
    match state(now, &working(&ctx.state).unwrap_or_default(), &k) {
        "live" => Ok((token(cfg)?, k)),
        st => Err(format!("lane key {} {st}", k.id)),
    }
}

fn signed_in(ctx: &Ctx, cfg: &Config, k: &Key, now: u64) -> Result<()> {
    let events = working(&ctx.state)?;
    let out = events
        .iter()
        .rfind(|e| e.kind == "signed-out" && e.str("agent") == Some(&k.agent))
        .map(|e| e.ts);
    if out.is_none() || crate::hosts::proxy_ok(&events, Some(&k.agent)) >= out {
        return Ok(());
    }
    let d = serde_json::json!({"agent": k.agent, "host": k.host});
    let e = Event::new(now, &k.fleet, Some(k.issue), "proxy-ok", d).passive();
    crate::events::emit(&ctx.state, cfg, e)
}

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

/// `drover proxy`: serve `[proxy] port` on 127.0.0.1 until killed, one thread per connection.
pub fn serve(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> Result<()> {
    let l =
        TcpListener::bind(("127.0.0.1", cfg.proxy.port)).with_context(|| format!("proxy port {}", cfg.proxy.port))?;
    let clock = crate::clock::wall;
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
                    crate::stderr!("proxy: {e:#}");
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
    if !path.starts_with('/') || path.chars().any(|ch| "@\\#\"".contains(ch) || ch.is_control()) {
        return reply(&mut c, 400, "bad request path");
    }
    let bearer = get("authorization").and_then(|a| a.strip_prefix("Bearer "));
    let (token, key) = match authorize(ctx, cfg, (get("x-api-key").or(bearer).unwrap_or_default(), now)) {
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
    let kept: Vec<_> = heads
        .iter()
        .filter(|h| !DROP.contains(&h.0.as_str()))
        .cloned()
        .collect();
    let kept = match crate::providers::proxied() {
        Some(px) => (px.headers)(&kept),
        None => kept,
    };
    for (k, v) in &kept {
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
    std::fs::remove_file(&tmp).ok();
    match res {
        Ok(s) if s.starts_with('2') => signed_in(ctx, cfg, &key, now),
        Ok(s) if s.starts_with("401") => rejected(ctx, cfg),
        Ok(_) => Ok(()),
        Err(_) => reply(&mut c, 502, "upstream did not answer"),
    }
}

fn relay(up: &mut impl BufRead, c: &mut TcpStream) -> Result<String> {
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
            0 => return Ok(status),
            n => c.write_all(buf.get(..n).unwrap_or_default())?,
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

/// Before a remote ACP lane starts (and each survey of a `shed-acp` host): `proxy-token-expiring` once at 30 d and 7 d left, the proxy answering here (else started detached) and host `name`'s tunnel (`ssh -R`) answering through its far end (else restarted on a freed remote port); whether it answers after this look.
pub fn ensure(ctx: &Ctx, cfg: &Config, r: &dyn Runner, (name, machine): (&str, &str)) -> Result<bool> {
    if let (path, Token::Saved(at)) = token_file(cfg) {
        let left = days_left(ctx.now, at);
        if let Some(stage) = [7, 30].into_iter().find(|d| left <= *d) {
            let why = format!(
                "the proxy token runs out in ~{left} d (saved + 365 d): {} again",
                fix(cfg, &path)
            );
            alert(ctx, cfg, ("proxy-token-expiring", &format!("{at}:{stage}"), &why)).ok();
        }
    }
    let port = cfg.proxy.port;
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    if TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(cfg.limits.proxy_connect_secs)).is_err() {
        r.detach(&ctx.exe, &argv(&["proxy"]), &ctx.state.with_file_name("proxy.log"))?;
    }
    let pid = ctx.state.with_file_name(format!("tunnel-{name}.pid"));
    let on = crate::hosts::On::new(r, Some(machine));
    let wait = cfg.limits.proxy_connect_secs.to_string();
    let answers = || ok(&on, "curl", &["-s", "-o", "/dev/null", "-m", &wait, &url(cfg)]).is_ok();
    if tunnel(ctx, r, name) {
        if answers() {
            return Ok(true);
        }
        let old = std::fs::read_to_string(&pid).unwrap_or_default();
        ok(r, "kill", &[old.trim()]).ok();
    }
    ok(&on, "sh", &["-c", FREE, "sh", &port.to_string()]).ok();
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
    std::fs::write(pid, n.to_string())?;
    Ok(answers())
}

const FREE: &str = "for p in $({ ss -Htlnp \"sport = :$1\"; sudo -n ss -Htlnp \"sport = :$1\"; } 2>/dev/null | \
grep -o 'pid=[0-9]*' | cut -d= -f2 | sort -u); do kill \"$p\" 2>/dev/null || sudo -n kill \"$p\"; done";

/// The health row `survey` keeps on a `shed-acp` host whose tunnel did not answer through its far end (a `✗`: no new lane there).
pub const DOWN: &str = "✗ tunnel down";

fn tunnel(ctx: &Ctx, r: &dyn Runner, name: &str) -> bool {
    let pid = std::fs::read_to_string(ctx.state.with_file_name(format!("tunnel-{name}.pid"))).unwrap_or_default();
    !pid.trim().is_empty() && ok(r, "kill", &["-0", pid.trim()]).is_ok()
}

/// The proxy's URL as a lane on a tunnelled host reaches it.
pub fn url(cfg: &Config) -> String {
    format!("http://127.0.0.1:{}", cfg.proxy.port)
}
