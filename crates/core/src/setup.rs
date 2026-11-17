use crate::events::{append, working, Event};
use crate::runner::{ok, Runner};
use crate::{config::Config, gate::refused, hosts::On, term, Ctx};
use anyhow::{Context, Result};
use serde_json::{json, Value};

pub const TIMEOUT: u64 = 600;
const GH_TOKEN: &str =
    "printf 'Fine-grained GitHub token: '; read -rs T; printf %s \"$T\" | gh auth login --with-token";

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
    let login = |h: crate::providers::Entry| Some((h.name, h.login()?));
    let logins = crate::providers::lanes()
        .filter_map(login)
        .chain([("gh", "gh auth login")]);
    for (n, c) in logins.chain(crate::providers::acp_only().filter_map(login)) {
        if !v.iter().any(|x| x.0 == n) {
            v.push((n.into(), c.into()));
        }
    }
    v
}

/// Write fleet `name`'s missing hygiene into its local clone (`[comments]`, `[docs]`, the first baseline); a row for each write.
pub fn hygiene(ctx: &Ctx, cfg: &Config, r: &dyn Runner, name: &str) -> Vec<String> {
    let (Some(f), h) = (cfg.fleet.get(name), cfg.hygiene(name)) else {
        return vec![];
    };
    let dir = f.local_path();
    if !dir.is_dir() {
        return vec![];
    }
    let path = dir.join(".drover/arch.toml");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let Ok(arch) = toml::from_str::<toml::Table>(&text) else {
        return vec![format!("✗ hygiene {name} — {} does not parse → fix it", path.display())];
    };
    let mut add = String::new();
    if h.comments.as_deref() == Some("strict") && !arch.contains_key("comments") {
        add += "\n[comments]\npolicy = \"strict\"\n";
    }
    if h.docs.as_deref() == Some("on") && !arch.contains_key("docs") {
        let files = ok(r, "git", &["-C", &dir.display().to_string(), "ls-files"]).unwrap_or_default();
        let stem = |p: &&str| {
            let n = p.rsplit('/').next().unwrap_or(p);
            ["cli", "config", "api"].contains(&n.split('.').next().unwrap_or(n))
        };
        let watch: Vec<_> = files.lines().filter(stem).map(|p| format!("{p:?}")).collect();
        add += &format!(
            "\n[docs]\npaths = [\"docs/**/*.md\", \"README.md\"]\nwatch = [{}]\n",
            watch.join(", ")
        );
    }
    let mut rows = vec![];
    if !add.is_empty() {
        let wrote = std::fs::create_dir_all(dir.join(".drover")).and_then(|()| std::fs::write(&path, text + &add));
        rows.push(match wrote {
            Ok(()) => format!("✓ hygiene {name} — wrote {} → commit it", path.display()),
            Err(e) => format!("✗ hygiene {name} — {}: {e}", path.display()),
        });
    }
    let base = arch
        .get("baseline")
        .and_then(|b| b.as_str())
        .unwrap_or(".drover/arch-baseline.txt");
    if h.baseline.as_deref() == Some("setup") && path.exists() && !dir.join(base).exists() {
        let q = |p: &str| format!("'{}'", p.replace('\'', "'\\''"));
        let line = format!(
            "cd {} && {} arch-check --write-baseline",
            q(&dir.display().to_string()),
            q(&ctx.exe)
        );
        rows.push(match ok(r, "sh", &["-c", &line]) {
            Ok(_) => format!("✓ hygiene {name} — wrote the {base} baseline → commit it"),
            Err(e) => format!("✗ hygiene {name} — baseline: {e:#}"),
        });
    }
    rows
}

/// The vendor of each setup in `all` that is a provider's login, by its name.
pub fn vendors(cfg: &Config) -> Vec<(String, String)> {
    let of = |(n, _): (String, String)| Some((n.clone(), crate::providers::get(&n).vendor()?.into()));
    all(cfg).into_iter().filter_map(of).collect()
}

/// Start setup `name` on `host` in a new shed terminal there; logs `setup` (no command line, no input); its name.
pub fn start(ctx: &Ctx, cfg: &Config, r: &dyn Runner, host: &str, name: &str, via: &str) -> Result<String> {
    let boot = ("gh-token".to_string(), GH_TOKEN.to_string());
    let Some((_, line)) = all(cfg).into_iter().chain([boot]).find(|s| s.0 == name) else {
        return refused("NO_SUCH_SETUP", format!("no setup `{name}`"));
    };
    let h = cfg.hosts.get(host);
    if h.is_none() && !cfg.hosts.is_empty() {
        return refused("NO_SUCH_HOST", format!("no [hosts.{host}]"));
    }
    let on = On::new(r, h.and_then(|h| h.machine.as_deref()));
    let b = on.shed().context("no shed runtime")?;
    let pane = &format!(
        "drover-setup-{}",
        name.replace(|c: char| !c.is_ascii_alphanumeric(), "-")
    );
    term::Shed(crate::shed::Shed { acp: None, ..b }).start(ctx, &on, pane, &[], "/tmp", &term::Acp::default())?;
    term::ok(&on, &term::run_argv(pane, &[&format!("cd; {line}; exit")]))?;
    let d = json!({"host": host, "name": name, "pane": pane, "machine": on.machine, "via": via});
    append(&ctx.state, &Event::new(ctx.now, "", None, "setup", d))?;
    Ok(pane.into())
}

/// The open `setup` event of `pane` (no `setup-closed` after it).
pub fn open<'e>(events: &'e [Event], pane: &str) -> Option<&'e Event> {
    let i = events
        .iter()
        .rposition(|e| e.kind.starts_with("setup") && e.str("pane") == Some(pane))?;
    events.get(i).filter(|e| e.kind == "setup")
}

/// Close setup `e`'s terminal (its scrollback goes with it) and log `setup-closed`.
pub fn close(ctx: &Ctx, r: &dyn Runner, e: &Event, why: &str) -> Result<()> {
    let pane = e.str("pane").unwrap_or_default();
    term::close(ctx, &On::of(r, Some(e)), pane)?;
    let d = json!({"pane": pane, "why": why});
    append(&ctx.state, &Event::new(ctx.now, "", None, "setup-closed", d))
}

/// `check`: close every setup open `TIMEOUT` seconds.
pub fn reap(ctx: &Ctx, r: &dyn Runner, events: &[Event]) -> Result<()> {
    for e in events.iter().filter(|e| e.kind == "setup" && ctx.now >= e.ts + TIMEOUT) {
        if open(events, e.str("pane").unwrap_or_default()).is_some_and(|o| o.ts == e.ts) {
            close(ctx, r, e, "timeout").ok();
        }
    }
    Ok(())
}

/// The sheet's read of setup `pane`: its masked screen and every `https://` URL (a wrapped one joined), or closed.
pub fn screen(ctx: &Ctx, r: &dyn Runner, pane: &str) -> Result<Value> {
    let (events, shut) = (working(&ctx.state)?, json!({"closed": true, "lines": []}));
    let Some(e) = open(&events, pane) else { return Ok(shut) };
    let live = ctx.now < e.ts + TIMEOUT;
    let o = term::exec(&On::of(r, Some(e)), &term::read_argv(pane))?;
    if !live || term::failed_with(&o, "pane_not_found") {
        return close(ctx, r, e, if live { "exited" } else { "timeout" }).map(|()| shut);
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
        let more = ls
            .iter()
            .skip(i + 1)
            .take_while(|n| l.ends_with(u) && !n.is_empty() && !n.trim().contains(' '));
        urls.push(more.fold(u.to_string(), |u, n| u + n.trim()));
    }
    let lines: Vec<_> = ls.iter().map(|l| crate::lanes::mask(l)).collect();
    Ok(json!({"lines": lines, "urls": urls, "closed": false}))
}

/// The sheet's reply into setup `pane`: its text (keys and text only: no control characters), then Enter; never logged.
pub fn send(ctx: &Ctx, r: &dyn Runner, pane: &str, text: &str) -> Result<String> {
    let events = working(&ctx.state)?;
    let on = On::of(r, Some(open(&events, pane).context("that sign-in has closed")?));
    if text.chars().any(char::is_control) || text.chars().count() > 2000 {
        return refused("BAD_VALUE", "up to 2000 chars, no control characters");
    }
    if !text.is_empty() {
        let sent = term::ok(&on, &term::send_text_argv(pane, text));
        sent.map_err(|_| anyhow::anyhow!("could not type into the sign-in pane"))?;
    }
    term::keys(&on, pane, "Enter").map(|_| "sent".into())
}
