use crate::config::{Config, Fleet};
use crate::events::{fold, Event, Json};
use crate::gate::refused;
use crate::runner::{ok, Runner};
use crate::{gh, Ctx};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

struct Blamed {
    blame: String,
    dir: String,
    tip: Option<String>,
    shas: crate::reroot::Shas,
}

impl Blamed {
    fn now<'a>(&'a self, s: &'a str) -> &'a str {
        if self.tip.is_some() {
            s
        } else {
            self.shas.now(s)
        }
    }
    fn since(&self, sha: &str, f: &Fleet) -> String {
        format!(
            "{sha}..{}",
            self.tip.clone().unwrap_or(format!("origin/{}", f.integration))
        )
    }
}

fn blamed(ctx: &Ctx, r: &dyn Runner, dir: &str, (name, line): (&str, u64), events: &[Event]) -> Result<Blamed> {
    let git = |d: &str, a: &[&str]| ok(r, "git", &[&["-C", d][..], a].concat());
    let lines = format!("{line},{line}");
    let blame = git(dir, &["blame", "--porcelain", "-L", &lines, "--", name])?;
    let shas = crate::reroot::Shas::of(events);
    let first = blame.split_whitespace().next().unwrap_or_default();
    let Some(run) = shas.root(first) else {
        return Ok(Blamed {
            blame,
            dir: dir.to_string(),
            tip: None,
            shas,
        });
    };
    let copy = crate::reroot::dir(ctx, &run.repo).join("old.git");
    if !copy.join("HEAD").exists() {
        let why = format!(
            "{first:.9} is older than the compact's cut; its history is in {}",
            run.archive
        );
        return refused("BEFORE_CUT", why);
    }
    let path = git(dir, &["rev-parse", "--show-prefix"])?.trim().to_string() + name;
    let copy = copy.to_string_lossy().into_owned();
    let blame = git(&copy, &["blame", "--porcelain", "-L", &lines, &run.cut, "--", &path])?;
    let tip = Some(run.line.clone());
    Ok(Blamed {
        blame,
        dir: copy,
        tip,
        shas,
    })
}

/// `drover why <file:line>`: blame's commit, the issue that merged it and the issue's asks with their answers; plain text, or `json`.
pub fn show(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, at: &str, as_json: bool) -> Result<()> {
    let (file, line) = (at.rsplit_once(':'))
        .and_then(|(f, l)| Some((f, l.parse::<u64>().ok()?)))
        .context("usage: drover why <file:line>")?;
    let p = Path::new(file);
    let dir = p
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let events = crate::events::history(&ctx.state)?;
    let b = blamed(ctx, r, &dir.to_string_lossy(), (&name, line), &events)?;
    let git = |a: &[&str]| ok(r, "git", &[&["-C", &b.dir][..], a].concat());
    let sha = b.blame.split_whitespace().next().unwrap_or_default().to_string();
    let text = b.blame.lines().find_map(|l| l.strip_prefix('\t')).unwrap_or_default();
    let subject = (b.blame.lines())
        .find_map(|l| l.strip_prefix("summary "))
        .unwrap_or_default();
    let url = git(&["remote", "get-url", "origin"]).unwrap_or_default();
    let (fleet, f) = (cfg.fleet.iter().find(|(_, f)| url.contains(&f.repo)))
        .or_else(|| cfg.fleet.iter().next())
        .context("no fleet in fleet.toml")?;
    let since = git(&["log", "--merges", "--ancestry-path", "--format=%H", &b.since(&sha, f)]);
    let since = since.unwrap_or_default();
    let by_sha = |m: &str| {
        events
            .iter()
            .find(|e| e.kind == "merged" && e.fleet == *fleet && e.str("merge_sha").map(|s| b.now(s)) == Some(m))
    };
    let merged = by_sha(&sha).or_else(|| since.lines().rev().find_map(by_sha));
    let issue = (merged.and_then(|e| e.issue)).or_else(|| {
        subject
            .rsplit_once("(#")
            .and_then(|(_, t)| t.split(')').next()?.parse().ok())
    });
    let ask = |e: &Event| {
        let id = e.str("id")?;
        let d = events
            .iter()
            .rfind(|d| d.kind == "decision" && d.fleet == *fleet && d.str("id") == Some(id));
        let answer = d.map(|d| json!({"choice": d.str("choice"), "by": d.str("by").map(|b| crate::orchestrator::label(cfg, &events, b)), "via": d.str("via")}));
        (issue.is_some() && e.issue == issue)
            .then(|| json!({"id": id, "kind": e.str("kind"), "question": e.str("question"), "answer": answer}))
    };
    let asks: Vec<Value> = (events
        .iter()
        .filter(|e| e.kind == "needs-decision" && e.fleet == *fleet))
    .filter_map(ask)
    .collect();
    let st = issue.and_then(|n| fold(&events).remove(&(fleet.clone(), n)));
    let issue = issue.map(|n| {
        let title = gh::spec(ctx, r, f, fleet, n, false).ok().map(|v| v.at("title").clone());
        let m = |k: &str| merged.map_or(Value::Null, |e| e.val(k).clone());
        let rounds = st.as_ref().map(|s| s.rounds);
        json!({"fleet": fleet, "number": n, "title": title, "pr": m("pr"), "merge_sha": m("merge_sha"), "into": m("into"), "rounds": rounds})
    });
    let v = json!({"file": file, "line": line, "text": text, "commit": {"sha": sha, "subject": subject},
        "issue": issue, "asks": asks});
    if as_json {
        return Ok(writeln!(out, "{v}")?);
    }
    let s = |v: &Value| v.as_str().map_or_else(|| v.to_string(), String::from);
    writeln!(out, "line    {at}  {}\ncommit  {sha:.9}  {subject}", text.trim())?;
    match v.at("issue") {
        Value::Null => writeln!(out, "issue   none")?,
        i => {
            writeln!(out, "issue   #{} {}", i["number"], s(&i["title"]))?;
            if !i["pr"].is_null() {
                let (pr, m, into) = (s(&i["pr"]), s(&i["merge_sha"]), s(&i["into"]));
                writeln!(
                    out,
                    "        PR #{pr} · merged {m:.9} into {into} · {} review round(s)",
                    i["rounds"]
                )?;
            }
        }
    }
    if !asks.is_empty() {
        writeln!(out, "asks")?;
    }
    for a in &asks {
        writeln!(out, "  {} {} · {}", s(&a["id"]), s(&a["kind"]), s(&a["question"]))?;
        let ans = &a["answer"];
        match ans {
            Value::Null => writeln!(out, "       → unanswered")?,
            _ => writeln!(
                out,
                "       → {} ({}): {}",
                s(&ans["by"]),
                s(&ans["via"]),
                s(&ans["choice"])
            )?,
        }
    }
    Ok(())
}
