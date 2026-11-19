use crate::config::Config;
use crate::events::{emit, fold, last_spawn, working, Event, Json};
use crate::hosts::On;
use crate::runner::{argv, json, Runner};
use crate::Ctx;
use anyhow::Result;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

/// The repo's opt-in to blocks' UI check (blocks#79 hook contract).
pub const CONFIG: &str = ".blocks/guard.json";

const RUN: &str = r#"o=$(mktemp) || exit 125
cd "$1" || exit 125
INTEGRATION="$2" GUARD_OUT="$o" just guard >&2
c=$?
cat "$o"
rm -f "$o"
exit $c"#;

const SHOWN: usize = 20;

/// `turn done`'s UI check for `role`'s lane: `Err` the `UI_CHECK` refusal (new findings, told once), else the result `turn-end` carries (`None`: the check is not due).
pub fn turn(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, n): (&str, u64),
    role: &str,
) -> Result<Option<Value>> {
    let f = cfg.fleet(fleet)?;
    let events = working(&ctx.state)?;
    let Some(s) = fold(&events).remove(&(fleet.to_string(), n)) else {
        return Ok(None);
    };
    let agent = s.lanes.get(role).cloned().unwrap_or_default();
    let Some(sp) = last_spawn(&events, &agent) else {
        return Ok(None);
    };
    let (on, wt) = (On::of(r, Some(sp)), sp.str("worktree").unwrap_or_default());
    let Some(setup) = on.read(&Path::new(wt).join(CONFIG)) else {
        return Ok(None);
    };
    let branch = crate::gate::branch_of(Some(&s), n);
    let fields = [
        "pr",
        "view",
        &branch,
        "-R",
        &f.repo,
        "--json",
        "number,headRefOid,state",
    ];
    let pr = json(r, "gh", &fields).unwrap_or_default();
    let head = pr.at("headRefOid").as_str().unwrap_or_default();
    if head.is_empty() || pr.at("state") != "OPEN" || s.reviewed(head) {
        return Ok(None);
    }
    let num = pr.at("number").to_string();
    let files = crate::gh::diff_names(ctx, r, f, (fleet, n), &num, head).unwrap_or_default();
    let setup: Value = serde_json::from_str(&setup).unwrap_or_default();
    let dirs = setup
        .at("screens")
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str);
    let screens: Vec<String> = dirs.map(|d| format!("{}/", d.trim_end_matches('/'))).collect();
    let guarded = |p: &&String| p.starts_with(".blocks/") || screens.iter().any(|d| p.starts_with(d.as_str()));
    let Some(why) = crate::verify::ui(&f.ui_paths, &files).or_else(|| files.iter().find(guarded)) else {
        return Ok(None);
    };
    writeln!(out, "UI check: {why} is in the diff; running `just guard` in {wt}")?;
    let start = crate::clock::instant();
    let args = argv(&["-c", RUN, "sh", wt, &f.integration]);
    let o = on.run_for("sh", &args, "", Duration::from_secs(cfg.limits.guard_secs));
    let mut d = json!({"sha": head, "agent": agent, "secs": start.elapsed().as_secs()});
    let code = o.as_ref().map_or(-1, |o| o.code);
    let outcome = o
        .as_ref()
        .ok()
        .and_then(|o| serde_json::from_str::<Value>(&o.stdout).ok());
    let list = |k: &str, v: &Value| v.at(k).as_array().cloned().unwrap_or_default();
    let log = |d: &Value| {
        emit(
            &ctx.state,
            cfg,
            Event::new(ctx.now, fleet, Some(n), "ui-check", d.clone()).passive(),
        )
    };
    let Some(v) = outcome.filter(|_| matches!(code, 0 | 1)) else {
        let err = o.map_or_else(|e| format!("{e:#}"), |o| o.stderr);
        let tail: String = (err.trim().lines().last().unwrap_or_default().chars())
            .take(200)
            .collect();
        let result = if code == 124 { "timeout" } else { "broken" };
        writeln!(out, "UI check {result} (exit {code}), the turn ends: {tail}")?;
        d.put("result", json!(result));
        d.put("error", json!(format!("exit {code}: {tail}")));
        log(&d)?;
        return Ok(Some(d));
    };
    d.put("known", json!(list("known", &v).len()));
    let new = list("new", &v);
    d.put("new", json!(new.len()));
    if new.is_empty() {
        d.put("result", json!("clean"));
        log(&d)?;
        return Ok(Some(d));
    }
    let key = |v: &Value| {
        ["rule", "file", "at"]
            .map(|k| v.at(k).as_str().unwrap_or_default())
            .join(" ")
    };
    let keys = json!(new.iter().map(key).collect::<Vec<_>>());
    let told =
        (events.iter()).rfind(|e| e.is("ui-check", fleet, n) && e.ts >= sp.ts && e.data.get("refused").is_some());
    let again = told.is_some_and(|e| e.data.get("keys") == Some(&keys));
    let lines = findings(&new);
    d.put("result", json!("new"));
    d.put("keys", keys);
    if again {
        d.put("findings", json!(lines));
        log(&d)?;
        writeln!(
            out,
            "UI check: the same findings as the refused `done`; the turn goes to review with them"
        )?;
        return Ok(Some(d));
    }
    d.put("refused", json!(true));
    log(&d)?;
    crate::gate::refused(
        "UI_CHECK",
        format!(
            "{} new finding(s) from `just guard`; fix them, then `drover turn done`:\n{lines}",
            new.len()
        ),
    )
}

fn findings(new: &[Value]) -> String {
    let s = |v: &Value, k: &str| v.at(k).as_str().unwrap_or_default().to_string();
    let line = |v: &Value| {
        let (rule, file, at) = (s(v, "rule"), s(v, "file"), s(v, "at"));
        let file = v.at("line").as_u64().map_or(file.clone(), |l| format!("{file}:{l}"));
        format!("{rule} {file} {at}: {}\n  fix: {}\n", s(v, "message"), s(v, "fix"))
    };
    let mut t: String = new.iter().take(SHOWN).map(line).collect();
    if let Some(more) = new.len().checked_sub(SHOWN).filter(|m| *m > 0) {
        t.push_str(&format!("and {more} more\n"));
    }
    t.trim_end().to_string()
}

/// Rev's brief section: the UI check findings the lane's last `turn done` took to review.
pub fn brief(events: &[Event], (fleet, n): (&str, u64)) -> Option<String> {
    let done = (events.iter()).rfind(|e| e.is("turn-end", fleet, n) && e.str("state") == Some("done"))?;
    let lines = done.val("ui_check").get("findings")?.as_str()?;
    Some(format!(
        "## UI check findings\n\nThe lane's `turn done` was refused `UI_CHECK` once and went on with these unchanged; judge each:\n\n{lines}"
    ))
}
