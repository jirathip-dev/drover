use crate::config::Config;
use crate::devices::sha256;
use crate::events::{emit, read, Event};
use crate::gate::refused;
use crate::runner::{argv, Runner};
use crate::{orchestrator, Ctx};
use anyhow::Result;
use serde_json::json;

/// An approval's window: 15 min from the card's post.
pub const TTL: u64 = 900;

/// What a card asks to run: the command, why, what it touches, its folder and timeout (s).
pub struct Card<'a> {
    pub cmd: &'a str,
    pub why: &'a str,
    pub blast: &'a str,
    pub dir: &'a str,
    pub timeout: u64,
}

/// The hard no `cmd` is, even on an approved card, if any: a pattern guard over its lowercased words.
pub fn forbidden(cmd: &str) -> Option<&'static str> {
    let c = cmd.to_lowercase();
    let w: Vec<&str> = c
        .split(|ch: char| ch.is_whitespace() || "'\";|&()`".contains(ch))
        .filter(|w| !w.is_empty())
        .collect();
    let has = |x: &str| w.contains(&x);
    let any = |f: &dyn Fn(&str) -> bool| w.iter().any(|x| f(x));
    let secret = [
        "find-generic-password",
        "find-internet-password",
        "printenv",
        "tokens.env",
    ]
    .iter()
    .any(|x| has(x))
        || (has("gh") && has("auth") && has("token"))
        || (has("secret-tool") && has("lookup"))
        || any(&|x| x.contains('$') && ["token", "secret", "password", "key"].iter().any(|k| x.contains(k)));
    if secret {
        return Some("prints or sends a secret's value");
    }
    let prod = |x: &str| {
        x.split(|c: char| !c.is_ascii_alphanumeric())
            .any(|p| p == "prod" || p == "production")
    };
    let wipe = ["rm", "delete", "drop", "destroy", "truncate", "purge"]
        .iter()
        .any(|x| has(x));
    if (has("terraform") && has("destroy")) || c.contains("drop database") || (wipe && any(&prod)) {
        return Some("deletes production data");
    }
    let force = any(&|x| {
        ["-f", "-d", "--delete", "--mirror"].contains(&x) || x.starts_with("--force") || x.starts_with(['+', ':'])
    });
    if has("filter-branch") || has("filter-repo") || (has("git") && has("push") && force) {
        return Some("force-pushes or rewrites history");
    }
    let del = has("delete") || has("-xdelete") || has("--method=delete");
    if (del && any(&|x| x.contains("protection") || x.contains("rulesets")))
        || has("--owner-override")
        || has("--no-verify")
        || ((has("drover") || has("herdsman")) && has("timer") && has("remove"))
    {
        return Some("turns off branch protection or drover's gates");
    }
    None
}

/// `drover card post`: log card `r<n>` in `thread`; its id.
pub fn post(ctx: &Ctx, cfg: &Config, thread: &str, c: &Card) -> Result<String> {
    if let Some(w) = forbidden(c.cmd) {
        return refused("CARD_FORBIDDEN", format!("this command {w}; no run card may"));
    }
    if [c.cmd, c.why, c.blast, c.dir].iter().any(|s| s.trim().is_empty()) || c.timeout == 0 {
        return refused(
            "BAD_VALUE",
            "a run card needs --cmd, --why, --blast, --dir and a timeout",
        );
    }
    let events = read(&ctx.state)?;
    let t = orchestrator::find(&events, thread)?;
    let id = format!("r{}", events.iter().filter(|e| e.kind == "run-card").count() + 1);
    let dir = std::path::absolute(c.dir)?.display().to_string();
    let d = json!({"thread": thread, "id": id, "cmd": c.cmd, "why": c.why, "blast": c.blast, "dir": dir,
        "host": "main", "timeout": c.timeout, "sha": sha256(c.cmd.as_bytes())});
    emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "run-card", d))?;
    Ok(id)
}

/// Card `id`'s `run-settled`, if any.
pub fn settled<'a>(events: &'a [Event], id: &str) -> Option<&'a Event> {
    events
        .iter()
        .find(|e| e.kind == "run-settled" && e.str("id") == Some(id))
}

/// The owner's [approve] (`sha`: the prefix of the command's sha the owner saw, if the tap carries it) or [deny] on card `id`; the line the thread records.
pub fn settle(ctx: &Ctx, cfg: &Config, r: &dyn Runner, id: &str, approve: bool, sha: Option<&str>) -> Result<String> {
    settle_via(ctx, cfg, r, id, approve, sha, None)
}

/// #725: `settle`, its `run-settled` logged `via` the surface the tap came from (the lifeboat's `lifeboat`).
pub fn settle_via(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    id: &str,
    approve: bool,
    sha: Option<&str>,
    via: Option<&str>,
) -> Result<String> {
    let events = read(&ctx.state)?;
    let Some(c) = events.iter().find(|e| e.kind == "run-card" && e.str("id") == Some(id)) else {
        return refused("NO_SUCH_CARD", format!("no run card {id}"));
    };
    if let Some(s) = settled(&events, id) {
        let how = ["was denied", "ran already"][usize::from(s.val("approved") == true)];
        return refused("ALREADY_SETTLED", format!("{id} {how}; a new run needs a new card"));
    }
    let [thread, cmd, dir, sum] = ["thread", "cmd", "dir", "sha"].map(|k| c.str(k).unwrap_or_default());
    if approve && ctx.now > c.ts + TTL {
        return refused(
            "EXPIRED",
            format!("{id} expired 15 min after it was posted; post a new card"),
        );
    }
    if approve && (sha256(cmd.as_bytes()) != sum || sha.is_some_and(|s| s.is_empty() || !sum.starts_with(s))) {
        return refused(
            "CARD_CHANGED",
            format!("{id}'s command is not the one approved; post a new card"),
        );
    }
    if let Some(w) = forbidden(cmd).filter(|_| approve) {
        return refused("CARD_FORBIDDEN", format!("{id}'s command {w}"));
    }
    let mut d = json!({"thread": thread, "id": id, "sha": sum, "by": "owner", "approved": approve});
    if let Some(v) = via {
        d["via"] = json!(v);
    }
    emit(&ctx.state, cfg, Event::new(ctx.now, &c.fleet, None, "run-settled", d))?;
    let line = if approve {
        let log = ctx.state.with_file_name(format!("run-{id}.log"));
        r.detach(&ctx.exe, &argv(&["card", "run", id]), &log)?;
        format!("run {id} approved by owner, running in {dir}…")
    } else {
        format!("run {id} denied by owner; nothing ran")
    };
    let _ = orchestrator::say(ctx, cfg, r, (thread, &line, &[]), ("system", false));
    Ok(line)
}

/// #821: run approved card `id` once, detached, up to its --timeout; log and post its exit code and output tail.
pub fn run(ctx: &Ctx, cfg: &Config, r: &dyn Runner, id: &str) -> Result<String> {
    let events = read(&ctx.state)?;
    let Some(c) = events.iter().find(|e| e.kind == "run-card" && e.str("id") == Some(id)) else {
        return refused("NO_SUCH_CARD", format!("no run card {id}"));
    };
    if settled(&events, id).is_none_or(|s| s.val("approved") != true) {
        return refused("NOT_APPROVED", format!("{id} is not approved; nothing ran"));
    }
    let ran = |e: &Event| matches!(e.kind.as_str(), "run-started" | "run-done") && e.str("id") == Some(id);
    if events.iter().any(ran) {
        return refused("ALREADY_SETTLED", format!("{id} ran already; post a new card"));
    }
    let [thread, cmd, dir, sum] = ["thread", "cmd", "dir", "sha"].map(|k| c.str(k).unwrap_or_default());
    let (pid, start) = (std::process::id(), crate::now());
    let secs = c.val("timeout").as_u64().unwrap_or(600);
    let d = json!({"thread": thread, "id": id, "sha": sum, "pid": pid});
    emit(&ctx.state, cfg, Event::new(ctx.now, &c.fleet, None, "run-started", d))?;
    let out = ctx.state.with_file_name(format!("run-{id}.out"));
    let (sh, path) = ("exec >\"$3\" 2>&1; cd -- \"$1\" && eval \"$2\"", out.display());
    let args = argv(&["-c", sh, "sh", dir, cmd, &path.to_string()]);
    let o = r.run_for("sh", &args, "", std::time::Duration::from_secs(secs));
    let (code, mut note) = o.map_or_else(|e| (-1, format!("{e:#}")), |o| (o.code, o.stdout + &o.stderr));
    if code == 124 {
        note = format!("killed after {secs}s (its --timeout)");
    }
    let text = std::fs::read_to_string(&out).unwrap_or_default() + &note;
    let mut tail: Vec<String> = text.lines().rev().take(20).map(crate::lanes::redact).collect();
    tail.reverse();
    let tail = tail.join("\n");
    let now = ctx.now + crate::now().saturating_sub(start);
    let ctx = &Ctx { now, ..ctx.clone() };
    let d = json!({"thread": thread, "id": id, "sha": sum, "by": "owner", "code": code, "tail": tail});
    emit(&ctx.state, cfg, Event::new(ctx.now, &c.fleet, None, "run-done", d))?;
    let line = format!("run {id} approved by owner, ran in {dir}: exit {code}\n```\n{tail}\n```");
    let _ = orchestrator::say(ctx, cfg, r, (thread, &line, &[]), ("system", false));
    if code == 124 {
        let _ = r.run("kill", &argv(&["-KILL", "--", &format!("-{pid}")]));
    }
    Ok(line)
}

/// A card as the thread shows it: its command, why, blast radius and place, and how the owner settles it.
pub fn text(e: &Event) -> String {
    let s = |k: &str| e.str(k).unwrap_or_default();
    let id = s("id");
    format!(
        "run card {id}: `{}`\nwhy: {}\ntouches: {}\nruns on: {} host, {}\n[approve] drover card approve {id} --sha {} · [deny] drover card deny {id} (expires 15 min after posting)",
        s("cmd"),
        s("why"),
        s("blast"),
        s("host"),
        s("dir"),
        &s("sha")[..12.min(s("sha").len())]
    )
}
