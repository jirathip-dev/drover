//! #345: the client side of `drover service` (docs/service.md): a kick over its local socket, a remote lane's
//! commands over HTTP with its per-lane token, and the token check the service runs. With no service, `None`: the
//! caller goes on in direct mode.
use crate::config::Config;
use crate::events::{ends, read, Event};
use crate::gate::refused;
use crate::runner::{argv, Out, Runner};
use crate::Ctx;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

/// The service's local socket, beside the state database.
pub fn sock(ctx: &Ctx) -> PathBuf {
    ctx.state.with_file_name("service.sock")
}

/// One request line to the running service and its one reply line; `None` when no service listens.
pub fn call(ctx: &Ctx, req: &Value) -> Option<Value> {
    let s = UnixStream::connect(sock(ctx)).ok()?;
    let t = Some(Duration::from_secs(crate::config::limits().service_call_secs));
    s.set_read_timeout(t).ok()?;
    s.set_write_timeout(t).ok()?;
    (&s).write_all(format!("{req}\n").as_bytes()).ok()?;
    let mut line = String::new();
    BufReader::new(&s).read_line(&mut line).ok()?;
    serde_json::from_str(&line).ok()
}

/// The running service's pid.
pub fn up(ctx: &Ctx) -> Option<u64> {
    call(ctx, &json!({"ping": true}))?["pid"].as_u64()
}

/// A kick the service drives in process at once: its pid (the spawn marker's holder).
pub fn kick(ctx: &Ctx, fleet: &str, issue: u64, why: &str, id: Option<&str>) -> Option<u32> {
    let v = call(
        ctx,
        &json!({"kick": {"fleet": fleet, "issue": issue, "why": why, "id": id}}),
    )?;
    v["pid"].as_u64().and_then(|p| u32::try_from(p).ok())
}

/// A lane on another host: `args` run by the service at `url` (`DROVER_SERVICE`) as the token's lane; a `--file`
/// travels as its text. The token rides in the body, never in argv.
pub fn post(r: &dyn Runner, url: &str, token: &str, args: &[String]) -> Result<Out> {
    let mut files = serde_json::Map::new();
    for w in args.windows(2).filter(|w| w[0] == "--file") {
        let text = std::fs::read_to_string(&w[1]).with_context(|| format!("cannot read {}", w[1]))?;
        files.insert(w[1].clone(), text.into());
    }
    let body = json!({"token": token, "args": args, "files": files});
    let url = format!("{}/api/v1/cmd", url.trim_end_matches('/'));
    let a = argv(&["-sS", "--max-time", "150", "--data-binary", "@-", &url]);
    let o = r.run_in("curl", &a, &body.to_string())?;
    let v: Option<Value> = serde_json::from_str(&o.stdout).ok();
    let Some(v) = v.filter(|_| o.code == 0) else {
        bail!("drover service at {url}: {}{}", o.stderr.trim(), o.stdout.trim())
    };
    let text = |k: &str| v[k].as_str().unwrap_or_default().to_string();
    let code = v["code"].as_i64().and_then(|c| i32::try_from(c).ok()).unwrap_or(1);
    Ok(Out {
        code,
        stdout: text("stdout"),
        stderr: text("stderr"),
    })
}

/// A lane's token scope: its fleet, issue, role, agent, `DROVER_LANE` and its spawn's time.
#[derive(Serialize, Deserialize)]
pub struct Grant {
    pub fleet: String,
    pub issue: u64,
    pub role: String,
    pub agent: String,
    pub lane: String,
    pub at: u64,
}

fn key(token: &str) -> String {
    format!("token-{}", crate::devices::sha256(token.as_bytes()))
}

/// Spawn's token for a lane on another host, kept only as its hash.
pub fn grant(ctx: &Ctx, g: &Grant) -> Result<String> {
    let t: String = crate::devices::random(32)?.iter().map(|b| format!("{b:02x}")).collect();
    crate::db::put(&ctx.state, &key(&t), ctx.now, g)?;
    Ok(t)
}

/// The `DROVER_LANE` `args` run as, if the token may run them: its own lane's command (#389's `foreign`; a verdict
/// only from a check role), while its spawn is the issue's latest for its role, not closed, and the issue is open.
pub fn authorize(ctx: &Ctx, cfg: &Config, token: &str, args: &[String]) -> Result<String> {
    let Some((_, g)) = crate::db::get::<Grant>(&ctx.state, &key(token)) else {
        return refused("TOKEN", "unknown token");
    };
    if args.is_empty() {
        return refused("TOKEN", "no command");
    }
    let events = read(&ctx.state)?;
    if let Some(why) = crate::cli::foreign(args, &g.lane, &events) {
        return refused("TOKEN", why);
    }
    if args[0] == "verdict" && !cfg.fleet(&g.fleet)?.flow().is_check(&g.role) {
        return refused("TOKEN", format!("a {} lane records no verdict", g.role));
    }
    let mine = |e: &&Event| e.fleet == g.fleet && e.issue == Some(g.issue);
    let sp = events
        .iter()
        .filter(mine)
        .rfind(|e| e.kind == "spawn" && e.str("role") == Some(&g.role));
    let closed = |sp: &Event| {
        let dead =
            |e: &Event| e.kind == "lane-dead" && e.str("key").is_some_and(|k| k.ends_with(&format!(":{}", g.agent)));
        let gone = |e: &&Event| e.ts >= sp.ts && (e.kind == "lane-closed" && ends(e, sp) || dead(e));
        events.iter().filter(mine).any(|e| gone(&e))
    };
    let done = crate::events::states(&ctx.state)?
        .get(&(g.fleet.clone(), g.issue))
        .is_some_and(crate::events::IssueState::done);
    if done || !sp.is_some_and(|sp| sp.ts == g.at && !closed(sp)) {
        return refused("TOKEN", format!("{}'s lane is closed", g.agent));
    }
    Ok(g.lane)
}

/// The live log after `seq`, for API v1's event stream.
pub fn since(ctx: &Ctx, seq: u64) -> Result<Vec<Event>> {
    crate::db::events(&ctx.state, "archived = 0 AND seq > ?1", seq)
}
