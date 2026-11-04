//! #387: secrets in the host's keychain as `herdsman/<fleet>/<NAME>`. The owner saves one from a thread's masked card
//! (paired devices only); a lane gets it from `secret:<NAME>` in its fleet's `env`, read by its own shell at spawn. The
//! value never reaches the event log, the chat, a command line or a lane's output.
use crate::config::Config;
use crate::events::{emit, read, Event};
use crate::gate::refused;
use crate::runner::{argv, Runner};
use crate::{conductor, Ctx};
use anyhow::{bail, Result};
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

/// The shell expression a lane's pane evaluates for `secret:<name>`: the fleet's, else the `all` one.
pub fn shell(fleet: &str, name: &str) -> String {
    let get = |f: &str| {
        format!(
            "security find-generic-password -a herdsman -s '{}' -w",
            service(f, name)
        )
    };
    format!("\"$({} 2>/dev/null || {})\"", get(fleet), get("all"))
}

/// The keychain item's password (`all`'s `TELEGRAM_BOT_TOKEN`, …), for herdsman's own use.
pub fn get(r: &dyn Runner, fleet: &str, name: &str) -> Result<String> {
    let svc = service(fleet, name);
    let o = r.run(
        "security",
        &argv(&["find-generic-password", "-a", "herdsman", "-s", &svc, "-w"]),
    )?;
    if o.code != 0 {
        bail!("no keychain item {svc}: `herdsman secret request <thread> {name}` asks the owner for it");
    }
    Ok(o.stdout.trim_end_matches('\n').into())
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
        // `-X` takes it hex-encoded, on stdin: never argv, so never `ps`.
        let hex: String = v.bytes().map(|b| format!("{b:02x}")).collect();
        let add = format!("add-generic-password -U -a herdsman -s {svc} -X {hex}\n");
        let o = r.run_in("security", &argv(&["-i"]), &add)?;
        let has = r.run(
            "security",
            &argv(&["find-generic-password", "-a", "herdsman", "-s", &svc]),
        )?;
        if o.code != 0 || has.code != 0 {
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
    let svc = service(fleet, name);
    crate::runner::ok(
        r,
        "security",
        &["delete-generic-password", "-a", "herdsman", "-s", &svc],
    )?;
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, fleet, None, "secret-rm", json!({"name": name})),
    )
}
