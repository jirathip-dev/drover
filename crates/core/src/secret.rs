use crate::config::Config;
use crate::events::{emit, working, Event};
use crate::gate::refused;
use crate::runner::Runner;
use crate::{orchestrator, Ctx};
use anyhow::{Context, Result};
use serde_json::json;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A secret's name: `A-Z`, `0-9`, `_`.
pub fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
}

/// `<config dir>/secrets/<name>`, where a `secret:<name>` env is read from.
pub fn path(ctx: &Ctx, name: &str) -> PathBuf {
    ctx.config.with_file_name("secrets").join(name)
}

/// The secret in file `p`, trimmed (None: no such file); refused `SECRET_FILE_OPEN` when group or others may read it.
pub fn file(p: &Path) -> Result<Option<String>> {
    let Ok(m) = std::fs::metadata(p) else {
        return Ok(None);
    };
    if m.permissions().mode() & 0o077 != 0 {
        let fix = format!("chmod 600 {}", p.display());
        return refused(
            "SECRET_FILE_OPEN",
            format!("{} is readable by others → {fix}", p.display()),
        );
    }
    let v = std::fs::read_to_string(p).with_context(|| format!("secret file {}", p.display()))?;
    Ok(Some(v.trim().to_string()).filter(|v| !v.is_empty()))
}

/// `drover secret request <thread> <NAME> --why`: a masked-input card in the thread.
pub fn request(ctx: &Ctx, cfg: &Config, thread: &str, name: &str, why: &str) -> Result<()> {
    if !name_ok(name) || why.trim().is_empty() {
        return refused(
            "BAD_VALUE",
            "a secret's name is A-Z, 0-9 and _, and --why says what it is for",
        );
    }
    let t = orchestrator::find(&working(&ctx.state)?, thread)?.clone();
    let d = json!({"thread": thread, "name": name, "why": why});
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &t.fleet, None, "secret-request", d),
    )
}

/// Thread `id`'s open request for `name` (no `secret-set` or `secret-dismissed` after it).
pub fn open<'a>(events: &'a [Event], id: &str, name: &str) -> Option<&'a Event> {
    let mine = |e: &&Event| e.kind.starts_with("secret-") && e.str("name") == Some(name);
    crate::orchestrator::of(events, id)
        .rfind(mine)
        .filter(|e| e.kind == "secret-request")
}

/// A card's [save] (`value`, from a paired owner device: written 0600 to `path`) or [dismiss] (None); the line the thread records.
pub fn settle(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    (id, name): (&str, &str),
    value: Option<&str>,
) -> Result<String> {
    let events = working(&ctx.state)?;
    let Some(req) = open(&events, id, name) else {
        return refused("NO_SUCH_REQUEST", format!("{id} has no open request for secret {name}"));
    };
    let d = json!({"thread": id, "name": name, "by": "owner"});
    let kind = if let Some(v) = value {
        if v.is_empty() || v.len() > 8192 {
            return refused("BAD_VALUE", "a secret is 1 to 8,192 bytes");
        }
        crate::hosts::On::new(r, None).write_private(&path(ctx, name), v)?;
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
    let _ = orchestrator::say(ctx, cfg, r, (id, line, &[]), ("system", false));
    Ok(line.clone())
}

/// `drover secret ls` (owner only: the lane hook refuses it in a lane): `<fleet> <NAME>` of each secret set and not removed since.
pub fn ls(ctx: &Ctx, out: &mut dyn Write) -> Result<()> {
    let mut set = std::collections::BTreeSet::new();
    for e in working(&ctx.state)?
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

/// `drover secret rm <fleet|all> <NAME>` (owner only): delete `<config>/secrets/<NAME>`; logs `secret-rm`.
pub fn rm(ctx: &Ctx, cfg: &Config, fleet: &str, name: &str) -> Result<()> {
    if !name_ok(name) {
        return refused("BAD_VALUE", "a secret's name is A-Z, 0-9 and _");
    }
    match std::fs::remove_file(path(ctx, name)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e)?,
        _ => {}
    }
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, fleet, None, "secret-rm", json!({"name": name})),
    )
}

/// #707 `drover migrate`: the old store's Keychain items are left alone; the commands that delete them (each `secret-set` the log `state` names, and a search for the rest), for the owner to run.
pub fn retired(state: &Path, out: &mut dyn Write) -> Result<()> {
    let set = working(state).unwrap_or_default();
    let svc: std::collections::BTreeSet<String> = (set.iter().filter(|e| e.kind == "secret-set"))
        .map(|e| format!("herdsman/{}/{}", e.fleet, e.str("name").unwrap_or_default()))
        .collect();
    writeln!(
        out,
        "drover keeps no secret store now (#707); delete its old Keychain items yourself:"
    )?;
    for s in svc {
        writeln!(out, "  security delete-generic-password -a herdsman -s {s}")?;
    }
    writeln!(out, "  (list any others: security dump-keychain | grep '\"herdsman/')")?;
    Ok(())
}
