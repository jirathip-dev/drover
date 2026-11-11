//! #387: `herdsman config propose`: a fleet.toml change (roles, models, gates, land mode, serial groups, hosts, accounts)
//! as a thread card with [apply] [discard]. Only the owner's tap (or `config apply`, which the lane hook refuses in a lane) applies it, with a
//! backup, a `doctor` check and an event; settled, the card folds to the one line the thread records.
use crate::config::{self, Config};
use crate::devices::sha256;
use crate::events::{emit, read, Event};
use crate::gate::refused;
use crate::runner::Runner;
use crate::{orchestrator, Ctx};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

/// `herdsman config propose <thread> <file>`: `file` is the whole new fleet.toml; validated, its diff is the card.
pub fn propose(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, thread: &str, file: &Path) -> Result<()> {
    let events = read(&ctx.state)?;
    let t = orchestrator::find(&events, thread)?;
    let text = std::fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?;
    Config::parse(&text).context("the proposed fleet.toml is invalid")?;
    let cur = std::fs::read_to_string(&ctx.config)?;
    let diff = diff(&cur, &text);
    if diff.is_empty() {
        return refused("BAD_VALUE", "the proposal changes nothing");
    }
    let id = format!("c{}", events.iter().filter(|e| e.kind == "config-propose").count() + 1);
    let d = json!({"thread": thread, "id": id, "diff": diff, "text": text, "base": sha256(cur.as_bytes())});
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &t.fleet, None, "config-propose", d),
    )?;
    Ok(writeln!(out, "{id}")?)
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
    crate::orchestrator::of(events, thread).filter(move |e| {
        e.kind == "config-propose" && settled(events, e.str("id").unwrap_or_default()).is_none() && !stale(ctx, e)
    })
}

fn install(ctx: &Ctx, r: &dyn Runner, cur: &str, text: &str) -> Result<(Config, String, bool)> {
    let new = Config::parse(text)?;
    let backup = format!("{}.bak-{}", ctx.config.display(), ctx.now);
    std::fs::write(&backup, cur)?;
    crate::devices::write0600(&ctx.config, text.as_bytes())?;
    crate::spawn::profiles_sync(ctx, &new, &crate::cli::usage_doc(), &mut vec![])?;
    let ok = crate::doctor::doctor(ctx, r, &mut vec![], false, false)? == 0;
    Ok((new, backup, ok))
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
        let (new, backup, ok) = install(ctx, r, &cur, p.str("text").unwrap_or_default())?;
        d.as_object_mut()
            .unwrap()
            .extend([("backup".into(), json!(backup)), ("doctor".into(), json!(ok))]);
        // #558: each fleet it adds has its first queued issue kicked now, not at the next pass.
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
    let _ = orchestrator::say(ctx, cfg, r, (thread, &line, &[]), ("system", false));
    Ok(line)
}

/// #786: `GET /api/v1/config`: fleet.toml as cooee's config screens read it.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(optional_fields = nullable))]
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ConfigView {
    /// The file's sha256: a manual edit sends it back and is refused `STALE` once the file moved.
    pub hash: String,
    /// The raw TOML for the editor (a secret-shaped value never loads: `SECRET_IN_CONFIG`).
    pub text: String,
    pub fields: Vec<ConfigField>,
}

/// #786: one setting: its dotted path, TOML kind, allowed values, doc line and `secret:NAME`'s name.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(optional_fields = nullable))]
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ConfigField {
    pub path: String,
    pub kind: String,
    pub value: Value,
    pub allowed: Vec<String>,
    pub doc: String,
    pub secret: Option<String>,
}

/// #786: `config-check` and `config-edit`'s body: raw TOML, field edits, a fleet added or removed, at `hash`.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(optional_fields = nullable))]
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ConfigChange {
    pub hash: String,
    pub text: Option<String>,
    pub set: Vec<ConfigSet>,
    pub add: Option<NewFleet>,
    pub remove: Option<String>,
    pub confirm: bool,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(optional_fields = nullable))]
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ConfigSet {
    pub path: String,
    pub value: Value,
}

/// #786: a fleet from a GitHub repo; its roles copied from the first fleet's.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(optional_fields = nullable))]
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct NewFleet {
    pub name: String,
    pub repo: String,
    pub local: String,
    pub integration: String,
}

/// #786: `POST /api/v1/config-check`'s answer: the change's diff and its errors; nothing written.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(optional_fields = nullable))]
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ConfigCheck {
    pub diff: String,
    pub errors: Vec<String>,
}

/// #786: every setting of fleet.toml, its raw text and hash.
pub fn view(ctx: &Ctx) -> Result<ConfigView> {
    let text = std::fs::read_to_string(&ctx.config)?;
    let cfg = Config::parse(&text)?;
    let docs: BTreeMap<&str, &str> = include_str!("../../../fleet.example.toml")
        .lines()
        .filter_map(|l| {
            let (k, rest) = l.trim_start_matches(['#', ' ']).split_once('=')?;
            let k =
                Some(k.trim()).filter(|k| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))?;
            Some((k, rest.split_once(" # ")?.1.trim()))
        })
        .rev()
        .collect();
    let mut leaves = vec![];
    walk(String::new(), toml::from_str(&text)?, &mut leaves);
    let fields = leaves.into_iter().map(|(path, v)| {
        let key = path.rsplit('.').next().unwrap_or_default();
        let allowed = match key {
            "harness" => crate::providers::lane_names().into_iter().map(String::from).collect(),
            "effort" => crate::providers::EFFORTS.iter().map(|e| (*e).into()).collect(),
            "model" => cfg.models.clone(),
            _ => vec![],
        };
        ConfigField {
            kind: v.type_str().into(),
            secret: v.as_str().and_then(|s| s.strip_prefix("secret:")).map(String::from),
            allowed,
            doc: docs.get(key).copied().unwrap_or_default().into(),
            value: serde_json::to_value(v).unwrap_or_default(),
            path,
        }
    });
    Ok(ConfigView {
        hash: sha256(text.as_bytes()),
        fields: fields.collect(),
        text,
    })
}

fn walk(path: String, v: toml::Value, out: &mut Vec<(String, toml::Value)>) {
    let at = |k: &dyn std::fmt::Display| format!("{path}.{k}").trim_start_matches('.').to_string();
    match v {
        toml::Value::Table(t) => t.into_iter().for_each(|(k, v)| walk(at(&k), v, out)),
        toml::Value::Array(a) if a.iter().any(toml::Value::is_table) => {
            a.into_iter().enumerate().for_each(|(i, v)| walk(at(&i), v, out));
        }
        v => out.push((path, v)),
    }
}

fn changed(cfg: &Config, cur: &str, c: &ConfigChange) -> Result<String> {
    let mut text = c.text.clone().unwrap_or_else(|| cur.into());
    for s in &c.set {
        let lit = toml::Value::try_from(&s.value)?.to_string();
        text = match s.path.rsplit_once('.') {
            Some((t, k)) => config::set_in(&text, &format!("[{t}]"), k, &lit)?,
            None => config::set_in(&format!("[_]\n{text}"), "[_]", &s.path, &lit)?[4..].into(),
        };
    }
    if let Some(f) = &c.remove {
        cfg.fleet(f)?;
        let mut keep = true;
        text = (text.split_inclusive('\n'))
            .filter(|l| {
                if let Some(t) = table(l) {
                    keep = t != format!("fleet.{f}") && !t.starts_with(&format!("fleet.{f}."));
                }
                keep
            })
            .collect();
    }
    let Some(a) = &c.add else { return Ok(text) };
    let bad = a.name.is_empty() || !a.name.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c));
    if bad || cfg.fleet.contains_key(&a.name) {
        return refused(
            "BAD_VALUE",
            format!("\"{}\" is no new fleet name (letters, digits, - and _)", a.name),
        );
    }
    let prefix = format!("fleet.{}.role.", cfg.fleet.keys().next().map_or("", String::as_str));
    let (mut roles, mut on) = (String::new(), false);
    for l in cur.split_inclusive('\n') {
        if let Some(t) = table(l) {
            on = t.starts_with(&prefix);
            if on {
                roles += &format!("\n[fleet.{}.role.{}]\n", a.name, &t[prefix.len()..]);
                continue;
            }
        }
        roles += if on { l.trim_end_matches('\n') } else { continue };
        roles += "\n";
    }
    let q = |s: &str| toml::Value::from(s).to_string();
    Ok(format!(
        "{}\n\n[fleet.{}]\nrepo = {}\nlocal = {}\nintegration = {}\n{roles}",
        text.trim_end(),
        a.name,
        q(&a.repo),
        q(&a.local),
        q(&a.integration)
    ))
}

fn table(l: &str) -> Option<&str> {
    let t = l.trim_start().strip_prefix('[')?.trim_start_matches('[');
    Some(t.split(']').next()?.trim())
}

/// #786: `c`'s diff and errors over the current fleet.toml, writing nothing.
pub fn check(ctx: &Ctx, cfg: &Config, c: &ConfigChange) -> Result<ConfigCheck> {
    let cur = std::fs::read_to_string(&ctx.config)?;
    let (err, text) = match changed(cfg, &cur, c) {
        Ok(t) => (Config::parse(&t).err(), t),
        Err(e) => (Some(e), cur.clone()),
    };
    Ok(ConfigCheck {
        diff: diff(&cur, &text),
        errors: err.map(|e| vec![format!("{e:#}")]).unwrap_or_default(),
    })
}

/// #786: the owner's manual edit: refused `STALE` if fleet.toml moved since `c.hash`, logged `config-edit` `by`.
pub fn edit(ctx: &Ctx, cfg: &Config, r: &dyn Runner, c: &ConfigChange, by: &str) -> Result<String> {
    let cur = std::fs::read_to_string(&ctx.config)?;
    if sha256(cur.as_bytes()) != c.hash {
        return refused("STALE", "fleet.toml changed since it was read; read it again");
    }
    let text = changed(cfg, &cur, c)?;
    if let Err(e) = Config::parse(&text) {
        return refused("BAD_VALUE", format!("{e:#}"));
    }
    let diff = diff(&cur, &text);
    if diff.is_empty() {
        return refused("BAD_VALUE", "the edit changes nothing");
    }
    if let Some(f) = c.remove.as_deref().filter(|_| !c.confirm) {
        let live = crate::check::live_lanes(ctx, cfg, r)?;
        let events = read(&ctx.state)?;
        let mut lanes = (events.iter().filter(|e| e.kind == "spawn" && e.fleet == f)).filter_map(|e| e.str("agent"));
        if let Some(l) = lanes.find(|a| live.contains(*a)) {
            return refused(
                "LANE_LIVE",
                format!("{f} has live lanes ({l}); confirm to remove its config anyway"),
            );
        }
    }
    let (new, backup, ok) = install(ctx, r, &cur, &text)?;
    let fleet = c
        .add
        .as_ref()
        .map(|a| a.name.as_str())
        .or(c.remove.as_deref())
        .unwrap_or("all");
    let d = json!({"by": by, "diff": diff, "backup": backup, "doctor": ok, "added": c.add.as_ref().map(|a| &a.name), "removed": c.remove});
    emit(&ctx.state, &new, Event::new(ctx.now, fleet, None, "config-edit", d))?;
    Ok(format!(
        "fleet.toml edited by {by}, backup kept, doctor {}",
        ["✗ (run herdsman doctor)", "✓"][usize::from(ok)]
    ))
}
