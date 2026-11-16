pub mod plan;
pub mod push;
pub mod reset;
pub mod shas;
pub mod write;

pub use shas::{rooted, Run, Shas};

use crate::config::{Config, Fleet};
use crate::events::{fold, history, Event, Json};
use crate::gate::refused;
use crate::runner::{json, ok, Runner};
use crate::Ctx;
use anyhow::{Context, Result};
use plan::{Class, Kind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// What `drover compact` is asked to do.
pub enum Act {
    Cut { cut: String, thread: Option<String> },
    Undo { thread: Option<String> },
    Push { tip: String, undo: bool },
    Reset,
}

/// A ref's move as the plan file keeps it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefMove {
    pub name: String,
    pub old: String,
    pub new: Option<String>,
    pub class: String,
    pub pr: Option<u64>,
}

/// One commit of the new line after the root: old and new id, subject and how many old commits it holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub old: String,
    pub new: String,
    pub subject: String,
    pub held: usize,
}

/// The planned compact of a repo: what the dry run prints, the card pins and the push sends.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Plan {
    pub repo: String,
    pub line: String,
    pub cut: String,
    pub root: String,
    pub old_tip: String,
    pub new_tip: String,
    pub archive: String,
    pub commits: [usize; 2],
    pub refs: Vec<RefMove>,
    pub chain: Vec<Step>,
}

/// The repo's scratch folder: the bare copy, the plan and the map.
pub fn dir(ctx: &Ctx, repo: &str) -> PathBuf {
    ctx.state.with_file_name("compact").join(repo)
}

fn is_sha(s: &str) -> bool {
    s.len() >= 7 && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn origin(r: &dyn Runner, f: &Fleet) -> Result<String> {
    let local = f.local_path();
    Ok(ok(
        r,
        "git",
        &["-C", &local.to_string_lossy(), "remote", "get-url", "origin"],
    )?
    .trim()
    .to_string())
}

/// The map file as rows: old id → (new id, kind).
pub fn map(d: &Path) -> Result<BTreeMap<String, (String, String)>> {
    let text = std::fs::read_to_string(d.join("compact-map.tsv")).context("no compact-map.tsv: run --cut first")?;
    let row = |l: &str| {
        let mut w = l.split('\t').map(String::from);
        Some((w.next()?, (w.next()?, w.next()?)))
    };
    Ok(text.lines().filter_map(row).collect())
}

fn load(d: &Path) -> Result<Plan> {
    let text = std::fs::read_to_string(d.join("plan.json")).context("no plan.json: run --cut first")?;
    Ok(serde_json::from_str(&text)?)
}

fn build(ctx: &Ctx, r: &dyn Runner, f: &Fleet, cut: &str) -> Result<Plan> {
    let d = dir(ctx, &f.repo);
    std::fs::create_dir_all(&d)?;
    let copy = d.join("old.git");
    write::mirror(r, &origin(r, f)?, &copy)?;
    let prs = json(
        r,
        "gh",
        &[
            "pr",
            "list",
            "-R",
            &f.repo,
            "--state",
            "open",
            "--limit",
            "500",
            "--json",
            "number,headRefName,isCrossRepository",
        ],
    )?;
    let prs = prs.as_array().cloned().unwrap_or_default();
    let open = (prs.iter())
        .filter_map(|p| Some((p["number"].as_u64()?, p["headRefName"].as_str()?.to_string())))
        .collect();
    let old = write::read(r, &copy, open)?;
    let g = |a: &[&str]| ok(r, "git", &[&["-C", &copy.to_string_lossy()][..], a].concat());
    let line = format!("refs/heads/{}", f.integration);
    let (sha, label) = if is_sha(cut) {
        let s = g(&["rev-parse", &format!("{cut}^{{commit}}")])?;
        let day = g(&["log", "-1", "--format=%cs", s.trim()])?;
        (s, day)
    } else {
        let before = format!("--before={cut} 00:00");
        (
            g(&["rev-list", "-1", "--first-parent", &before, &line])?,
            cut.to_string(),
        )
    };
    let (sha, label) = (sha.trim().to_string(), label.trim().to_string());
    if sha.is_empty() {
        return refused("BAD_VALUE", format!("no commit on {} before {cut}", f.integration));
    }
    let archive = format!("{}-history", f.repo);
    let msg = format!("{}: initial (history before {label} in {archive})\n", f.repo_name());
    let shape = plan::plan(&old, &f.integration, &sha, &msg).map_err(|e| crate::gate::refuse("BAD_VALUE", e))?;
    let (ids, notes) = write::write(r, &copy, &shape)?;
    let id = |n: usize| ids.get(n).cloned().unwrap_or_default();
    let pr_of = |name: &str| {
        let b = name.strip_prefix("refs/heads/")?;
        old.open.iter().find(|o| o.1 == b).map(|o| o.0)
    };
    let refs: Vec<RefMove> = (shape.refs.iter())
        .map(|m| RefMove {
            name: m.name.clone(),
            old: m.old.clone(),
            new: match m.class {
                Class::Notes => notes.iter().find(|n| n.0 == m.name).map(|n| n.1.clone()),
                _ => m.to.map(id),
            },
            class: format!("{:?}", m.class),
            pr: pr_of(&m.name),
        })
        .collect();
    let checked: Vec<&RefMove> = refs.iter().filter(|m| m.class != "Notes" && m.new.is_some()).collect();
    let ends: Vec<&str> = (checked.iter())
        .flat_map(|m| [m.old.as_str(), m.new.as_deref().unwrap_or_default()])
        .collect();
    let trees = write::trees(r, &copy, &ends)?;
    let bad: Vec<&str> = (checked.iter().zip(trees.chunks(2)))
        .filter(|(_, t)| t.first() != t.get(1))
        .map(|(m, _)| m.name.as_str())
        .collect();
    if !bad.is_empty() {
        return refused(
            "COMPACT_TREE",
            format!("a new tip's tree differs: {}; nothing was sent", bad.join(", ")),
        );
    }
    let mut held = vec![0usize; shape.nodes.len()];
    for (n, _) in shape.place.values().filter(|p| p.1 == Kind::Held) {
        if let Some(h) = held.get_mut(*n) {
            *h += 1;
        }
    }
    let chain = (shape.line.iter().enumerate())
        .map(|(i, o)| Step {
            old: o.clone(),
            new: id(i + 1),
            subject: old
                .commits
                .get(o)
                .map_or("", |c| c.message.lines().next().unwrap_or_default())
                .to_string(),
            held: held.get(i + 1).copied().unwrap_or_default(),
        })
        .collect();
    let old_tip = (old.refs.iter().find(|x| x.0 == line)).map_or(String::new(), |x| x.1.clone());
    let count = g(&["rev-list", "--count", &old_tip])?
        .trim()
        .parse()
        .unwrap_or_default();
    let p = Plan {
        repo: f.repo.clone(),
        line: f.integration.clone(),
        cut: sha,
        root: id(0),
        new_tip: id(shape.line.len()),
        old_tip,
        archive,
        commits: [count, shape.line.len() + 1],
        refs,
        chain,
    };
    let mut rows: Vec<String> = (shape.place.iter())
        .map(|(o, (n, k))| format!("{o}\t{}\t{}\n", id(*n), k.word()))
        .collect();
    rows.sort();
    std::fs::write(d.join("compact-map.tsv"), rows.concat())?;
    std::fs::write(d.join("plan.json"), serde_json::to_string_pretty(&p)?)?;
    Ok(p)
}

fn fleets<'a>(cfg: &'a Config, repo: &str) -> Vec<(&'a String, &'a Fleet)> {
    cfg.fleet.iter().filter(|(_, f)| f.repo == repo).collect()
}

/// Every reason the repo is not ready to compact (none: ready).
pub fn ready(cfg: &Config, r: &dyn Runner, events: &[Event], p: &Plan, prs: &[Value]) -> Vec<String> {
    let mut why = vec![];
    let st = fold(events);
    for (name, f) in fleets(cfg, &p.repo) {
        if !f.paused {
            why.push(format!("fleet {name} is not paused"));
        }
        for s in st.values().filter(|s| s.fleet == *name && !s.done()) {
            if s.state.starts_with("review") || s.state.starts_with("verify") {
                why.push(format!("#{} has a check lane working ({})", s.issue, s.state));
            }
        }
    }
    let names: Vec<&String> = fleets(cfg, &p.repo).into_iter().map(|x| x.0).collect();
    for (wt, machine) in reset::folders(r, events, &names) {
        let on = crate::hosts::On::new(r, machine.as_deref());
        if let Some(d) = crate::spawn::dirty(&on, &wt) {
            why.push(format!("{}: {d}", wt.display()));
        }
    }
    for pr in prs.iter().filter(|p| p["isCrossRepository"] == true) {
        why.push(format!("PR #{} comes from a fork", pr["number"]));
    }
    for m in p.refs.iter().filter(|m| m.class == "Detached" && m.pr.is_some()) {
        why.push(format!(
            "PR #{} would be detached: update or close it",
            m.pr.unwrap_or_default()
        ));
    }
    for b in [p.line.as_str(), "main"] {
        let api = format!("repos/{}/branches/{b}/protection", p.repo);
        let v = json(r, "gh", &["api", &api]).unwrap_or(Value::Null);
        if v.at("allow_force_pushes").at("enabled") == false {
            why.push(format!("force pushes are not allowed on {b}"));
        }
    }
    why
}

/// Each issue's stored ids that now live in the new history: (fleet, issue, its `moved` list).
pub fn moves(
    events: &[Event],
    names: &[&String],
    m: &BTreeMap<String, (String, String)>,
    root: &str,
) -> Vec<(String, u64, Value)> {
    let mut by: BTreeMap<(String, u64), Vec<String>> = BTreeMap::new();
    for e in events
        .iter()
        .filter(|e| names.contains(&&e.fleet) && ["merged", "verdict", "why"].contains(&e.kind.as_str()))
    {
        let Some(i) = e.issue else { continue };
        let ids = by.entry((e.fleet.clone(), i)).or_default();
        for k in ["merge_sha", "verdict_sha", "sha"] {
            ids.extend(e.str(k).filter(|s| !ids.iter().any(|x| x == s)).map(String::from));
        }
    }
    let mut out = vec![];
    for ((fleet, i), ids) in by {
        let mut moved: Vec<(String, String, Vec<String>)> = vec![];
        let kind = |s: &String| m.get(s).filter(|(to, _)| to != root).map(|(to, k)| (to, k.as_str()));
        let mut ids: Vec<&String> = ids.iter().filter(|s| kind(s).is_some()).collect();
        ids.sort_by_key(|s| kind(s).map(|k| k.1 != "same"));
        for s in ids {
            match kind(s) {
                Some((to, "same")) => moved.push((s.clone(), to.clone(), vec![])),
                Some((to, "held")) => match moved.iter_mut().find(|x| &x.1 == to) {
                    Some(x) => x.2.push(s.clone()),
                    None => moved.push((String::new(), to.clone(), vec![s.clone()])),
                },
                _ => {}
            }
        }
        for x in moved.iter_mut().filter(|x| x.0.is_empty()) {
            x.0 = m
                .iter()
                .find(|(_, v)| v.0 == x.1 && v.1 == "same")
                .map(|(o, _)| o.clone())
                .unwrap_or_default();
        }
        let v: Vec<Value> = moved
            .into_iter()
            .map(|(f, t, h)| json!({"from": f, "to": t, "held": h}))
            .collect();
        if !v.is_empty() {
            out.push((fleet, i, Value::Array(v)));
        }
    }
    out
}

fn report(out: &mut dyn Write, p: &Plan, mv: &[(String, u64, Value)], why: &[String]) -> Result<()> {
    writeln!(out, "compact {} · line {} · cut {:.9}", p.repo, p.line, p.cut)?;
    let [a, b] = p.commits;
    writeln!(out, "commits   {} {a} → {b} (1 root + {})", p.line, b - 1)?;
    writeln!(
        out,
        "root      {:.12} · history before the cut in {}",
        p.root, p.archive
    )?;
    writeln!(out, "chain     {}", p.chain.len())?;
    for s in &p.chain {
        writeln!(
            out,
            "  {:.9} ← {:.9}  {}  holds {} old commits",
            s.new, s.old, s.subject, s.held
        )?;
    }
    let n = |c: &str| p.refs.iter().filter(|m| m.class == c).count();
    let classes = ["Line", "SameTree", "Carried", "Detached", "Merged", "Notes"];
    let each: Vec<String> = classes.iter().map(|c| format!("{c} {}", n(c))).collect();
    writeln!(out, "refs      {}: {}", p.refs.len(), each.join(" · "))?;
    let open: Vec<&RefMove> = p.refs.iter().filter(|m| m.pr.is_some()).collect();
    writeln!(out, "open PRs  {}, each costs one fresh review", open.len())?;
    for m in open {
        let new = m.new.as_deref().unwrap_or("deleted");
        writeln!(
            out,
            "  #{} {}  {:.9} → {new:.9}  {}",
            m.pr.unwrap_or_default(),
            m.name,
            m.old,
            m.class
        )?;
    }
    writeln!(out, "events    {} issues to remap", mv.len())?;
    for (fleet, i, v) in mv {
        for x in v.as_array().into_iter().flatten() {
            let held = x["held"].as_array().map_or(0, Vec::len);
            let (f, t) = (
                x["from"].as_str().unwrap_or_default(),
                x["to"].as_str().unwrap_or_default(),
            );
            writeln!(out, "  {fleet}#{i}  {f:.9} → {t:.9}, {held} lane heads held")?;
        }
    }
    let checks = if why.is_empty() {
        "ready ✓".to_string()
    } else {
        why.join(" · ")
    };
    writeln!(out, "checks    tip trees equal ✓ · {checks}")?;
    writeln!(out, "new tip   {} · nothing pushed", p.new_tip)?;
    Ok(())
}

/// `drover compact <fleet>`: plan and print (`dry`), post its card, run the approved push, undo or reset.
pub fn run(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    fleet: &str,
    act: &Act,
    dry: bool,
) -> Result<()> {
    let f = cfg.fleet(fleet)?;
    let d = dir(ctx, &f.repo);
    let events = history(&ctx.state)?;
    let names: Vec<&String> = fleets(cfg, &f.repo).into_iter().map(|x| x.0).collect();
    let card = |thread: &str, cmd: String, what: &str| -> Result<String> {
        let c = crate::card::Card {
            cmd: &cmd,
            why: what,
            blast: &format!("force-moves every ref of {} (archive first, lease on each)", f.repo),
            dir: &ctx.state.parent().unwrap_or(Path::new(".")).to_string_lossy(),
            timeout: 3600,
        };
        let id = crate::card::post(ctx, cfg, thread, &c)?;
        Ok(format!("run card {id} posted: {cmd}"))
    };
    match act {
        Act::Cut { cut, thread } => {
            if let Some(done) = push::finished(ctx, r, f, &d, &events, &names)? {
                return Ok(writeln!(out, "{done}")?);
            }
            let p = build(ctx, r, f, cut)?;
            let prs = json(
                r,
                "gh",
                &[
                    "pr",
                    "list",
                    "-R",
                    &f.repo,
                    "--state",
                    "open",
                    "--limit",
                    "500",
                    "--json",
                    "number,isCrossRepository",
                ],
            )?;
            let why = ready(cfg, r, &events, &p, prs.as_array().map_or(&[][..], Vec::as_slice));
            report(out, &p, &moves(&events, &names, &map(&d)?, &p.root), &why)?;
            match thread.as_deref().filter(|_| !dry) {
                None => Ok(()),
                Some(_) if !why.is_empty() => refused("COMPACT_NOT_READY", why.join("; ")),
                Some(t) => {
                    let line = card(
                        t,
                        format!("drover compact {fleet} --push {}", p.new_tip),
                        "compact: archive, then one atomic push",
                    )?;
                    Ok(writeln!(out, "{line}")?)
                }
            }
        }
        Act::Undo { thread } => {
            let p = load(&d)?;
            push::at(r, f, &p, true)?;
            writeln!(
                out,
                "undo {} · {} refs back to their old values · line {:.9} → {:.9}",
                p.repo,
                p.refs.len(),
                p.new_tip,
                p.old_tip
            )?;
            match thread.as_deref().filter(|_| !dry) {
                None => Ok(writeln!(out, "nothing pushed")?),
                Some(t) => {
                    let line = card(
                        t,
                        format!("drover compact {fleet} --undo --push {}", p.old_tip),
                        "undo the compact",
                    )?;
                    Ok(writeln!(out, "{line}")?)
                }
            }
        }
        Act::Push { tip, undo } => {
            let want = format!(
                "drover compact {fleet}{} --push {tip}",
                if *undo { " --undo" } else { "" }
            );
            if !crate::card::running(&events, &want) {
                return refused(
                    "COMPACT_UNAPPROVED",
                    format!("`{want}` runs only inside its approved run card"),
                );
            }
            let p = load(&d)?;
            if *tip != *if *undo { &p.old_tip } else { &p.new_tip } {
                return refused(
                    "COMPACT_MOVED",
                    format!("{tip:.12} is not this plan's tip; run --cut again"),
                );
            }
            if *undo {
                push::undo(ctx, r, f, &d, &p, &names)?;
            } else {
                let prs = json(
                    r,
                    "gh",
                    &[
                        "pr",
                        "list",
                        "-R",
                        &f.repo,
                        "--state",
                        "open",
                        "--limit",
                        "500",
                        "--json",
                        "number,isCrossRepository",
                    ],
                )?;
                let why = ready(cfg, r, &events, &p, prs.as_array().map_or(&[][..], Vec::as_slice));
                if !why.is_empty() {
                    return refused("COMPACT_NOT_READY", why.join("; "));
                }
                push::push(ctx, r, f, &d, &p, &events, &names)?;
            }
            reset::all(r, f, &events, &names, out)
        }
        Act::Reset => reset::all(r, f, &events, &names, out),
    }
}
