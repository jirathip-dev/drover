//! #387: `herdsman config propose`: a fleet.toml change (roles, models, gates, land mode, serial groups, hosts, accounts)
//! as a thread card with [apply] [discard]. Only the owner's tap (or `config apply`, which the lane hook refuses in a lane) applies it, with a
//! backup, a `doctor` check and an event; settled, the card folds to the one line the thread records.
use crate::config::Config;
use crate::devices::sha256;
use crate::events::{emit, read, Event};
use crate::gate::refused;
use crate::runner::Runner;
use crate::{conductor, Ctx};
use anyhow::{Context, Result};
use serde_json::json;
use std::io::Write;
use std::path::Path;

/// `herdsman config propose <thread> <file>`: `file` is the whole new fleet.toml; validated, its diff is the card.
pub fn propose(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, thread: &str, file: &Path) -> Result<()> {
    let text = std::fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?;
    Ok(writeln!(out, "{}", card(ctx, cfg, thread, &text, json!({}))?)?)
}

/// The new fleet.toml `text` as a card in `thread`, `extra` merged into its event; its id.
pub fn card(ctx: &Ctx, cfg: &Config, thread: &str, text: &str, extra: serde_json::Value) -> Result<String> {
    let events = read(&ctx.state)?;
    let t = conductor::find(&events, thread)?;
    Config::parse(text).context("the proposed fleet.toml is invalid")?;
    let cur = std::fs::read_to_string(&ctx.config)?;
    let diff = diff(&cur, text);
    if diff.is_empty() {
        return refused("BAD_VALUE", "the proposal changes nothing");
    }
    let id = format!("c{}", events.iter().filter(|e| e.kind == "config-propose").count() + 1);
    let mut d = json!({"thread": thread, "id": id, "diff": diff, "text": text, "base": sha256(cur.as_bytes())});
    if let serde_json::Value::Object(m) = extra {
        d.as_object_mut().unwrap().extend(m);
    }
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &t.fleet, None, "config-propose", d),
    )?;
    Ok(id)
}

/// #682 `[use]`: fleet.toml `text` with each role in `scope` (`all`, `<fleet>`, `<fleet>.<role>`) switched to `to`: an
/// `[accounts]` name first on its accounts harness's roles, `deepseek` as the `dsh` harness; the `<fleet>.<role>`s it changes.
pub fn switch(cfg: &Config, text: &str, scope: &str, to: &str) -> Result<(String, Vec<String>)> {
    let (sf, sr) = scope.split_once('.').map_or((scope, None), |(f, r)| (f, Some(r)));
    let deepseek = to == "deepseek";
    if !deepseek && !cfg.accounts.names.contains_key(to) {
        return refused("BAD_VALUE", format!("{to} is neither an [accounts] name nor deepseek"));
    }
    let (mut new, mut roles, acc) = (text.to_string(), vec![], crate::harness::accounts_named().map(|h| h.0));
    for (f, fc) in cfg.fleet.iter().filter(|(f, _)| sf == "all" || sf == *f) {
        for (name, r) in fc.role.iter().filter(|(n, r)| sr.is_none_or(|x| x == *n) && !r.default) {
            let (key, lit) = if deepseek {
                if r.harness == "dsh" {
                    continue;
                }
                ("harness", "\"dsh\"".to_string())
            } else if Some(r.harness.as_str()) != acc || r.account_list().first().map(String::as_str) == Some(to) {
                continue;
            } else if r.accounts.is_empty() {
                ("account", format!("\"{to}\""))
            } else {
                let rest = r.accounts.iter().filter(|a| *a != to).map(|a| format!(", \"{a}\""));
                ("accounts", format!("[\"{to}\"{}]", rest.collect::<String>()))
            };
            new = crate::config::set_in(&new, &format!("[fleet.{f}.role.{name}]"), key, &lit)?;
            roles.push(format!("{f}.{name}"));
        }
    }
    if roles.is_empty() {
        return refused("BAD_VALUE", format!("no role in {scope} to switch to {to}"));
    }
    Ok((new, roles))
}

/// The changed lines of `a` → `b` (`- ` / `+ `, by longest common subsequence), each run under its `[section]`.
pub fn diff(a: &str, b: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    let mut l = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            l[i][j] = if a[i] == b[j] {
                l[i + 1][j + 1] + 1
            } else {
                l[i + 1][j].max(l[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut out, mut sec, mut shown) = (0, 0, String::new(), "", "");
    while i < a.len() || j < b.len() {
        let line = if i < a.len() && j < b.len() && a[i] == b[j] {
            (i, j) = (i + 1, j + 1);
            sec = if a[i - 1].starts_with('[') { a[i - 1] } else { sec };
            continue;
        } else if i < a.len() && (j == b.len() || l[i + 1][j] >= l[i][j + 1]) {
            i += 1;
            format!("- {}\n", a[i - 1])
        } else {
            j += 1;
            format!("+ {}\n", b[j - 1])
        };
        if sec != shown {
            (out, shown) = (out + &format!("  {sec}\n"), sec);
        }
        out += &line;
    }
    out
}

/// Proposal `id`'s `config-settled`, if any.
pub fn settled<'a>(events: &'a [Event], id: &str) -> Option<&'a Event> {
    events
        .iter()
        .find(|e| e.kind == "config-settled" && e.str("id") == Some(id))
}

/// #460: proposal `p` was made over a fleet.toml that has changed since: it can no longer apply.
pub fn stale(ctx: &Ctx, p: &Event) -> bool {
    let cur = std::fs::read(&ctx.config).unwrap_or_default();
    p.str("base") != Some(&sha256(&cur))
}

/// #460: thread `thread`'s proposals that wait for the owner (neither settled nor stale), oldest first.
pub fn waiting<'a>(ctx: &'a Ctx, events: &'a [Event], thread: &'a str) -> impl Iterator<Item = &'a Event> {
    events.iter().filter(move |e| {
        e.kind == "config-propose"
            && e.str("thread") == Some(thread)
            && settled(events, e.str("id").unwrap_or_default()).is_none()
            && !stale(ctx, e)
    })
}

/// The owner's [apply] or [discard] on proposal `id`; the line the thread records.
pub fn settle(ctx: &Ctx, cfg: &Config, r: &dyn Runner, id: &str, apply: bool) -> Result<String> {
    let events = read(&ctx.state)?;
    let Some(p) = events
        .iter()
        .find(|e| e.kind == "config-propose" && e.str("id") == Some(id))
    else {
        return refused("NO_SUCH_PROPOSAL", format!("no config proposal {id}"));
    };
    if let Some(s) = settled(&events, id) {
        let how = if s.data["applied"] == true {
            "applied"
        } else {
            "discarded"
        };
        return refused("ALREADY_SETTLED", format!("{id} was {how}"));
    }
    let (thread, mut d) = (
        p.str("thread").unwrap_or_default(),
        json!({"thread": p.str("thread"), "id": id}),
    );
    let line = if apply {
        let cur = std::fs::read_to_string(&ctx.config)?;
        if stale(ctx, p) {
            return refused(
                "STALE",
                format!("fleet.toml changed since {id} was proposed; ask for a new one"),
            );
        }
        let backup = format!("{}.bak-{}", ctx.config.display(), ctx.now);
        std::fs::write(&backup, &cur)?;
        crate::devices::write0600(&ctx.config, p.str("text").unwrap_or_default().as_bytes())?;
        let ok = crate::doctor::doctor(ctx, r, &mut vec![], false, false)? == 0;
        d.as_object_mut()
            .unwrap()
            .extend([("backup".into(), json!(backup)), ("doctor".into(), json!(ok))]);
        // #558: each fleet it adds has its first queued issue kicked now, not at the next pass.
        let new = Config::parse(p.str("text").unwrap_or_default())?;
        let mut kicked = String::new();
        for (f, _) in new
            .fleet
            .iter()
            .filter(|(f, c)| !c.paused && !cfg.fleet.contains_key(*f))
        {
            if let Some(n) = crate::queue::startable(&events, f).first().and_then(|n| n.parse().ok()) {
                crate::drive::kick(ctx, r, f, n, "config-apply");
                kicked += &format!(", {f} #{n} kicked");
            }
        }
        format!(
            "config {id} applied by owner, backup kept, doctor {}{kicked}",
            ["✗ (run herdsman doctor)", "✓"][usize::from(ok)]
        )
    } else {
        format!("config {id} discarded by owner")
    };
    d["applied"] = json!(apply);
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &p.fleet, None, "config-settled", d),
    )?;
    let _ = conductor::say(ctx, cfg, r, (thread, &line, &[]), ("system", false)); // logged even if the conductor can't wake
    Ok(line)
}
