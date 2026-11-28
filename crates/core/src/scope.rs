use crate::config::{Config, Fleet};
use crate::events::{emit, working, Event, Json};
use crate::gate::refused;
use crate::runner::Runner;
use crate::Ctx;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::Write;

/// The issue number a scope lane runs under: the issue it scopes is not filed yet, and a fleet runs one scope at a time.
pub const LANE: u64 = 0;

/// The marks a draft may queue its issue with.
pub const MARKS: [&str; 3] = ["repro", "design", "architect"];

/// Fleet `fleet`'s requests not yet filed or dropped, oldest first: the first is the one that runs.
pub fn open<'a>(ev: &'a [Event], fleet: &str) -> Vec<&'a Event> {
    let done: BTreeSet<&str> = (ev.iter())
        .filter(|e| e.kind == "scope-done" && e.fleet == fleet)
        .filter_map(|e| e.str("req"))
        .collect();
    let open =
        |e: &&Event| e.kind == "scope-request" && e.fleet == fleet && !done.contains(e.str("req").unwrap_or_default());
    ev.iter().filter(open).collect()
}

/// Request `q`'s scope lane has started.
pub fn started(ev: &[Event], q: &Event) -> bool {
    (ev.iter()).any(|e| e.kind == "scope-started" && e.fleet == q.fleet && e.str("req") == q.str("req"))
}

/// `[New issue]`: the owner's line and files as fleet `fleet`'s scope request, logged once per `req`; it waits while another scope runs.
pub fn request(
    ctx: &Ctx,
    cfg: &Config,
    fleet: &str,
    (text, files): (&str, &[(String, Vec<u8>)]),
    (req, by): (&str, &str),
) -> Result<String> {
    cfg.fleet(fleet)?;
    let text = text.trim();
    if text.is_empty() && files.is_empty() {
        return refused("BAD_VALUE", "a new issue is a line or a screenshot");
    }
    if text.chars().count() > 2000 || text.chars().any(|c| c.is_control() && c != '\n') {
        return refused(
            "BAD_VALUE",
            "a new issue's text is at most 2,000 chars, no control characters",
        );
    }
    crate::orchestrator::files_ok(files)?;
    let ev = working(&ctx.state)?;
    let sent = |e: &Event| e.kind == "scope-request" && e.fleet == fleet && e.str("req") == Some(req);
    if req.is_empty() {
        return refused("BAD_VALUE", "a new issue carries its request id `req`");
    }
    if ev.iter().any(sent) {
        return Ok(format!("{fleet}: already sent"));
    }
    let dir = ctx.state.with_file_name("scope").join(fleet);
    std::fs::create_dir_all(&dir)?;
    let mut saved = vec![];
    for (name, bytes) in files {
        let path = dir.join(format!("{}-{name}", ctx.now));
        std::fs::write(&path, bytes)?;
        saved.push(json!({"name": name, "path": path}));
    }
    let ahead = open(&ev, fleet).len();
    let d = json!({"req": req, "text": text, "files": saved, "by": by});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, None, "scope-request", d))?;
    Ok(match ahead {
        0 => format!("{fleet}: scoping"),
        n => format!("{fleet}: waits behind {n}"),
    })
}

/// The brief's issue: a scope lane's is its request (the owner's line and files), any other lane's the GitHub issue.
pub fn spec(ctx: &Ctx, r: &dyn Runner, (f, fleet, issue): (&Fleet, &str, u64), role: &str) -> Result<Value> {
    if role != "scope" {
        return crate::gh::spec(ctx, r, f, fleet, issue, true);
    }
    let ev = working(&ctx.state)?;
    let q = open(&ev, fleet).into_iter().next().context("no scope request waits")?;
    let quote: Vec<String> = (q.str("text").unwrap_or_default().lines())
        .map(|l| format!("> {l}"))
        .collect();
    let files: Vec<String> = (q.val("files").as_array().into_iter().flatten())
        .map(|f| {
            format!(
                "- {}: `{}`",
                f.at("name").as_str().unwrap_or_default(),
                f.at("path").as_str().unwrap_or_default()
            )
        })
        .collect();
    let files = if files.is_empty() {
        String::new()
    } else {
        format!("\n\nAttached (read each file):\n\n{}", files.join("\n"))
    };
    let body = format!(
        "The owner's new issue, sent from [New issue]:\n\n{}{files}",
        quote.join("\n")
    );
    Ok(json!({"title": "new issue to scope", "body": body}))
}

/// Start fleet `fleet`'s oldest waiting request as its one scope lane, unless a scope runs there.
pub fn next(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<()> {
    let ev = working(&ctx.state)?;
    let Some(q) = open(&ev, fleet).into_iter().next().filter(|q| !started(&ev, q)) else {
        return Ok(());
    };
    let req = q.str("req").unwrap_or_default().to_string();
    let with = r
        .shed()
        .is_none()
        .then(|| cfg.shed())
        .flatten()
        .map(|b| crate::shed::With(r, b));
    let r: &dyn Runner = with.as_ref().map_or(r, |w| w);
    let o = crate::spawn::Opts {
        supersede: true,
        ..crate::spawn::Opts::default()
    };
    crate::spawn::spawn(ctx, cfg, r, out, (fleet, LANE, "scope"), &o)?;
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, fleet, None, "scope-started", json!({"req": req})),
    )
}

/// `drover draft`: the running scope's issue draft as the owner's ask: approve files and queues it, drop files nothing, any other answer is a revise note for the lane.
pub fn draft(
    ctx: &Ctx,
    cfg: &Config,
    out: &mut dyn Write,
    fleet: &str,
    (title, body): (&str, &str),
    marks: &[String],
) -> Result<()> {
    cfg.fleet(fleet)?;
    let ev = working(&ctx.state)?;
    let Some(q) = open(&ev, fleet).into_iter().find(|q| started(&ev, q)) else {
        return refused("NO_SCOPE", format!("no scope lane runs on {fleet}"));
    };
    let (title, body) = (title.trim(), body.trim());
    if title.is_empty() || title.contains('\n') || body.is_empty() {
        return refused("BAD_VALUE", "a draft is a one-line --title and a --body");
    }
    let marks: Vec<&str> = MARKS.into_iter().filter(|m| marks.iter().any(|x| x == m)).collect();
    let sp = ev.iter().rfind(|e| e.is("spawn", fleet, LANE));
    let queued: Vec<String> = marks.iter().map(|m| format!("--{m}")).collect();
    let queued = format!("queued {}", queued.join(" ")).trim_end().to_string();
    let question = format!("**{title}**\n\n{body}\n\n_Approve files it and {queued}._");
    let scope = json!({"req": q.str("req"), "title": title, "body": body, "marks": marks});
    let d = json!({"question": question, "options": ["approve", "drop"], "recommend": 1, "kind": "owner",
        "agent": sp.and_then(|e| e.str("agent")), "scope": scope});
    let ask = Event::new(ctx.now, fleet, None, "needs-decision", d);
    let ask = crate::events::ask(&ctx.state, cfg, vec![ask], None)?
        .pop()
        .context("ask not logged")?;
    Ok(writeln!(out, "{}", ask.str("id").unwrap_or_default())?)
}

pub(crate) fn is_draft(ask: &Event) -> bool {
    ask.kind == "needs-decision" && ask.data.get("scope").is_some()
}

/// The owner's answer `text` to draft `ask`: approve files exactly its title and body and queues it with its marks, drop files nothing; both end the scope. Any other text is a revise note (false: the lane gets it).
pub fn decide(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, ask: &Event, text: &str) -> Result<bool> {
    let (fleet, s) = (ask.fleet.as_str(), ask.val("scope"));
    let filed = match text {
        "approve" => {
            let f = cfg.fleet(fleet)?;
            let field = |k: &str| s.at(k).as_str().unwrap_or_default();
            let a = [
                "issue",
                "create",
                "-R",
                &f.repo,
                "--title",
                field("title"),
                "--body",
                field("body"),
            ];
            let url = crate::hygiene::gh(ctx, r, cfg, fleet, &a)?;
            let n: u64 =
                (url.trim().rsplit('/').next().and_then(|n| n.parse().ok())).context("gh issue create: no number")?;
            let mut op = crate::queue::op("add", &[n], false, None, None, None);
            for m in s.at("marks").as_array().into_iter().flatten().filter_map(Value::as_str) {
                op.put(m, json!(true));
            }
            writeln!(out, "filed {fleet}#{n}: {}", field("title"))?;
            writeln!(out, "{}", crate::queue_change(ctx, cfg, fleet, op, None)?)?;
            Some(n)
        }
        "drop" => None,
        _ => return Ok(false),
    };
    end(ctx, cfg, r, (fleet, s.at("req").as_str().unwrap_or_default()), filed)?;
    Ok(true)
}

/// The card's [drop]: request `req`, waiting or running, ends with nothing filed; a running one's lane closes.
pub fn drop_request(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str, req: &str) -> Result<String> {
    let ev = working(&ctx.state)?;
    if !open(&ev, fleet).iter().any(|q| q.str("req") == Some(req)) {
        return refused("NO_SUCH_REQUEST", format!("{fleet} has no open new issue {req}"));
    }
    end(ctx, cfg, r, (fleet, req), None)?;
    Ok(format!("{fleet}: dropped, nothing filed"))
}

fn end(ctx: &Ctx, cfg: &Config, r: &dyn Runner, (fleet, req): (&str, &str), filed: Option<u64>) -> Result<()> {
    let ev = working(&ctx.state)?;
    let running = open(&ev, fleet)
        .into_iter()
        .any(|q| q.str("req") == Some(req) && started(&ev, q));
    let outcome = if filed.is_some() { "filed" } else { "dropped" };
    let d = json!({"req": req, "outcome": outcome, "filed": filed});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, None, "scope-done", d))?;
    let Some(sp) = ev.iter().rfind(|e| e.is("spawn", fleet, LANE)).filter(|_| running) else {
        return Ok(());
    };
    if let Some(p) = sp.str("pane") {
        crate::term::close(ctx, r, p).ok();
    }
    let d = json!({"agent": sp.str("agent"), "pane": sp.str("pane"), "reason": format!("scope {outcome}")});
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, fleet, Some(LANE), "lane-killed", d),
    )
}
