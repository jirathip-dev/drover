use crate::access::{civil_month_day, civil_year};
use crate::config::Config;
use crate::events::{plain, Event};
use crate::runner::{ok, Runner};
use crate::Ctx;
use anyhow::{Context, Result};
use serde_json::json;
use std::io::Write;

/// `drover lesson digest`: the last `days` of rev FAIL findings, the owner's lines in the fleet's conductor threads, `by: owner` events and reverts on integration, secret-looking values redacted.
pub fn digest(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str, days: u64) -> Result<(String, usize)> {
    let f = cfg.fleet(fleet)?;
    let since = ctx.now.saturating_sub(days * 86400);
    let events = crate::events::history(&ctx.state)?;
    let recent: Vec<&Event> = events.iter().filter(|e| e.fleet == fleet && e.ts >= since).collect();
    let mut s =
        format!("# Lessons digest: {fleet}, last {days} days\n\n## Review FAIL findings (blocking, with proof)\n");
    let fails: Vec<&&Event> = (recent.iter())
        .filter(|e| e.kind == "verdict" && e.str("verdict") == Some("FAIL"))
        .collect();
    for e in &fails {
        let n = e.issue.unwrap_or_default();
        let round = events
            .iter()
            .filter(|v| v.is("verdict", fleet, n) && v.ts <= e.ts)
            .count();
        for (_, ls) in crate::gate::findings(e.str("body").unwrap_or_default())
            .into_iter()
            .filter(|f| f.0)
        {
            s += &format!("\n#{n} round {round}:\n{}\n", ls.join("\n"));
        }
    }
    s += "\n## The owner in orchestrator threads\n\n";
    let owner_msg = |e: &&&Event| {
        e.kind == "thread-msg"
            && e.str("from").is_some_and(|f| f != "orchestrator")
            && crate::orchestrator::link(e).is_none()
    };
    for e in recent.iter().filter(owner_msg) {
        let text = e.str("text").unwrap_or_default().replace('\n', "\n  ");
        s += &format!("- {}: {text}\n", e.str("thread").unwrap_or("?"));
    }
    s += "\n## By the owner (overrides, extends, parks, answers against the recommendation)\n\n";
    let asked = |e: &Event| {
        let ask = events
            .iter()
            .find(|a| a.kind == "needs-decision" && a.fleet == fleet && a.str("id") == e.str("id"));
        let rec = ask.and_then(|a| {
            a.val("options")
                .get(usize::try_from(a.val("recommend").as_u64()?.checked_sub(1)?).ok()?)
        });
        rec.and_then(|o| o.as_str()).is_some_and(|o| Some(o) != e.str("choice"))
    };
    let owner = |e: &&&Event| match e.kind.as_str() {
        "decision" => e.str("by") != Some("default") && asked(e),
        k => e.str("by") == Some("owner") && !k.starts_with("secret"),
    };
    for e in recent.iter().filter(owner) {
        s += &format!("- {}: {}\n", e.kind, plain(e));
    }
    s += "\n## Reverts on integration\n\n";
    let local = f.local_path().to_string_lossy().into_owned();
    let integration = format!("origin/{}", f.integration);
    let log = ["log", &integration, "-i", "--grep=^revert", "--format=%ct %h %s"];
    for l in ok(r, "git", &[&["-C", local.as_str()][..], &log].concat())
        .unwrap_or_default()
        .lines()
    {
        if let Some((_, c)) = l
            .split_once(' ')
            .filter(|(t, _)| t.parse::<u64>().is_ok_and(|t| t >= since))
        {
            s += &format!("- {c}\n");
        }
    }
    let show = |p: &str| ok(r, "git", &["-C", local.as_str(), "show", &format!("{integration}:{p}")]).ok();
    let rules = crate::repo_read("rules.md", show).unwrap_or_default();
    let checks = review(ctx, cfg, r, fleet, &rules)?;
    if !checks.is_empty() {
        s += "\n## Lesson checks (`drover lesson apply` promotes and retires them)\n\n";
    }
    for (c, stats, decision) in checks {
        s += &format!("- {}: {stats} → {decision}\n", c.name);
    }
    let text = s.lines().map(crate::lanes::redact).collect::<Vec<_>>().join("\n") + "\n";
    Ok((text, fails.len()))
}

const RULES: [&str; 2] = [".drover/rules.md", ".herdsman/rules.md"];
pub const PROSE_ONLY: &str = "- blocking [lesson] .drover/rules.md:1: this lesson PR adds or changes no enforcer \
(a CI step, a test, a type, a lint rule, a lesson check) and retires no rules line\n  Proof: spec\n  > A PR that only \
edits prose is a FAIL in rev, unless it retires lines.\n  roles/lesson.md:3 says each class is fixed by a check";

/// Whether issue `n` is a lessons issue: its lane's build, else the job that filed it.
pub fn is_lesson(ctx: &Ctx, st: Option<&crate::events::IssueState>, (fleet, n): (&str, u64)) -> bool {
    st.is_some_and(|s| s.build.as_deref() == Some("lesson"))
        || crate::events::working(&ctx.state).is_ok_and(|e| crate::jobs::of(&e, fleet, n) == Some("lesson"))
}

/// Whether a PR's unified `diff` only edits prose: every file Markdown, no `.drover/rules.md` line removed or changed (a retirement or promotion), no lesson check added.
pub fn prose_only(diff: &str) -> bool {
    let (mut file, mut added) = ("", String::new());
    for l in diff.lines() {
        if let Some(p) = l.strip_prefix("diff --git a/") {
            file = p.split(" b/").next().unwrap_or(p);
            if !std::path::Path::new(file)
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("md"))
            {
                return false;
            }
        } else if RULES.contains(&file) && l.starts_with('-') && !l.starts_with("---") {
            return false;
        } else if let Some(a) = l
            .strip_prefix('+')
            .filter(|_| RULES.contains(&file) && !l.starts_with("+++"))
        {
            added += &format!("{a}\n");
        }
    }
    checks(&added).is_empty()
}

/// A check a lesson PR added: a `.drover/rules.md` line `- don't X (enforced by: <name> `<cmd>`, shadow since <date>)` (or `active since`). A line without that mark is hand-written: the lesson role never runs or changes it.
pub struct LessonCheck {
    pub line: usize,
    pub name: String,
    pub cmd: String,
    pub active: bool,
    pub since: String,
}

pub fn checks(rules: &str) -> Vec<LessonCheck> {
    let parse = |(line, l): (usize, &str)| {
        let (active, rest) = (l.split_once("shadow since ").map(|x| (false, x.1)))
            .or_else(|| l.split_once("active since ").map(|x| (true, x.1)))?;
        let name = l.split_once("enforced by: ")?.1.split([' ', '`', ',', ')']).next()?;
        let cmd = l.split('`').nth(1).filter(|c| !c.is_empty())?;
        let since = rest.get(..10)?.into();
        (!name.is_empty()).then(|| LessonCheck {
            line,
            name: name.into(),
            cmd: cmd.into(),
            active,
            since,
        })
    };
    rules.lines().enumerate().filter_map(parse).collect()
}

fn failing(r: &dyn Runner, dir: &std::path::Path, cs: Vec<LessonCheck>) -> Vec<(LessonCheck, String)> {
    let run = |c: LessonCheck| {
        let sh = crate::runner::argv(&["-c", &format!("cd \"$0\" && {}", c.cmd), &dir.to_string_lossy()]);
        let o = r.run("sh", &sh).unwrap_or_default();
        (o.code != 0).then(|| (c, format!("{}{}", o.stdout, o.stderr)))
    };
    cs.into_iter().filter_map(run).collect()
}

fn malformed(rules: &str) -> Vec<String> {
    let unenforced = |l: &str| {
        let named = l.split_once("(enforced by: ").map(|x| x.1.trim_start());
        !named.is_some_and(|n| !n.is_empty() && !n.starts_with(')'))
    };
    let lines: Vec<&str> = rules.lines().collect();
    let mut bad: Vec<String> = (lines.iter().enumerate())
        .filter(|(_, l)| l.starts_with("- ") && unenforced(l))
        .map(|(i, _)| {
            format!(
                "line {} names no enforcer: end it `(enforced by: <check>)` or `(enforced by: none)`",
                i + 1
            )
        })
        .collect();
    if lines.len() > 30 {
        bad.push(format!(
            "{} lines, over 30: delete the rules a check now enforces",
            lines.len()
        ));
    }
    bad
}

/// `drover lesson-check` (CI, at the repo root): a shadow check that fails is reported as would-fail (stdout and `$GITHUB_STEP_SUMMARY`) and never fails the build; an active one that fails exits 1, as does a rule line naming no enforcer or a file over 30 lines.
pub fn ci(r: &dyn Runner, rules: &std::path::Path, out: &mut dyn Write) -> Result<i32> {
    let text = std::fs::read_to_string(rules).unwrap_or_default();
    let bad = malformed(&text);
    let (mut code, mut s) = (i32::from(!bad.is_empty()), String::new());
    for b in bad {
        s += &format!("FAIL {}: {b}\n", rules.display());
    }
    for (c, o) in failing(r, std::path::Path::new("."), checks(&text)) {
        let what = if c.active { "FAIL" } else { "would-fail (shadow)" };
        code |= i32::from(c.active);
        s += &format!("{what} {}: `{}`\n```\n{}\n```\n", c.name, c.cmd, o.trim_end());
    }
    write!(out, "{s}")?;
    if let Some(p) = crate::config::env_os("GITHUB_STEP_SUMMARY").filter(|_| !s.is_empty()) {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p)?;
        write!(f, "## Lesson checks (#510)\n\n{s}")?;
    }
    Ok(code)
}

/// At a merge's premerge (`wt`: the PR merged with integration) each shadow check runs; one `lesson-shadow` event (the checks run, those that would fail) is the history the next lesson run promotes or retires by.
pub fn record(ctx: &Ctx, r: &dyn Runner, wt: &std::path::Path, (fleet, n): (&str, u64)) -> Result<()> {
    let text = std::fs::read_to_string(crate::repo_path(&wt.join(".drover/rules.md"))).unwrap_or_default();
    let shadow: Vec<LessonCheck> = checks(&text).into_iter().filter(|c| !c.active).collect();
    if shadow.is_empty() {
        return Ok(());
    }
    let ran: Vec<String> = shadow.iter().map(|c| c.name.clone()).collect();
    let hits: Vec<String> = failing(r, wt, shadow).into_iter().map(|c| c.0.name).collect();
    let d = json!({"ran": ran, "hits": hits});
    crate::events::append(&ctx.state, &Event::new(ctx.now, fleet, Some(n), "lesson-shadow", d))
}

/// Day number `day` as `YYYY-MM-DD`.
pub fn ymd(day: u64) -> String {
    let (m, d) = civil_month_day(day);
    format!("{}-{m:02}-{d:02}", civil_year(day))
}

/// Each lesson check in `rules` with its history and what the next lesson run does with it. The owner's `promote <name>` / `retire <name>` line in a lesson PR's body comes first; else a shadow check with a false positive (a `false positive: <name>` line from rev or the owner) is fixed or removed; one that caught a real instance over at least 7 days or 10 PRs is promoted; one that ran and caught nothing in 30 days is retired.
pub fn review(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    fleet: &str,
    rules: &str,
) -> Result<Vec<(LessonCheck, String, &'static str)>> {
    let cs = checks(rules);
    if cs.is_empty() {
        return Ok(vec![]);
    }
    let pr = [
        "pr",
        "list",
        "-R",
        &cfg.fleet(fleet)?.repo,
        "--state",
        "all",
        "--search",
        "lessons: in:title",
    ];
    let bodies = ok(r, "gh", &[&pr[..], &["--json", "body", "--jq", ".[].body"]].concat()).unwrap_or_default();
    let read = "archived = 0 AND fleet = ?1 AND type IN ('lesson-shadow', 'verdict', 'thread-msg')";
    let events = crate::db::events(&ctx.state, read, fleet)?;
    let today = ctx.now / 86400;
    let decide = |c: LessonCheck| {
        let mine = |e: &&Event| e.fleet == fleet && ymd(e.ts / 86400) >= c.since;
        let shadow = |e: &&Event| {
            e.kind == "lesson-shadow" && e.val("ran").as_array().is_some_and(|a| a.contains(&json!(c.name)))
        };
        let prs: Vec<&Event> = events.iter().filter(mine).filter(shadow).collect();
        let caught = prs
            .iter()
            .filter(|e| e.val("hits").as_array().is_some_and(|a| a.contains(&json!(c.name))))
            .count();
        let fp = format!("false positive: {}", c.name);
        let said = |e: &Event| {
            match e.kind.as_str() {
                "verdict" => e.str("body"),
                "thread-msg" if e.str("from") != Some("orchestrator") => e.str("text"),
                _ => None,
            }
            .is_some_and(|t| t.lines().any(|l| l.trim() == fp))
        };
        let fps = events.iter().filter(mine).filter(|e| said(e)).count();
        let owner = bodies.lines().map(str::trim).find_map(|l| match l.split_once(' ') {
            Some((w @ ("promote" | "retire"), n)) if n == c.name => Some(w),
            _ => None,
        });
        let mark = if c.active { "active" } else { "shadow" };
        let stats = format!(
            "{mark} since {}, {} PRs, {caught} caught, {fps} false positives",
            c.since,
            prs.len()
        );
        let decision = match owner {
            Some("retire") => "retire",
            _ if c.active => "keep active",
            Some(_) => "promote",
            _ if fps > 0 => "fix or remove",
            _ if caught > 0 && (c.since <= ymd(today.saturating_sub(7)) || prs.len() >= 10) => "promote",
            _ if caught == 0 && !prs.is_empty() && c.since <= ymd(today.saturating_sub(30)) => "retire",
            _ => "keep shadow",
        };
        (c, stats, decision)
    };
    Ok(cs.into_iter().map(decide).collect())
}

/// `drover lesson apply`: the lesson run rewrites `rules` (its worktree's `.drover/rules.md`) by `review`: a promoted check becomes `active since <today>`, a retired one is deleted (the commit is its history); fix-or-remove ones are listed for the lane. Hand-written lines stay byte for byte.
pub fn apply(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    fleet: &str,
    rules: &std::path::Path,
) -> Result<()> {
    let text = std::fs::read_to_string(rules).with_context(|| format!("read {}", rules.display()))?;
    let mut lines: Vec<Option<String>> = text.lines().map(|l| Some(l.to_string())).collect();
    for (c, stats, decision) in review(ctx, cfg, r, fleet, &text)? {
        writeln!(out, "{}: {stats} → {decision}", c.name)?;
        let to = format!("active since {}", ymd(ctx.now / 86400));
        let Some(l) = lines.get_mut(c.line) else { continue };
        match decision {
            "promote" => *l = l.take().map(|s| s.replace(&format!("shadow since {}", c.since), &to)),
            "retire" => *l = None,
            _ => {}
        }
    }
    let kept: Vec<String> = lines.into_iter().flatten().collect();
    Ok(std::fs::write(
        rules,
        kept.iter().map(|l| format!("{l}\n")).collect::<String>(),
    )?)
}
