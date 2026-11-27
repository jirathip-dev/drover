use crate::config::{Config, Fleet};
use crate::events::{append, emit, fold, last_spawn, working, Event, Json};
use crate::hosts::On;
use crate::runner::{argv, json, Runner};
use crate::Ctx;
use anyhow::{Context, Result};
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

/// The refusal code of a `turn done` whose head is unpushed or fails a done check.
pub const CODE: &str = "DONE_CHECK";

type Checked = Option<(String, Option<(&'static str, String)>)>;

/// `turn done`'s hard gate for `role`'s lane: the head it passed at (None: no open PR with an unreviewed head); a refusal is logged `done-refused` (code, why, head) and returned as `REFUSED <code>`.
pub fn turn(ctx: &Ctx, cfg: &Config, r: &dyn Runner, (fleet, n): (&str, u64), role: &str) -> Result<Option<String>> {
    let Some((head, refused)) = check(ctx, cfg, r, (fleet, n), role)? else {
        return Ok(None);
    };
    let Some((code, why)) = refused else {
        return Ok(Some(head));
    };
    let d = json!({"code": code, "why": why, "sha": head, "role": role});
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, fleet, Some(n), "done-refused", d).passive(),
    )?;
    crate::gate::refused(code, why)
}

fn check(ctx: &Ctx, cfg: &Config, r: &dyn Runner, (fleet, n): (&str, u64), role: &str) -> Result<Checked> {
    let f = cfg.fleet(fleet)?;
    let events = working(&ctx.state)?;
    let Some(s) = fold(&events).remove(&(fleet.to_string(), n)) else {
        return Ok(None);
    };
    let agent = s.lanes.get(role).cloned().unwrap_or_default();
    let (Some(sp), branch) = (last_spawn(&events, &agent), crate::gate::branch_of(Some(&s), n)) else {
        return Ok(None);
    };
    let fields = ["pr", "view", &branch, "-R", &f.repo, "--json", "body,headRefOid,state"];
    let pr = json(r, "gh", &fields).unwrap_or_default();
    let head = pr.at("headRefOid").as_str().unwrap_or_default().to_string();
    if head.is_empty() || pr.at("state") != "OPEN" || s.reviewed(&head) {
        return Ok(None);
    }
    let (on, wt) = (On::of(r, Some(sp)), sp.str("worktree").unwrap_or_default());
    let git = |a: &[&str]| {
        let mut last = String::new();
        for _ in 0..3 {
            match on.run("git", &argv(&[&["-C", wt][..], a].concat())) {
                Ok(o) if o.code == 0 && !o.stdout.trim().is_empty() => return Ok(o.stdout.trim().to_string()),
                Ok(o) => last = format!("exit {}{}", o.code, if o.code == 0 { ", no output" } else { "" }),
                Err(e) => last = format!("{e:#}"),
            }
        }
        Err(format!(
            "could not check: `git {}` in the worktree failed 3 times ({last}): retry `drover turn done`",
            a.join(" ")
        ))
    };
    let refuse = |code: &'static str, why: String| Ok(Some((head.clone(), Some((code, why)))));
    let at = git(&["rev-parse", "HEAD"]).or_else(|e| crate::gate::refused(CODE, e))?;
    if !at.eq_ignore_ascii_case(&head) {
        let why = format!("push first: the worktree is at {at:.7}, the PR head is {head:.7}");
        return refuse(CODE, why);
    }
    let failed = std::cell::RefCell::new(None);
    let read = |a: &[&str]| git(a).map_err(|e| failed.replace(Some(e))).ok();
    let subject = |c: &str| read(&["log", "-1", "--format=%s", c]);
    let parent = |c: &str| read(&["rev-parse", &format!("{c}^1")]);
    let text = pr.at("body").as_str().unwrap_or_default();
    if let Err(e) = body(text, &head, &f.integration, &subject, &parent) {
        if let Some(e) = failed.take() {
            return crate::gate::refused(CODE, e);
        }
        return refuse("PR_BODY", e);
    }
    let limit = Duration::from_secs(cfg.limits.gate_check_minutes * 60);
    let pr = crate::gatekeeper::Pr {
        head: &head,
        body: text,
    };
    let heavy = crate::heavy::Heavy::at(cfg, f, sp.str("host"));
    let exe = if on.machine.is_some() { "drover" } else { &ctx.exe };
    for c in checks(f, &on, Path::new(wt)) {
        let o = if c == crate::gatekeeper::CHECK {
            crate::gatekeeper::check(&on, Path::new(wt), &pr, &f.private_names, limit)
        } else {
            on.run_for(
                "sh",
                &crate::heavy::Heavy::check(heavy.as_ref(), exe, &c, wt),
                "",
                limit,
            )
        };
        let o = match o {
            Ok(o) if o.code == 0 => continue,
            Ok(o) => o,
            Err(e) => return refuse(CODE, format!("`{c}` cannot start: {e:#}")),
        };
        let what = match o.code {
            124 => format!("timed out after {} min", cfg.limits.gate_check_minutes),
            126 | 127 => "cannot start".into(),
            x => format!("failed (exit {x})"),
        };
        let all = crate::lanes::mask(&format!("{}{}", o.stdout, o.stderr));
        let lines: Vec<&str> = all.lines().filter(|l| !l.trim().is_empty()).collect();
        let tail = lines.get(lines.len().saturating_sub(10)..).unwrap_or_default();
        return refuse(CODE, format!("`{c}` {what}:\n{}", tail.join("\n")));
    }
    Ok(Some((head, None)))
}

fn checks(f: &Fleet, on: &On, wt: &Path) -> Vec<String> {
    if let Some(c) = &f.done_checks {
        return c.clone();
    }
    let (gk, arch, cargo) = (
        crate::gatekeeper::wrapper(wt),
        wt.join(".drover/arch.toml"),
        wt.join("Cargo.toml"),
    );
    let (gk, arch, cargo) = (gk.to_string_lossy(), arch.to_string_lossy(), cargo.to_string_lossy());
    let there = on.existing(&[&gk, &arch, &cargo]);
    if there.contains(gk.as_ref()) {
        return vec![crate::gatekeeper::CHECK.into()];
    }
    [(&arch, "drover arch-check"), (&cargo, "cargo fmt --check")]
        .into_iter()
        .filter(|(p, _)| there.contains(p.as_ref()))
        .map(|(_, c)| c.to_string())
        .collect()
}

/// `body` has `## For humans`, no "Generated with" line, and names `head` as `scripts/evidence-check.sh` does.
pub fn body(
    body: &str,
    head: &str,
    base: &str,
    subject: &dyn Fn(&str) -> Option<String>,
    parent: &dyn Fn(&str) -> Option<String>,
) -> Result<(), String> {
    let lines = || body.lines().map(str::trim);
    if !lines().any(|l| l.eq_ignore_ascii_case("## For humans")) {
        return Err("the PR body has no `## For humans` section: add 2-4 plain lines, `gh pr edit --body`".into());
    }
    let generated = |l: &str| {
        l.trim_start_matches(|c: char| !c.is_ascii_alphanumeric())
            .starts_with("Generated with")
    };
    if let Some(l) = lines().find(|l| generated(l)) {
        return Err(format!("the PR body carries `{l}`: drop it, `gh pr edit --body`"));
    }
    let low = body.to_ascii_lowercase();
    let names = |c: &str| {
        let c = c.get(..7).unwrap_or(c).to_ascii_lowercase();
        (low.match_indices(&c)).any(|(i, _)| !low[..i].ends_with(|ch: char| ch.is_ascii_hexdigit()))
    };
    let merge = |s: &str| {
        let s = s
            .strip_prefix("Merge ")
            .map(|s| s.strip_prefix("remote-tracking ").unwrap_or(s));
        let s = s.map(|s| s.strip_prefix("branch ").unwrap_or(s));
        let s = s.map(|s| s.strip_prefix('\'').unwrap_or(s));
        let s = s.and_then(|s| s.strip_prefix("origin/").unwrap_or(s).strip_prefix(base));
        s.is_some_and(|t| matches!(t.strip_prefix('\'').unwrap_or(t).chars().next(), None | Some(' ')))
    };
    let mut c = head.to_string();
    while !names(&c) {
        match subject(&c).filter(|s| merge(s)).and_then(|_| parent(&c)) {
            Some(p) => c = p,
            None => {
                return Err(format!(
                    "the PR body's evidence names no sha of head {head:.7}: re-run the gates at the head and `gh pr edit --body` with it"
                ))
            }
        }
    }
    Ok(())
}

/// `drover pr-body`: `body` on `file` against `head`, the merge walk read from git in the working directory; exit 1 with the reason.
pub fn pr_body(r: &dyn Runner, file: &Path, head: &str, base: &str, out: &mut dyn Write) -> Result<i32> {
    if head.len() < 7 || !head.bytes().all(|b| b.is_ascii_hexdigit()) {
        anyhow::bail!("--head {head:?} is no commit sha: give at least 7 hex digits");
    }
    let text = std::fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
    let git = |a: &[&str]| crate::runner::ok(r, "git", a).ok().map(|s| s.trim().to_string());
    let subject = |c: &str| git(&["log", "-1", "--format=%s", c]);
    let parent = |c: &str| git(&["rev-parse", &format!("{c}^1")]);
    match body(&text, head, base, &subject, &parent) {
        Ok(()) => Ok(0),
        Err(e) => {
            writeln!(out, "PR_BODY: {e}")?;
            Ok(1)
        }
    }
}

pub(crate) fn ungated(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, n): (&str, u64),
    ev: &[Event],
    (lane, head): (&str, &str),
    idle: bool,
) -> Result<bool> {
    let sp = crate::events::last_spawn(ev, lane).map_or(0, |e| e.ts);
    let at = |k: &'static str| move |e: &&Event| e.is(k, fleet, n) && e.str("sha") == Some(head) && e.ts >= sp;
    let refused: Vec<&Event> = ev.iter().filter(at("done-refused")).collect();
    let told = ev.iter().rfind(at("done-nudged"));
    let why = refused.last().map_or_else(
        || format!("head {head:.7} is pushed with no passing `drover turn done`"),
        |e| {
            format!(
                "turn done refused {}: {}",
                e.str("code").unwrap_or_default(),
                e.str("why").unwrap_or_default()
            )
        },
    );
    let quiet = told.map(|t| refused.last().map_or(t.ts, |e| e.ts.max(t.ts)));
    if refused.len() >= 3 || quiet.is_some_and(|q| ctx.now >= q + cfg.limits.stall_minutes * 60) {
        writeln!(out, "{fleet} #{n} needs-human: {why}")?;
        let d = json!({"code": "DONE_REFUSED", "reason": why, "agent": lane, "sha": head});
        emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "needs-human", d))?;
        return Ok(true);
    }
    if !idle || told.is_some() {
        return Ok(false);
    }
    let text = format!(
        "drover: {why}. No review starts until `drover turn done` passes at the pushed head: the PR body names it and has `## For humans`, the format and arch checks pass. Fix what it names, push, then `drover turn done` again."
    );
    let sent = crate::drive::tell(r, ev, lane, &text).unwrap_or(false);
    writeln!(out, "{fleet} #{n} no passing turn done at {head:.7}: told")?;
    let d = json!({"agent": lane, "sha": head, "sent": sent});
    append(
        &ctx.state,
        &Event::new(ctx.now, fleet, Some(n), "done-nudged", d).passive(),
    )?;
    Ok(false)
}
