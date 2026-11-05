//! #387: secrets as `herdsman/<fleet>/<NAME>`; #441: only in the main host's OS store (`Runner::secret`, the keyring crate).
//! The owner saves one from a thread's masked card (paired devices only) or imports `tokens.env`; spawn delivers a
//! lane's `secret:<NAME>` env and account token to its host (`spawn::deliver`). The value never reaches the event
//! log, the chat, a command line or a lane's output.
use crate::config::Config;
use crate::events::{emit, read, Event};
use crate::gate::refused;
use crate::runner::Runner;
use crate::{conductor, Ctx};
use anyhow::{bail, Context, Result};
use serde_json::json;
use std::io::Write;

/// The keychain service of `fleet`'s (or `all`'s) secret `name`.
pub fn service(fleet: &str, name: &str) -> String {
    format!("herdsman/{fleet}/{name}")
}

/// A secret's name: `A-Z`, `0-9`, `_`.
pub fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
}

/// `fleet`'s secret `name`, else the `all` one; None when neither is set.
pub fn lookup(r: &dyn Runner, fleet: &str, name: &str) -> Result<Option<String>> {
    match r.secret(&service(fleet, name))? {
        None if fleet != "all" => r.secret(&service("all", name)),
        v => Ok(v),
    }
}

/// #441: a main host with no usable OS store refuses at startup; there is no plain-file fallback.
pub fn probe(r: &dyn Runner) -> Result<()> {
    match r.secret(&service("all", "HERDSMAN_PROBE")) {
        Ok(_) => Ok(()),
        Err(e) => refused(
            "NO_STORE",
            format!("this host has no usable secret store ({e:#}); herdsman keeps secrets nowhere else"),
        ),
    }
}

/// The keychain item's password (`all`'s `TELEGRAM_BOT_TOKEN`, …), for herdsman's own use.
pub fn get(r: &dyn Runner, fleet: &str, name: &str) -> Result<String> {
    let svc = service(fleet, name);
    r.secret(&svc)?.with_context(|| {
        format!("no keychain item {svc}: `herdsman secret request <thread> {name}` asks the owner for it")
    })
}

/// `herdsman secret request <thread> <NAME> --why`: a masked-input card in the thread.
pub fn request(ctx: &Ctx, cfg: &Config, thread: &str, name: &str, why: &str) -> Result<()> {
    if !name_ok(name) || why.trim().is_empty() {
        return refused(
            "BAD_VALUE",
            "a secret's name is A-Z, 0-9 and _, and --why says what it is for",
        );
    }
    let t = conductor::find(&read(&ctx.state)?, thread)?.clone();
    let d = json!({"thread": thread, "name": name, "why": why});
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &t.fleet, None, "secret-request", d),
    )
}

/// Thread `id`'s open request for `name` (no `secret-set` or `secret-dismissed` after it).
pub fn open<'a>(events: &'a [Event], id: &str, name: &str) -> Option<&'a Event> {
    let mine = |e: &&Event| e.kind.starts_with("secret-") && e.str("thread") == Some(id) && e.str("name") == Some(name);
    events.iter().rfind(mine).filter(|e| e.kind == "secret-request")
}

/// A card's [save] (`value`, from a paired owner device) or [dismiss] (None); the line the thread records.
pub fn settle(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    (id, name): (&str, &str),
    value: Option<&str>,
) -> Result<String> {
    let events = read(&ctx.state)?;
    let Some(req) = open(&events, id, name) else {
        return refused("NO_SUCH_REQUEST", format!("{id} has no open request for secret {name}"));
    };
    let (svc, d) = (
        service(&req.fleet, name),
        json!({"thread": id, "name": name, "by": "owner"}),
    );
    let kind = if let Some(v) = value {
        if v.is_empty() || v.len() > 8192 {
            return refused("BAD_VALUE", "a secret is 1 to 8,192 bytes");
        }
        r.set_secret(&svc, Some(v))?;
        if r.secret(&svc)?.as_deref() != Some(v) {
            bail!("the keychain did not take {svc}");
        }
        "secret-set"
    } else {
        "secret-dismissed"
    };
    emit(&ctx.state, cfg, Event::new(ctx.now, &req.fleet, None, kind, d))?;
    let line = [
        format!("secret {name} dismissed (by owner)"),
        format!("secret {name} set ✓ (by owner)"),
    ];
    let line = &line[usize::from(value.is_some())];
    let _ = conductor::say(ctx, cfg, r, (id, line, &[]), ("system", false)); // logged even if the conductor can't wake
    Ok(line.clone())
}

/// `herdsman secret ls` (owner only: the lane hook refuses it in a lane): `<fleet> <NAME>` of each secret set and not removed since.
pub fn ls(ctx: &Ctx, out: &mut dyn Write) -> Result<()> {
    let mut set = std::collections::BTreeSet::new();
    for e in read(&ctx.state)?
        .iter()
        .filter(|e| e.kind == "secret-set" || e.kind == "secret-rm")
    {
        let k = (e.fleet.clone(), e.str("name").unwrap_or_default().to_string());
        let _ = if e.kind == "secret-set" {
            set.insert(k)
        } else {
            set.remove(&k)
        };
    }
    set.iter().try_for_each(|(f, n)| writeln!(out, "{f} {n}"))?;
    Ok(())
}

/// `herdsman secret rm <fleet|all> <NAME>` (owner only): delete the keychain item; logs `secret-rm`.
pub fn rm(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str, name: &str) -> Result<()> {
    r.set_secret(&service(fleet, name), None)?;
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, fleet, None, "secret-rm", json!({"name": name})),
    )
}

/// #441 `herdsman accounts import [FILE]` (owner only): each `[accounts]` token in the old `tokens.env` copied into the
/// store as `all`'s secret of its variable name, read back and checked by sha256; the file is left for the owner.
pub fn import(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, file: &str) -> Result<()> {
    use sha2::{Digest, Sha256};
    let path = crate::config::expand(file);
    let text = std::fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
    let hash = |v: &str| Sha256::digest(v.as_bytes());
    for (account, var) in &cfg.accounts.names {
        let Some(v) = crate::usage::env_value(&text, var).filter(|v| !v.is_empty()) else {
            writeln!(out, "{account:<8} ✗ {var} not in {}", path.display())?;
            continue;
        };
        let svc = service("all", var);
        r.set_secret(&svc, Some(&v))?;
        let back = r.secret(&svc)?.unwrap_or_default();
        anyhow::ensure!(
            hash(&back) == hash(&v),
            "{svc}: the store's copy does not match {}",
            path.display()
        );
        let d = json!({"name": var, "account": account, "by": "owner"});
        emit(&ctx.state, cfg, Event::new(ctx.now, "all", None, "secret-set", d))?;
        writeln!(out, "{account:<8} ✓ {svc} (sha256 matches)")?;
    }
    writeln!(
        out,
        "lanes now read the store, never {}; delete it when you are ready",
        path.display()
    )?;
    Ok(())
}
