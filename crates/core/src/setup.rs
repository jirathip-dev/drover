//! #303 setup terminals: a configured login (`[[setup]]`, else built-in claude/gh/codex) in a herdr pane on a host,
//! typed into from the board's sheet; it closes when the command exits or after `TIMEOUT`. Only names run, never lines.
use crate::events::{append, read, Event};
use crate::{config::Config, gate::refused, herdr, hosts::On, runner::argv, runner::Runner, Ctx};
use anyhow::{Context, Result};
use serde_json::{json, Value};

pub const TIMEOUT: u64 = 600;

/// Every setup name a host card offers with its shell line (`[[setup]]` env first), config before built-ins.
pub fn all(cfg: &Config) -> Vec<(String, String)> {
    let q = |v: &str| v.replace('\'', "'\\''");
    let env = |(k, x): (&String, &String)| match x.strip_prefix("~/") {
        Some(rest) => format!("{k}=~/'{}' ", q(rest)),
        None => format!("{k}='{}' ", q(x)),
    };
    let mut v: Vec<_> = cfg
        .setup
        .iter()
        .map(|s| (s.name.clone(), s.env.iter().map(env).collect::<String>() + &s.cmd))
        .collect();
    let builtin = "claude=claude /login,gh=gh auth login,codex=codex login";
    for (n, c) in builtin.split(',').filter_map(|b| b.split_once('=')) {
        if !v.iter().any(|x| x.0 == n) {
            v.push((n.into(), c.into()));
        }
    }
    v
}

/// Start setup `name` on `host` in a new workspace there; logs `setup` (no command line, no input); the pane id.
pub fn start(ctx: &Ctx, cfg: &Config, r: &dyn Runner, host: &str, name: &str, via: &str) -> Result<String> {
    let Some((_, line)) = all(cfg).into_iter().find(|s| s.0 == name) else {
        return refused("NO_SUCH_SETUP", format!("no setup `{name}`"));
    };
    let h = cfg.hosts.get(host);
    if h.is_none() && !cfg.hosts.is_empty() {
        return refused("NO_SUCH_HOST", format!("no [hosts.{host}]"));
    }
    let on = On::new(r, h.and_then(|h| h.machine.as_deref()));
    let ws = "workspace create --cwd /tmp --label herdsman-setup --no-focus";
    let ws = herdr::json(&on, &argv(&ws.split(' ').collect::<Vec<_>>()))?;
    let pane = ws["result"]["root_pane"]["pane_id"].as_str().context("no pane")?;
    herdr::ok(&on, &herdr::run_argv(pane, &[&format!("cd; {line}; exit")]))?;
    let d = json!({"host": host, "name": name, "pane": pane, "machine": on.machine, "via": via});
    append(&ctx.state, &Event::new(ctx.now, "", None, "setup", d))?;
    Ok(pane.into())
}

/// The open `setup` event of `pane` (no `setup-closed` after it).
pub fn open<'e>(events: &'e [Event], pane: &str) -> Option<&'e Event> {
    let i = events
        .iter()
        .rposition(|e| e.kind.starts_with("setup") && e.str("pane") == Some(pane))?;
    Some(&events[i]).filter(|e| e.kind == "setup")
}

/// Close setup `e`'s workspace (its scrollback goes with it) and log `setup-closed`.
pub fn close(ctx: &Ctx, r: &dyn Runner, e: &Event, why: &str) -> Result<()> {
    let pane = e.str("pane").unwrap_or_default();
    herdr::workspace_close(&On::of(r, Some(e)), pane.split(':').next().unwrap_or(pane))?;
    let d = json!({"pane": pane, "why": why});
    append(&ctx.state, &Event::new(ctx.now, "", None, "setup-closed", d))
}

/// `check`: close every setup open `TIMEOUT` seconds.
pub fn reap(ctx: &Ctx, r: &dyn Runner, events: &[Event]) -> Result<()> {
    for e in events.iter().filter(|e| e.kind == "setup" && ctx.now >= e.ts + TIMEOUT) {
        if open(events, e.str("pane").unwrap_or_default()).is_some_and(|o| o.ts == e.ts) {
            let _ = close(ctx, r, e, "timeout");
        }
    }
    Ok(())
}

/// The sheet's read of setup `pane`: its masked screen and every `https://` URL (a wrapped one joined), or closed.
pub fn screen(ctx: &Ctx, r: &dyn Runner, pane: &str) -> Result<Value> {
    let (events, shut) = (read(&ctx.state)?, json!({"closed": true, "lines": []}));
    let Some(e) = open(&events, pane) else { return Ok(shut) };
    let live = ctx.now < e.ts + TIMEOUT;
    let o = herdr::exec(&On::of(r, Some(e)), &herdr::read_argv(pane))?;
    if !live || herdr::failed_with(&o, "pane_not_found") {
        return close(ctx, r, e, ["timeout", "exited"][usize::from(live)]).map(|()| shut);
    }
    anyhow::ensure!(o.code == 0, "could not read the sign-in pane; retrying");
    let s = o.stdout;
    let ls: Vec<&str> = s.lines().map(str::trim_end).collect();
    let mut urls = vec![];
    for (i, l) in ls.iter().enumerate().filter(|(_, l)| l.contains("https://")) {
        let u = l[l.find("https://").unwrap_or(0)..]
            .split(' ')
            .next()
            .unwrap_or_default();
        let more = ls[i + 1..]
            .iter()
            .take_while(|n| l.ends_with(u) && !n.is_empty() && !n.trim().contains(' '));
        urls.push(more.fold(u.to_string(), |u, n| u + n.trim()));
    }
    let lines: Vec<_> = ls.iter().map(|l| crate::lanes::mask(l)).collect();
    Ok(json!({"lines": lines, "urls": urls, "closed": false}))
}

/// The sheet's reply into setup `pane`: its text (keys and text only: no control characters), then Enter; never logged.
pub fn send(ctx: &Ctx, r: &dyn Runner, pane: &str, text: &str) -> Result<String> {
    let events = read(&ctx.state)?;
    let on = On::of(r, Some(open(&events, pane).context("that sign-in has closed")?));
    if text.chars().any(char::is_control) || text.chars().count() > 2000 {
        return refused("BAD_VALUE", "up to 2000 chars, no control characters");
    }
    if !text.is_empty() {
        let sent = herdr::ok(&on, &herdr::send_text_argv(pane, text));
        sent.map_err(|_| anyhow::anyhow!("could not type into the sign-in pane"))?;
    }
    herdr::keys(&on, pane, "Enter").map(|_| "sent".into())
}
