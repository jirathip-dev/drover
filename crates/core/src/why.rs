use crate::config::{Config, Fleet};
use crate::events::{emit, fold, last_spawn, read, Event};
use crate::runner::{argv, json, ok, Runner};
use crate::{gh, hosts::On, Ctx};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

pub const MISSING: &str = "why.md missing: write it (Decided/Because/Rejected/Check/Owner), then turn done again";
const KEYS: [&str; 5] = ["Decided", "Because", "Rejected", "Check", "Owner"];
const NOTES: &str = "+refs/notes/why:refs/notes/why";

/// The note's `Owner:` line, or why `text` is no valid note. `short`: an update round's (q817: no `Rejected:` or `Check:` needed). A `Because:` cites a quote of `spec` (q816), an `answered` ask or a `file:line` within `len(file)`.
pub fn check(
    text: &str,
    short: bool,
    spec: &str,
    answered: &dyn Fn(&str) -> bool,
    len: &dyn Fn(&str) -> Option<usize>,
) -> Result<String, String> {
    let lines: Vec<(usize, &str)> = (text.lines().map(str::trim).zip(1..))
        .filter(|l| !l.0.is_empty())
        .map(|(l, n)| (n, l))
        .collect();
    if lines.len() > 15 || text.len() > 2048 {
        return Err(format!(
            "why.md form: {} lines, {} bytes (max 15, 2 KB)",
            lines.len(),
            text.len()
        ));
    }
    let mut keys = vec![];
    let mut owner = String::new();
    for (n, l) in lines {
        if crate::lanes::secret(l).is_some() {
            return Err(format!("why.md:{n} secret: {}", crate::lanes::redact(l)));
        }
        let (k, v) = l.split_once(':').map_or((l, ""), |(k, v)| (k, v.trim()));
        let bad = |w: &str| Err(format!("why.md:{n} form: {w}"));
        if !KEYS.contains(&k) {
            return bad(&format!("\"{k}:\" is no note key"));
        } else if v.is_empty() || l.chars().count() > 240 {
            return bad(&format!("{k}: needs one line of 1-240 chars"));
        } else if k == "Rejected" && !(v.contains('—') || v.contains("--") || v.starts_with("none")) {
            return bad("Rejected: is `alternative — why`");
        } else if k == "Because" {
            cite(v, spec, answered, len).map_err(|e| format!("why.md:{n} {e}"))?;
        }
        keys.push(k);
        owner = v.to_string();
    }
    let need: &[&str] = if short { &KEYS[..2] } else { &KEYS[..4] };
    if let Some(k) = need.iter().find(|k| !keys.contains(k)) {
        return Err(format!("why.md form: no {k}: line"));
    }
    if keys.iter().filter(|k| **k == "Owner").count() != 1 || keys.last() != Some(&"Owner") {
        return Err("why.md form: one Owner: line, the last".into());
    }
    Ok(owner)
}

fn cite(
    v: &str,
    spec: &str,
    answered: &dyn Fn(&str) -> bool,
    len: &dyn Fn(&str) -> Option<usize>,
) -> Result<(), String> {
    let flat = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut bad = None;
    let parts: Vec<&str> = v.split('"').collect();
    for q in parts.iter().skip(1).step_by(2).take((parts.len() - 1) / 2) {
        if q.chars().count() >= 12 {
            if flat(spec).contains(&flat(q)) {
                return Ok(());
            }
            let q: String = q.chars().take(40).collect();
            bad.get_or_insert(format!("\"{q}\" is not in the spec"));
        }
    }
    for w in v.split(|c: char| c.is_whitespace() || "()[],;`'\"".contains(c)) {
        let w = w.trim_end_matches(['.', ':']);
        let digits = |d: &str| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit());
        if w.strip_prefix('q').is_some_and(digits) {
            if answered(w) {
                return Ok(());
            }
            bad.get_or_insert(format!("{w} is no answered ask"));
        } else if let Some((p, l)) = w.rsplit_once(':').filter(|(p, l)| p.contains(['.', '/']) && digits(l)) {
            let l: usize = l.parse().unwrap_or(0);
            match len(p) {
                Some(n) if (1..=n).contains(&l) => return Ok(()),
                Some(n) => bad.get_or_insert(format!("{p}:{l} past its end ({n})")),
                None => bad.get_or_insert(format!("{p} is not in the head tree")),
            };
        }
    }
    Err(bad.map_or(
        "uncited: a Because: needs \"spec quote\", q<id> or file:line".into(),
        |b| format!("cite: {b}"),
    ))
}

/// `turn done`'s note for `role`'s lane: None when none is due (no PR, nothing new pushed) or it is on origin; else the `WHY_INVALID` reason (nothing written or pushed).
pub fn turn(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, n): (&str, u64),
    role: &str,
) -> Result<Option<(&'static str, String)>> {
    let f = cfg.fleet(fleet)?;
    let events = read(&ctx.state)?;
    let Some(s) = fold(&events).remove(&(fleet.to_string(), n)) else {
        return Ok(None);
    };
    let agent = s.lanes.get(role).cloned().unwrap_or_default();
    let (Some(sp), branch) = (last_spawn(&events, &agent), crate::gate::branch_of(Some(&s), n)) else {
        return Ok(None);
    };
    let fields = [
        "pr",
        "view",
        &branch,
        "-R",
        &f.repo,
        "--json",
        "number,body,headRefOid,state",
    ];
    let pr = json(r, "gh", &fields).unwrap_or_default();
    let head = pr["headRefOid"].as_str().unwrap_or_default();
    let noted = events
        .iter()
        .any(|e| e.is("why", fleet, n) && e.str("sha") == Some(head));
    if head.is_empty() || pr["state"] != "OPEN" || s.reviewed(head) || noted {
        return Ok(None);
    }
    let (on, wt) = (On::of(r, Some(sp)), sp.str("worktree").unwrap_or_default());
    let git = |a: &[&str]| {
        on.run("git", &argv(&[&["-C", wt][..], a].concat()))
            .ok()
            .filter(|o| o.code == 0)
    };
    let subject = |c: &str| git(&["log", "-1", "--format=%s", c]).map(|o| o.stdout.trim().to_string());
    let parent = |c: &str| git(&["rev-parse", &format!("{c}^1")]).map(|o| o.stdout.trim().to_string());
    if let Err(e) = body(
        pr["body"].as_str().unwrap_or_default(),
        head,
        &f.integration,
        &subject,
        &parent,
    ) {
        return Ok(Some(("PR_BODY", e)));
    }
    let path = Path::new(wt).join("why.md");
    let Some(text) = on.read(&path) else {
        return Ok(Some(("WHY_INVALID", MISSING.into())));
    };
    let spec = gh::spec(ctx, r, f, fleet, n, false).unwrap_or_default();
    let answered =
        |q: &str| (events.iter()).any(|e| e.kind == "decision" && e.fleet == fleet && e.str("id") == Some(q));
    let len = |p: &str| {
        let o = on
            .run("git", &argv(&["-C", wt, "cat-file", "-p", &format!("{head}:{p}")]))
            .ok()?;
        (o.code == 0).then(|| o.stdout.lines().count())
    };
    let short = sp.data.get("update") == Some(&Value::Bool(true));
    let owner = match check(&text, short, spec["body"].as_str().unwrap_or_default(), &answered, &len) {
        Ok(o) => o,
        Err(e) => return Ok(Some(("WHY_INVALID", e))),
    };
    let round = s.rounds + 1;
    if let Err(e) = attach(ctx, r, f, fleet, (&branch, head), &text) {
        writeln!(out, "why note not pushed: {e:#}")?;
        let d = json!({"sha": head, "round": round, "error": format!("{e:#}")});
        emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "why-failed", d))?;
        return Ok(None);
    }
    let body = section(pr["body"].as_str().unwrap_or_default(), round, &text, head);
    if let Err(e) = ok(
        r,
        "gh",
        &["pr", "edit", &pr["number"].to_string(), "-R", &f.repo, "--body", &body],
    ) {
        writeln!(out, "PR body: {e:#}")?;
    }
    let prev = events
        .iter()
        .rfind(|e| e.is("why", fleet, n))
        .and_then(|e| e.str("sha"));
    let range = prev.map_or(format!("origin/{}...{head}", f.integration), |p| format!("{p}..{head}"));
    let local = f.local_path().to_string_lossy().into_owned();
    let files = ok(r, "git", &["-C", &local, "diff", "--name-only", &range]).unwrap_or_default();
    let files: Vec<&str> = files.lines().collect();
    let d = json!({"sha": head, "round": round, "branch": branch, "agent": agent, "owner": owner, "files": files});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "why", d))?;
    on.remove(&path)?;
    writeln!(out, "why note on {head:.7} (round {round}) pushed to refs/notes/why")?;
    Ok(None)
}

/// #925: `body` has `## For humans`, no "Generated with" line, and names `head` as `scripts/evidence-check.sh` does.
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

/// Put `text` on `head` under `refs/notes/why` on origin, from the fleet's local clone (the `why-notes:<fleet>` lease held while free): origin's notes fetched into this write's own ref, the note added, pushed (never forced); a rejected push (another writer first) starts over, 3 tries. The local `refs/notes/why` follows origin's after.
pub fn attach(
    ctx: &Ctx,
    r: &dyn Runner,
    f: &Fleet,
    fleet: &str,
    (branch, head): (&str, &str),
    text: &str,
) -> Result<()> {
    let key = format!("why-notes:{fleet}");
    let mut lease = None;
    for _ in 0..60 {
        lease = crate::db::take(&ctx.state, &key, ctx.now, 120, "", &|_| false);
        if lease.is_some() {
            break;
        }
        r.sleep(1);
    }
    let local = f.local_path().to_string_lossy().into_owned();
    let git = |a: &[&str], input: &str| r.run_in("git", &argv(&[&["-C", &local][..], a].concat()), input);
    let mine = format!("refs/notes/why-{head}");
    let res = (|| {
        ok(r, "git", &["-C", &local, "fetch", "-q", "origin", branch])?;
        let mut err = String::new();
        for _ in 0..3 {
            git(&["update-ref", "-d", &mine], "")?;
            let _ = git(&["fetch", "-q", "origin", &format!("+refs/notes/why:{mine}")], "");
            let id = ["-c", "user.name=drover", "-c", "user.email=drover@localhost"];
            let o = git(
                &[&id[..], &["notes", "--ref", &mine, "add", "-f", "-F", "-", head]].concat(),
                text,
            )?;
            if o.code != 0 {
                bail!("git notes add: {}", o.stderr.trim());
            }
            let o = git(&["push", "-q", "origin", &format!("{mine}:refs/notes/why")], "")?;
            if o.code == 0 {
                let _ = git(&["fetch", "-q", "origin", NOTES], "");
                return Ok(());
            }
            err = o.stderr;
        }
        bail!("git push refs/notes/why: {}", err.trim())
    })();
    let _ = git(&["update-ref", "-d", &mine], "");
    if let Some(l) = lease {
        crate::db::release(&ctx.state, &key, Some(&l));
    }
    res
}

fn section(body: &str, round: u32, text: &str, head: &str) -> String {
    let h = format!("## Why (round {round})");
    let new = format!(
        "{h}\n\n{}\n\n<sub>note on {head:.7} · git log --notes=why</sub>\n",
        text.trim()
    );
    let Some(i) = body.find(&h) else {
        return format!("{}\n\n{new}", body.trim_end());
    };
    match body[i + h.len()..].find("\n## ") {
        Some(j) => format!("{}{new}\n{}", &body[..i], &body[i + h.len() + j + 1..]),
        None => format!("{}{new}", &body[..i]),
    }
}

fn note(r: &dyn Runner, local: &Path, sha: &str) -> Option<String> {
    let local = local.to_string_lossy();
    ok(r, "git", &["-C", &local, "notes", "--ref", "why", "show", sha])
        .ok()
        .map(|t| t.trim().to_string())
}

/// A brief's why-notes section: `rev`'s, the head under review's note (else why there is none); an impl lane's, every note on its branch, else (a new impl) the latest 1–3 notes on each file its `spec` names (5 files, 60 lines).
pub fn brief(
    r: &dyn Runner,
    f: &Fleet,
    events: &[Event],
    (fleet, n): (&str, u64),
    rev: Option<&str>,
    spec: &str,
) -> Option<String> {
    let local = f.local_path();
    let mine: Vec<&Event> = events.iter().filter(|e| e.is("why", fleet, n)).collect();
    if let Some(head) = rev {
        let text = mine
            .iter()
            .any(|e| e.str("sha") == Some(head))
            .then(|| note(r, &local, head))
            .flatten();
        if let Some(t) = text {
            return Some(format!("## Why note for this round ({head:.7})\n\n{t}"));
        }
        let done = |e: &&Event| e.is("turn-end", fleet, n) && e.str("state") == Some("done");
        let why = events.iter().rfind(done).and_then(|e| e.str("note"));
        return Some(format!(
            "## Why note for this round\n\nno why note: {}",
            why.unwrap_or("none attached")
        ));
    }
    if !mine.is_empty() {
        let each = |e: &&Event| {
            let sha = e.str("sha")?;
            Some(format!(
                "round {} · {sha:.7}\n{}\n\n",
                e.data.get("round").unwrap_or(&Value::Null),
                note(r, &local, sha)?
            ))
        };
        let all: String = mine.iter().filter_map(each).collect();
        return Some(format!("## Why notes on this branch\n\n{}", all.trim_end()));
    }
    let named = |p: &str| {
        spec.split(|c: char| c.is_whitespace() || "`'\"(),;:".contains(c))
            .any(|w| w.trim_end_matches('.') == p)
    };
    let mut files: Vec<(&str, Vec<(&str, u64)>)> = vec![];
    for e in events.iter().rev().filter(|e| e.kind == "why" && e.fleet == fleet) {
        let (Some(sha), Some(i)) = (e.str("sha"), e.issue) else {
            continue;
        };
        for p in (e.data.get("files").and_then(Value::as_array).into_iter().flatten())
            .filter_map(Value::as_str)
            .filter(|p| named(p))
        {
            match files.iter().position(|x| x.0 == p) {
                Some(x) if files[x].1.len() < 3 => files[x].1.push((sha, i)),
                None if files.len() < 5 => files.push((p, vec![(sha, i)])),
                _ => {}
            }
        }
    }
    let mut s = String::from("## Why notes for the files this issue names\n");
    for (p, shas) in &files {
        s += &format!("\n### {p}\n");
        for (sha, i) in shas {
            if let Some(t) = note(r, &local, sha) {
                s += &format!("\n#{i} · {sha:.7}\n{t}\n");
            }
        }
    }
    (!files.is_empty()).then(|| s.lines().take(60).collect::<Vec<_>>().join("\n"))
}

/// `drover why <file:line>`: blame's commit, the issue that merged it, the note on it (else on the round head that pushed it) and the issue's asks with their answers; plain text, or `json`.
pub fn show(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, at: &str, as_json: bool) -> Result<()> {
    let (file, line) = (at.rsplit_once(':'))
        .and_then(|(f, l)| Some((f, l.parse::<u64>().ok()?)))
        .context("usage: drover why <file:line>")?;
    let p = Path::new(file);
    let dir = p
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let dir = dir.to_string_lossy().into_owned();
    let git = |a: &[&str]| ok(r, "git", &[&["-C", &dir][..], a].concat());
    let _ = git(&["fetch", "-q", "origin", NOTES]);
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let blame = git(&["blame", "--porcelain", "-L", &format!("{line},{line}"), "--", &name])?;
    let sha = blame.split_whitespace().next().unwrap_or_default().to_string();
    let text = blame.lines().find_map(|l| l.strip_prefix('\t')).unwrap_or_default();
    let subject = blame
        .lines()
        .find_map(|l| l.strip_prefix("summary "))
        .unwrap_or_default();
    let events = read(&ctx.state)?;
    let url = git(&["remote", "get-url", "origin"]).unwrap_or_default();
    let (fleet, f) = (cfg.fleet.iter().find(|(_, f)| url.contains(&f.repo)))
        .or_else(|| cfg.fleet.iter().next())
        .context("no fleet in fleet.toml")?;
    let under = |a: &str, b: &str| git(&["merge-base", "--is-ancestor", a, b]).is_ok();
    let since = git(&[
        "log",
        "--merges",
        "--ancestry-path",
        "--format=%H",
        &format!("{sha}..origin/{}", f.integration),
    ]);
    let since = since.unwrap_or_default();
    let by_sha = |m: &str| {
        events
            .iter()
            .find(|e| e.kind == "merged" && e.fleet == *fleet && e.str("merge_sha") == Some(m))
    };
    let merged = since.lines().rev().find_map(by_sha);
    let head = |e: &&Event| e.kind == "why" && e.fleet == *fleet && e.str("sha").is_some_and(|h| under(&sha, h));
    let issue = (merged.and_then(|e| e.issue))
        .or_else(|| {
            subject
                .rsplit_once("(#")
                .and_then(|(_, t)| t.split(')').next()?.parse().ok())
        })
        .or_else(|| events.iter().find(|e| head(e)).and_then(|e| e.issue));
    let whys: Vec<&Event> = events
        .iter()
        .filter(|e| issue.is_some_and(|n| e.is("why", fleet, n)))
        .collect();
    let on = |h: &str| {
        Some((
            h.to_string(),
            git(&["notes", "--ref", "why", "show", h]).ok()?.trim().to_string(),
        ))
    };
    let round = |h: &str| {
        whys.iter()
            .find(|e| e.str("sha") == Some(h))
            .and_then(|e| e.data.get("round").cloned())
    };
    let note = on(&sha).or_else(|| whys.iter().copied().find(|e| head(e)).and_then(|e| on(e.str("sha")?)));
    let note = note.map(|(h, t)| {
        let owner = t
            .lines()
            .rev()
            .find_map(|l| l.strip_prefix("Owner:"))
            .unwrap_or_default()
            .trim()
            .to_string();
        json!({"sha": h, "round": round(&h), "text": t, "owner": owner})
    });
    let cited = |id: &str| {
        note.as_ref()
            .is_some_and(|n| n["text"].as_str().unwrap_or_default().contains(id))
    };
    let ask = |e: &Event| {
        let id = e.str("id")?;
        let d = events
            .iter()
            .rfind(|d| d.kind == "decision" && d.fleet == *fleet && d.str("id") == Some(id));
        let answer = d.map(|d| json!({"choice": d.str("choice"), "by": d.str("by").map(|b| crate::orchestrator::label(cfg, &events, b)), "via": d.str("via")}));
        let mine = issue.is_some() && e.issue == issue;
        (mine || cited(id))
            .then(|| json!({"id": id, "kind": e.str("kind"), "question": e.str("question"), "answer": answer}))
    };
    let asks: Vec<Value> = (events
        .iter()
        .filter(|e| e.kind == "needs-decision" && e.fleet == *fleet))
    .filter_map(ask)
    .collect();
    let st = issue.and_then(|n| fold(&events).remove(&(fleet.clone(), n)));
    let issue = issue.map(|n| {
        let title = gh::spec(ctx, r, f, fleet, n, false).ok().map(|v| v["title"].clone());
        let m = |k: &str| merged.map_or(Value::Null, |e| e.data[k].clone());
        let rounds = st.as_ref().map(|s| s.rounds);
        json!({"fleet": fleet, "number": n, "title": title, "pr": m("pr"), "merge_sha": m("merge_sha"), "into": m("into"), "rounds": rounds})
    });
    let v = json!({"file": file, "line": line, "text": text, "commit": {"sha": sha, "subject": subject},
        "issue": issue, "note": note, "asks": asks});
    if as_json {
        return Ok(writeln!(out, "{v}")?);
    }
    let s = |v: &Value| v.as_str().map_or_else(|| v.to_string(), String::from);
    writeln!(out, "line    {at}  {}\ncommit  {sha:.9}  {subject}", text.trim())?;
    match &v["issue"] {
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
    match &v["note"] {
        Value::Null => writeln!(out, "note    none")?,
        n => {
            writeln!(out, "note    round {} · on {:.9}", n["round"], s(&n["sha"]))?;
            for l in s(&n["text"]).lines() {
                writeln!(out, "  {l}")?;
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
