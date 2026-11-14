use crate::config::{write_atomic, Config};
use crate::events::Event;
use crate::gate::refused;
use crate::runner::Runner;
use crate::Ctx;
use anyhow::{ensure, Result};
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};

/// #935: the fleet `name` is now, after every `fleet-renamed` in order.
pub fn current(state: &Path, name: &str) -> String {
    let renames = crate::db::events(state, "type = 'fleet-renamed' AND seq > ?1", 0).unwrap_or_default();
    renames.iter().fold(name.to_string(), |n, e| {
        if e.str("old") == Some(n.as_str()) {
            e.fleet.clone()
        } else {
            n
        }
    })
}

fn name_ok(n: &str) -> bool {
    n.bytes()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && n.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
}

fn retable(text: &str, old: &str, new: &str) -> (String, Vec<String>) {
    let (mut out, mut changed) = (String::new(), vec![]);
    for l in text.split_inclusive('\n') {
        let t = l.trim_start().trim_start_matches('[');
        let header = l.trim_start().starts_with('[')
            && t.strip_prefix(&format!("fleet.{old}"))
                .is_some_and(|r| r.starts_with(['.', ']']));
        let n = if header {
            l.replacen(&format!("fleet.{old}"), &format!("fleet.{new}"), 1)
        } else if t.starts_with("fleets") {
            l.replace(&format!("\"{old}\""), &format!("\"{new}\""))
        } else {
            l.to_string()
        };
        if n != l {
            changed.push(format!("fleet.toml: {} → {}", l.trim(), n.trim()));
        }
        out += &n;
    }
    (out, changed)
}

fn files(ctx: &Ctx, cfg: &Config, old: &str, new: &str) -> Vec<(PathBuf, PathBuf)> {
    let dir = ctx.state.parent().unwrap_or(Path::new("."));
    let owns = |rest: &str, f: &str| {
        rest.strip_prefix(f)
            .is_some_and(|r| r.is_empty() || r.starts_with(['.', '-']))
    };
    let mut v: Vec<_> = (std::fs::read_dir(dir).into_iter().flatten().flatten())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let (word, rest) = name.split_once('-')?;
            let other = cfg
                .fleet
                .keys()
                .any(|f| f != old && f.starts_with(old) && owns(rest, f));
            (word.bytes().all(|c| c.is_ascii_lowercase()) && owns(rest, old) && !other)
                .then(|| (e.path(), dir.join(format!("{word}-{new}{}", &rest[old.len()..]))))
        })
        .collect();
    v.sort();
    v
}

/// #935 `drover fleet rename <old> <new> [--yes]`: every change printed; `yes` backs up and applies all or none.
pub fn run(ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, (old, new): (&str, &str), yes: bool) -> Result<()> {
    let text = std::fs::read_to_string(&ctx.config)?;
    let cfg = Config::parse(&text)?;
    let Some(f) = cfg.fleet.get(old) else {
        return refused("NO_SUCH_FLEET", format!("no fleet {old} in fleet.toml"));
    };
    let events = crate::events::history(&ctx.state)?;
    if !name_ok(new) || cfg.fleet.contains_key(new) || events.iter().any(|e| e.fleet == new) {
        let why = format!("{new} is taken or no fleet name (a-z, 0-9, - and _)");
        return refused("BAD_NAME", why);
    }
    if !f.paused {
        return refused("NOT_PAUSED", format!("pause {old} first: drover pause {old}"));
    }
    let states = crate::events::states(&ctx.state)?;
    let open = |a: &str| (states.values()).any(|s| s.fleet == old && !s.done() && s.lanes.values().any(|l| l == a));
    let agents = crate::host::agents(r)?;
    let live: Vec<&str> = agents
        .iter()
        .filter_map(|a| a["name"].as_str())
        .filter(|a| open(a))
        .collect();
    if !live.is_empty() {
        return refused(
            "LANE_LIVE",
            format!("{old} has live lanes {live:?}: let them finish or kill them"),
        );
    }
    let (new_text, mut lines) = retable(&text, old, new);
    let after = Config::parse(&new_text)?;
    let (a, b): (toml::Table, toml::Table) = (text.parse()?, new_text.parse()?);
    ensure!(
        a["fleet"].get(old) == b["fleet"].get(new) && after.fleet.contains_key(new),
        "fleet.toml: [fleet.{old}] did not move to [fleet.{new}] unchanged"
    );
    let mine: Vec<&Event> = events.iter().filter(|e| e.fleet == old).collect();
    lines.push(format!(
        "state.db: {} events (queue, history, asks, verdicts, threads) {old} → {new}",
        mine.len()
    ));
    let secrets: std::collections::BTreeSet<&str> = mine
        .iter()
        .filter(|e| e.kind == "secret-set")
        .filter_map(|e| e.str("name"))
        .collect();
    lines.extend(secrets.iter().map(|s| format!("secret {s}: {old} → {new}")));
    let moves = files(ctx, &cfg, old, new);
    let name = |p: &Path| p.file_name().unwrap_or_default().to_string_lossy().into_owned();
    lines.extend(moves.iter().map(|(a, b)| format!("file {} → {}", name(a), name(b))));
    lines.push(format!("new lanes spawn as {new}-<role>-<issue>; {new} stays paused"));
    let bak = ctx.state.with_file_name(format!("rename-{old}-{}", ctx.now));
    lines.push(format!("backup: {}", bak.display()));
    lines.iter().try_for_each(|l| writeln!(out, "{l}"))?;
    if !yes {
        writeln!(out, "dry run: --yes applies it")?;
        return Ok(());
    }
    std::fs::create_dir_all(&bak)?;
    std::fs::copy(&ctx.config, bak.join("fleet.toml"))?;
    for (a, _) in &moves {
        std::fs::copy(a, bak.join(name(a)))?;
    }
    let ev = Event::new(
        ctx.now,
        new,
        None,
        "fleet-renamed",
        json!({"old": old, "new": new, "by": "owner"}),
    );
    let done = crate::db::rename_fleet(&ctx.state, (old, new), &ev, &bak.join("state.db"), || {
        moves.iter().try_for_each(|(a, b)| std::fs::rename(a, b))?;
        write_atomic(&ctx.config, &new_text)
    });
    if let Err(e) = done {
        for (a, b) in moves.iter().filter(|(a, _)| !a.exists()) {
            std::fs::copy(bak.join(name(a)), a)?;
            std::fs::remove_file(b)?;
        }
        write_atomic(&ctx.config, &text)?;
        return Err(e.context(format!(
            "fleet rename failed: nothing moved, restored from {}",
            bak.display()
        )));
    }
    writeln!(out, "{old} renamed {new}: resume it with drover resume {new}")?;
    Ok(())
}
