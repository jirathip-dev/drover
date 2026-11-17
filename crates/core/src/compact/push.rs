use super::{map, moves, origin, Plan};
use crate::config::Fleet;
use crate::events::{Event, Json};
use crate::gate::refused;
use crate::runner::{argv, json, ok, Runner};
use crate::Ctx;
use anyhow::{bail, Result};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn remote(r: &dyn Runner, url: &str) -> Result<BTreeMap<String, String>> {
    let out = ok(r, "git", &["ls-remote", url])?;
    let row = |l: &str| l.split_once('\t').map(|(v, n)| (n.to_string(), v.to_string()));
    Ok(out.lines().filter_map(row).collect())
}

fn push_all(r: &dyn Runner, copy: &Path, url: &str, moves: &[(&str, &str, Option<&str>)]) -> Result<()> {
    let mut a = argv(&["-C", &copy.to_string_lossy(), "push", "--atomic", "--porcelain", url]);
    for (name, lease, _) in moves {
        a.push(format!("--force-with-lease={name}:{lease}"));
    }
    a.extend(
        moves
            .iter()
            .map(|(name, _, to)| format!("{}:{name}", to.unwrap_or_default())),
    );
    let o = r.run("git", &a)?;
    if o.code != 0 {
        bail!("the atomic push was refused, nothing moved: {}", o.stderr.trim());
    }
    Ok(())
}

/// Every plan ref of one card (`main`: main alone, else all but main) on origin at its old value (`undo`: its new one), else `COMPACT_MOVED`; the refs still to move.
pub fn at(r: &dyn Runner, f: &Fleet, p: &Plan, undo: bool, main: bool) -> Result<usize> {
    let now = remote(r, &origin(r, f)?)?;
    let (mut todo, mut moved) = (0, vec![]);
    for m in p.refs.iter().filter(|m| p.is_main(m) == main) {
        let (from, to) = if undo {
            (m.new.as_deref(), Some(m.old.as_str()))
        } else {
            (Some(m.old.as_str()), m.new.as_deref())
        };
        let is = now.get(&m.name).map(String::as_str);
        if is == from && from != to {
            todo += 1;
        } else if is != to {
            moved.push(m.name.as_str());
        }
    }
    if !moved.is_empty() {
        return refused("COMPACT_MOVED", format!("moved since the plan: {}", moved.join(", ")));
    }
    Ok(todo)
}

fn archive(r: &dyn Runner, f: &Fleet, d: &Path, p: &Plan, now: u64) -> Result<()> {
    let view = || json(r, "gh", &["repo", "view", &p.archive, "--json", "visibility"]);
    let v = if let Ok(v) = view() {
        v
    } else {
        ok(r, "gh", &["repo", "create", &p.archive, "--private"])?;
        view()?
    };
    if v.at("visibility") != "PRIVATE" {
        return refused(
            "COMPACT_ARCHIVE",
            format!("{} is not private; nothing was pushed", p.archive),
        );
    }
    let (url, copy) = (origin(r, f)?.replace(&f.repo, &p.archive), d.join("old.git"));
    let c = copy.to_string_lossy();
    let specs = [
        "+refs/heads/*:refs/heads/*",
        "+refs/tags/*:refs/tags/*",
        "+refs/notes/*:refs/notes/*",
        "+refs/pull/*/head:refs/heads/pull/*",
    ];
    ok(r, "git", &[&["-C", &*c, "push", "-q", &url][..], &specs].concat())?;
    let (old, there) = (
        ok(
            r,
            "git",
            &["-C", &c, "for-each-ref", "--format=%(refname)\t%(objectname)"],
        )?,
        remote(r, &url)?,
    );
    let as_there = |n: &str| match n.strip_prefix("refs/pull/").and_then(|x| x.strip_suffix("/head")) {
        Some(pr) => Some(format!("refs/heads/pull/{pr}")),
        None => (!n.starts_with("refs/pull/")).then(|| n.to_string()),
    };
    let missing: Vec<&str> = (old.lines().filter_map(|l| l.split_once('\t')))
        .filter(|(n, v)| as_there(n).is_some_and(|t| there.get(&t) != Some(&(*v).to_string())))
        .map(|(n, _)| n)
        .collect();
    if !missing.is_empty() {
        return refused(
            "COMPACT_ARCHIVE",
            format!("the archive's read-back misses {}", missing.join(", ")),
        );
    }
    let g = |a: &[&str], input: &str| -> Result<String> {
        let o = r.run_in("git", &argv(&[&["-C", &*c][..], a].concat()), input)?;
        Ok(o.stdout.trim().to_string())
    };
    let blob = |file: &str| -> Result<String> {
        g(
            &["hash-object", "-w", "--stdin"],
            &std::fs::read_to_string(d.join(file))?,
        )
    };
    let tree = format!(
        "100644 blob {}\tcompact-map.tsv\n100644 blob {}\tplan.json\n",
        blob("compact-map.tsv")?,
        blob("plan.json")?
    );
    let who = format!("drover <drover@localhost> {now} +0000");
    let raw = format!(
        "tree {}\nauthor {who}\ncommitter {who}\n\ncompact {}\n",
        g(&["mktree"], &tree)?,
        p.new_tip
    );
    let id = g(&["hash-object", "-t", "commit", "-w", "--stdin"], &raw)?;
    ok(
        r,
        "git",
        &[
            "-C",
            &c,
            "push",
            "-q",
            "-f",
            &url,
            &format!("{id}:refs/heads/drover-compact"),
        ],
    )
    .map(drop)
}

fn records(ctx: &Ctx, events: &[Event], names: &[&String], d: &Path, p: &Plan) -> Result<()> {
    let run = json!({"run": p.new_tip, "repo": p.repo, "line": p.line, "cut": p.cut, "root": p.root,
        "old_tip": p.old_tip, "archive": p.archive, "commits": p.commits, "refs": p.refs.len()});
    let mut evs: Vec<Event> = (names.iter())
        .map(|n| Event::new(ctx.now, n, None, "compacted", run.clone()).passive())
        .collect();
    for (fleet, i, moved) in moves(events, names, &map(d)?, &p.root) {
        let v = json!({"run": p.new_tip, "moved": moved});
        evs.push(Event::new(ctx.now, &fleet, Some(i), "compacted", v).passive());
    }
    crate::db::append_all(&ctx.state, &evs)
}

fn bodies(r: &dyn Runner, f: &Fleet, d: &Path, p: &Plan) -> Result<()> {
    let m = map(d)?;
    for pr in p.refs.iter().filter_map(|x| x.pr.filter(|_| x.new.is_some())) {
        let n = pr.to_string();
        let body = json(r, "gh", &["pr", "view", &n, "-R", &f.repo, "--json", "body"])?;
        let body = body.at("body").as_str().unwrap_or_default();
        let new = |w: &str| {
            let hex = w.len() >= 7 && w.len() <= 40 && w.bytes().all(|b| b.is_ascii_hexdigit());
            let (_, (to, kind)) = m
                .range(w.to_string()..)
                .next()
                .filter(|(o, _)| hex && o.starts_with(w))?;
            (kind == "same").then(|| to.get(..w.len()).unwrap_or(to).to_string())
        };
        let mut out = String::new();
        let mut word = String::new();
        for ch in body.chars().chain(['\u{0}']) {
            if ch.is_ascii_alphanumeric() {
                word.push(ch);
                continue;
            }
            out += &new(&word).unwrap_or(std::mem::take(&mut word));
            word.clear();
            out.extend(Some(ch).filter(|c| *c != '\u{0}'));
        }
        if out != body {
            ok(r, "gh", &["pr", "edit", &n, "-R", &f.repo, "--body", &out])?;
        }
    }
    Ok(())
}

/// A repo whose origin is at the plan's new values: its records logged if they were not; the line to print.
pub fn finished(
    ctx: &Ctx,
    r: &dyn Runner,
    f: &Fleet,
    d: &Path,
    events: &[Event],
    names: &[&String],
) -> Result<Option<String>> {
    let Ok(p) = super::load(d) else { return Ok(None) };
    if remote(r, &origin(r, f)?)?.get(&format!("refs/heads/{}", p.line)) != Some(&p.new_tip) {
        return Ok(None);
    }
    if !events
        .iter()
        .any(|e| e.kind == "compacted" && e.issue.is_none() && e.str("run") == Some(&p.new_tip))
    {
        records(ctx, events, names, d, &p)?;
    }
    Ok(Some(format!(
        "{} is compacted at {:.12} ({} → {} commits)",
        p.repo, p.new_tip, p.commits[0], p.commits[1]
    )))
}

/// The approved run: archive, one atomic push with a lease on every ref, the records, the open PR bodies.
pub fn push(
    ctx: &Ctx,
    r: &dyn Runner,
    f: &Fleet,
    d: &Path,
    p: &Plan,
    events: &[Event],
    names: &[&String],
) -> Result<()> {
    if at(r, f, p, false, false)? > 0 {
        archive(r, f, d, p, ctx.now)?;
        let mv: Vec<(&str, &str, Option<&str>)> = (p.refs.iter().filter(|m| !p.is_main(m)))
            .map(|m| (m.name.as_str(), m.old.as_str(), m.new.as_deref()))
            .collect();
        push_all(r, &d.join("old.git"), &origin(r, f)?, &mv)?;
    }
    finished(ctx, r, f, d, events, names)?;
    bodies(r, f, d, p)
}

fn copy(r: &dyn Runner, f: &Fleet, d: &Path, p: &Plan) -> Result<PathBuf> {
    let copy = d.join("old.git");
    if !copy.join("HEAD").exists() {
        super::write::mirror(r, &origin(r, f)?.replace(&f.repo, &p.archive), &copy)?;
    }
    Ok(copy)
}

/// The approved main card: main alone to its new value (`undo`: its old one), a lease on the value it leaves.
pub fn main(r: &dyn Runner, f: &Fleet, d: &Path, p: &Plan, undo: bool) -> Result<()> {
    let Some(m) = p.main() else {
        return refused("COMPACT_MOVED", "this plan does not move main");
    };
    let new = m.new.as_deref().unwrap_or_default();
    let (from, to) = if undo {
        (new, m.old.as_str())
    } else {
        (m.old.as_str(), new)
    };
    if at(r, f, p, undo, true)? > 0 {
        push_all(
            r,
            &copy(r, f, d, p)?,
            &origin(r, f)?,
            &[(m.name.as_str(), from, Some(to))],
        )?;
    }
    Ok(())
}

/// The approved undo: every ref but main back to its old value, a lease on its new one, then `compact-undone`.
pub fn undo(ctx: &Ctx, r: &dyn Runner, f: &Fleet, d: &Path, p: &Plan, names: &[&String]) -> Result<()> {
    let copy = copy(r, f, d, p)?;
    if at(r, f, p, true, false)? > 0 {
        let mv: Vec<(&str, &str, Option<&str>)> = (p.refs.iter().filter(|m| !p.is_main(m)))
            .map(|m| {
                (
                    m.name.as_str(),
                    m.new.as_deref().unwrap_or_default(),
                    Some(m.old.as_str()),
                )
            })
            .collect();
        push_all(r, &copy, &origin(r, f)?, &mv)?;
    }
    let evs: Vec<Event> = (names.iter())
        .map(|n| Event::new(ctx.now, n, None, "compact-undone", json!({"run": p.new_tip})).passive())
        .collect();
    crate::db::append_all(&ctx.state, &evs)
}
