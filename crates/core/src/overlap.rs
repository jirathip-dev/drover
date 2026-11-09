//! #129: fewer merge conflicts, with git alone: `git merge-tree` previews which open branches collide, a merge of
//! integration that adds nothing keeps a PASS, and `cap-check` reads the line cap from config.
use crate::config::{Config, Fleet};
use crate::events::{emit, fold, Event, IssueState};
use crate::runner::{argv, ok, Runner};
use crate::{gh, Ctx};
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Per issue: (the other issue, 0 for integration, the files both change).
pub type Conflicts = BTreeMap<u64, Vec<(u64, Vec<String>)>>;

pub fn git(r: &dyn Runner, dir: &Path, args: &[&str]) -> Result<String> {
    let d = dir.to_string_lossy();
    ok(r, "git", &[&["-C", &*d], args].concat()).map(|o| o.trim().to_string())
}

/// The last preview's conflicts; never runs git.
pub fn cached(ctx: &Ctx, fleet: &str) -> Conflicts {
    let v = crate::usage::load::<Value>(&gh::cache_path(ctx, "conflicts", fleet, 0)).unwrap_or_default();
    serde_json::from_value(v["conflicts"].clone()).unwrap_or_default()
}

/// The files `a` and `b` conflict on (`git merge-tree` exits 1, no worktree); empty when they merge cleanly.
fn collide(r: &dyn Runner, dir: &Path, a: &str, b: &str) -> Vec<String> {
    let d = dir.to_string_lossy();
    let o = r.run(
        "git",
        &argv(&["-C", &d, "merge-tree", "--write-tree", "--name-only", a, b]),
    );
    let o = o.ok().filter(|o| o.code == 1).unwrap_or_default();
    o.stdout
        .lines()
        .skip(1)
        .take_while(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// #320: a predicted conflict is information, not a hold.
fn keep_going(o: u64) -> String {
    format!("may conflict with #{o}; keep going, herdsman updates you after it merges")
}

/// Every pair of pushed, unmerged branches and each against integration, cached by head pair; logs one
/// `conflict-predicted` per new pair and prompts an impl/fix lane once per head about a ready PR or integration.
pub fn preview(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str, events: &[Event]) -> Result<Conflicts> {
    let (f, p) = (cfg.fleet(fleet)?, gh::cache_path(ctx, "conflicts", fleet, 0));
    let dir = f.local_path();
    let rev = |b: &str| git(r, &dir, &["rev-parse", &format!("origin/{b}")]);
    let ev = |n, kind, d| emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), kind, d));
    git(r, &dir, &["fetch", "-q", "origin"])?;
    let st: Vec<IssueState> = fold(events)
        .into_values()
        .filter(|s| s.fleet == fleet && !s.done())
        .collect();
    let pushed = st
        .iter()
        .filter_map(|s| Some((s.issue, rev(&crate::gate::branch_of(Some(s), s.issue)).ok()?)));
    let heads: Vec<(u64, String)> = std::iter::once((0, rev(&f.integration)?)).chain(pushed).collect();
    let old = crate::usage::load::<Value>(&p).unwrap_or_default();
    let (mut cache, mut out) = (BTreeMap::new(), Conflicts::new());
    for (i, (a, ha)) in heads.iter().enumerate() {
        for (b, hb) in &heads[i + 1..] {
            let key = format!("{ha} {hb}");
            let files: Vec<String> =
                serde_json::from_value(old["cache"][&key].clone()).unwrap_or_else(|_| collide(r, &dir, ha, hb));
            if !files.is_empty() && old["cache"][&key].is_null() {
                ev(
                    *b,
                    "conflict-predicted",
                    json!({"other": a, "files": files, "heads": key, "reason": (*a > 0).then(|| keep_going(*a))}),
                )?;
            }
            for (x, y) in [(b, a), (a, b)]
                .into_iter()
                .filter(|(x, _)| **x > 0 && !files.is_empty())
            {
                out.entry(*x).or_default().push((*y, files.clone()));
            }
            cache.insert(key, files);
        }
    }
    gh::store(&p, &json!({"cache": cache, "conflicts": out}))?;
    for s in st.iter().filter(|s| s.state == "impl" || s.state.starts_with("fix")) {
        let ready = |o: &u64| *o == 0 || st.iter().any(|x| x.issue == *o && x.state == "ready");
        let (Some(lane), Some((o, files))) = (&s.lane, out.get(&s.issue).and_then(|c| c.iter().find(|c| ready(&c.0))))
        else {
            continue;
        };
        let key = format!("{} {o}", heads.iter().find(|h| h.0 == s.issue).map_or("", |h| &h.1));
        if events
            .iter()
            .any(|e| e.is("heads-up", fleet, s.issue) && e.str("key") == Some(&key))
        {
            continue;
        }
        let (into, files) = (format!("origin/{}", f.integration), files.join(", "));
        let text = if *o == 0 {
            format!("HEADS-UP: your branch conflicts with {into} on {files}. Merge {into} now.")
        } else {
            format!(
                "HEADS-UP: #{o} is ready to merge and touches {files}: your branch {}.",
                keep_going(*o)
            )
        };
        let sp = crate::events::last_spawn(events, lane);
        let pane = sp.and_then(|e| e.str("pane")).unwrap_or_default();
        let on = crate::hosts::On::of(r, sp);
        if crate::herdr::reply(&on, sp.and_then(|e| e.str("harness")) == Some("dsh"), lane, pane, &text).is_ok() {
            ev(s.issue, "heads-up", json!({"key": key, "reason": text}))?;
        }
    }
    Ok(out)
}

/// #129: `h` merges a passed head (or an earlier pure update of one) with integration and adds nothing: its first parent
/// carries the PASS, its second is on integration, and its tree is the one `git merge-tree` makes of the two.
pub fn pure(r: &dyn Runner, f: &Fleet, s: &IssueState, h: &str) -> bool {
    let dir = f.local_path();
    git(r, &dir, &["fetch", "-q", "origin"]).is_ok() && pure_at(r, &dir, &format!("origin/{}", f.integration), s, h)
}

/// #207: `h` is a pure update of `x`, so a verdict at `x` (whatever it says) carries to `h`.
pub fn carries(r: &dyn Runner, f: &Fleet, s: &IssueState, x: &str, h: &str) -> bool {
    let mut s = s.clone();
    s.verdicts.push((x.into(), "PASS".into(), String::new()));
    pure(r, f, &s, h)
}

fn pure_at(r: &dyn Runner, dir: &Path, into: &str, s: &IssueState, h: &str) -> bool {
    let Ok(p) = git(r, dir, &["rev-list", "--parents", "-n1", h]) else {
        return false;
    };
    let [_, a, b] = p.split(' ').collect::<Vec<_>>()[..] else {
        return false;
    };
    let pass = s
        .verdicts
        .iter()
        .rfind(|v| v.0.eq_ignore_ascii_case(a))
        .is_some_and(|v| v.1 == "PASS");
    (pass || pure_at(r, dir, into, s, a))
        && git(r, dir, &["merge-base", "--is-ancestor", b, into]).is_ok()
        && git(r, dir, &["merge-tree", "--write-tree", a, b]).ok()
            == git(r, dir, &["rev-parse", &format!("{h}^{{tree}}")]).ok()
}

/// `herdsman cap-check`: formatted lines of `src/**/*.rs` and `crates/*/src/**/*.rs` (#264) against the first `[fleet.<n>] line_cap` in `config`;
/// #429: under `warn_within` lines of headroom it warns and still passes. #270: `web/src/**/*.ts` lines (#720: and
/// `web/plugin/src`'s) are reported, never capped.
pub fn cap_check(config: &Path, rules: &Path, out: &mut dyn std::io::Write) -> Result<i32> {
    let (cap, warn) = limits(&std::fs::read_to_string(config)?)
        .ok_or_else(|| anyhow::anyhow!("no [fleet.<name>] line_cap in {}", config.display()))?;
    let crates = std::fs::read_dir("crates").into_iter().flatten().flatten();
    let n = lines(Path::new("src"), "rs") + crates.map(|c| lines(&c.path().join("src"), "rs")).sum::<usize>();
    writeln!(out, "src lines (formatted): {n} / cap {cap}")?;
    writeln!(
        out,
        "web lines (TypeScript, soft budget, not capped): {}",
        lines(Path::new("web/src"), "ts") + lines(Path::new("web/plugin/src"), "ts")
    )?;
    let left = cap - i64::try_from(n)?;
    if (0..warn).contains(&left) {
        writeln!(out, "WARN: {left} lines of headroom (< {warn})")?;
    }
    // #419: a repo's `.herdsman/rules.md` holds at most 30 rules; one that code enforces is deleted.
    let over = std::fs::read_to_string(rules).map_or(0, |t| t.lines().count());
    if over > 30 {
        writeln!(
            out,
            "{}: {over} lines, over 30: delete the rules a check now enforces",
            rules.display()
        )?;
    }
    Ok(i32::from(left < 0 || over > 30))
}

/// The first `[fleet.<n>]` with a `line_cap`: (cap, `warn_within`, default 500).
fn limits(text: &str) -> Option<(i64, i64)> {
    let v: toml::Value = toml::from_str(text).ok()?;
    let t = v
        .get("fleet")?
        .as_table()?
        .values()
        .find(|t| t.get("line_cap").is_some())?;
    let warn = t.get("warn_within").and_then(toml::Value::as_integer).unwrap_or(500);
    Some((t["line_cap"].as_integer()?, warn))
}

/// #429: `(sha, lines, cap, warn_within)` at the fleet's fetched integration head with a `ci/cap.toml`, by git alone;
/// cached per sha (`serve`'s banner reads the cache).
pub fn cap_at(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str) -> Option<(String, i64, i64, i64)> {
    let (f, p) = (cfg.fleet(fleet).ok()?, gh::cache_path(ctx, "cap", fleet, 0));
    let dir = f.local_path();
    let sha = git(r, &dir, &["rev-parse", &format!("origin/{}", f.integration)]).ok()?;
    let old = crate::usage::load::<Value>(&p).unwrap_or_default();
    let n = |k: &str| old[k].as_i64();
    if old["sha"] == sha.as_str() {
        return Some((sha, n("lines")?, n("cap")?, n("warn")?));
    }
    let (cap, warn) = limits(&git(r, &dir, &["show", &format!("{sha}:ci/cap.toml")]).ok()?)?;
    let globs = [":(glob)src/**/*.rs", ":(glob)crates/*/src/**/*.rs"];
    let counts = git(r, &dir, &[&["grep", "-c", "", &sha, "--"][..], &globs].concat()).unwrap_or_default();
    let lines = counts
        .lines()
        .filter_map(|l| l.rsplit(':').next()?.parse::<i64>().ok())
        .sum();
    let _ = gh::store(&p, &json!({"sha": sha, "lines": lines, "cap": cap, "warn": warn}));
    Some((sha, lines, cap, warn))
}

fn lines(p: &Path, ext: &str) -> usize {
    let each = std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.path());
    let n = |p: PathBuf| match p.extension() {
        _ if p.is_dir() => lines(&p, ext),
        Some(x) if x == ext => std::fs::read_to_string(&p).map_or(0, |t| t.lines().count()),
        _ => 0,
    };
    each.map(n).sum()
}

/// #668: the `src/ net +N` a PR body states (`+1,234`, `-20`), if any.
fn stated_net(body: &str) -> Option<i64> {
    let rest = body.split("src/ net ").nth(1)?.trim_start();
    let n: String = rest.chars().take_while(|c| "+-,0123456789".contains(*c)).collect();
    n.replace([',', '+'], "").parse().ok()
}

/// #668: `herdsman cap <fleet>`: the queue-wide line-cap view, so one conductor-filed issue raises the cap for the
/// whole queue: integration's total against the cap, every open PR's `src/ net` (its body's, else its measured diff),
/// the queued issues with no PR, and the projected total with the ask a crossing lane files.
pub fn cap_view(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str, out: &mut dyn std::io::Write) -> Result<i32> {
    let f = cfg.fleet(fleet)?;
    let dir = f.local_path();
    let _ = git(r, &dir, &["fetch", "-q", "origin"]);
    let (sha, lines, cap, warn) = cap_at(ctx, cfg, r, fleet).ok_or_else(|| {
        anyhow::anyhow!(
            "no ci/cap.toml line_cap at origin/{} in {}",
            f.integration,
            dir.display()
        )
    })?;
    writeln!(
        out,
        "{fleet}: {lines} / cap {cap} at {} {sha:.7}, headroom {}",
        f.integration,
        cap - lines
    )?;
    let prs = gh::snapshot(ctx, r, f, fleet, ctx.now)?;
    let mut prs: Vec<&Value> = (prs.as_object().into_iter().flat_map(|m| m.values()))
        .filter(|p| p["baseRefName"] == f.integration.as_str())
        .collect();
    prs.sort_by_key(|p| p["number"].as_u64());
    let globs = [":(glob)src/**/*.rs", ":(glob)crates/*/src/**/*.rs"];
    let measured = |b: &str| -> Option<i64> {
        let range = format!("{sha}...origin/{b}");
        let o = git(r, &dir, &[&["diff", "--numstat", &range, "--"][..], &globs].concat()).ok()?;
        let n = |w: Option<&str>| w.and_then(|w| w.parse::<i64>().ok()).unwrap_or(0);
        Some(
            o.lines()
                .map(|l| l.split('\t'))
                .map(|mut w| n(w.next()) - n(w.next()))
                .sum(),
        )
    };
    let (mut sum, mut has) = (0, std::collections::BTreeSet::new());
    writeln!(out, "open PRs (src/ net):")?;
    for p in prs {
        let b = p["headRefName"].as_str().unwrap_or_default();
        let issue = b.strip_prefix("issue-").and_then(|n| n.parse::<u64>().ok());
        has.extend(issue);
        let who = issue.map_or_else(|| b.to_string(), |n| format!("#{n}"));
        let net = stated_net(p["body"].as_str().unwrap_or_default())
            .map(|n| (n, "body"))
            .or_else(|| measured(b).map(|n| (n, "measured")));
        let num = p["number"].as_u64().unwrap_or(0);
        match net {
            Some((n, how)) => {
                sum += n;
                writeln!(out, "  {who} (PR #{num}): {n:+} ({how})")?;
            }
            None => writeln!(
                out,
                "  {who} (PR #{num}): unknown (no `src/ net` in its body, branch not fetched)"
            )?,
        }
    }
    let (events, st) = (crate::events::read(&ctx.state)?, crate::events::states(&ctx.state)?);
    let queued: Vec<u64> = (crate::queue::view(&events, &st, fleet).into_iter().map(|i| i.issue))
        .filter(|n| !has.contains(n))
        .collect();
    let none = queued.is_empty().then_some("none");
    writeln!(
        out,
        "queued, no PR: {}",
        none.map_or_else(|| crate::queue::hashes(&queued), String::from)
    )?;
    let projected = lines + sum;
    writeln!(
        out,
        "sum of open PRs: {sum:+}; projected total {projected} / cap {cap}, headroom {}",
        cap - projected
    )?;
    writeln!(
        out,
        "ask options: 1. raise to {} in one conductor-filed issue (recommended) 2. trim",
        projected + warn
    )?;
    Ok(0)
}
