use crate::config::Config;
use crate::events::{append, Event, IssueState, Json};
use crate::gh::{cache_path, cached_pr, store};
use crate::runner::{ok, Runner};
use crate::usage::load;
use crate::Ctx;
use anyhow::Result;
use serde_json::{json, Value};

pub const REVIEW: &str = "drover/review";
pub const VERIFY: &str = "drover/verify";
pub const OVERRIDE: &str = "drover/override";

/// A commit status drover posts itself: never a CI check.
pub fn ours(name: &str) -> bool {
    name.starts_with("drover/")
}

fn link(cfg: &Config, issue: u64) -> Option<String> {
    let host = cfg.serve.allowed_hosts.first()?;
    let port = cfg.serve.tailscale_https_port.filter(|p| *p != 443);
    Some(format!(
        "https://{host}{}/?issue={issue}#queue",
        port.map_or(String::new(), |p| format!(":{p}"))
    ))
}

fn send(r: &dyn Runner, repo: &str, w: &Value) -> Result<()> {
    let s = |k: &str| w.at(k).as_str().unwrap_or_default();
    let desc: String = s("description").chars().take(140).collect();
    let mut a = vec![
        "api".to_string(),
        "-X".into(),
        "POST".into(),
        format!("repos/{repo}/statuses/{}", s("sha")),
        "-f".into(),
        format!("state={}", s("state")),
        "-f".into(),
        format!("context={}", s("context")),
        "-f".into(),
        format!("description={desc}"),
    ];
    if let Some(u) = w.at("url").as_str() {
        a.extend(["-f".into(), format!("target_url={u}")]);
    }
    ok(r, "gh", &a).map(|_| ())
}

fn put(ctx: &Ctx, cfg: &Config, r: &dyn Runner, (fleet, all): (&str, &mut Value), mut w: Value) -> Result<()> {
    let key = format!(
        "{} {}",
        w.at("sha").as_str().unwrap_or_default(),
        w.at("context").as_str().unwrap_or_default()
    );
    let failing = all.at(&key).at("error").is_string();
    if let Err(e) = send(r, &cfg.fleet(fleet)?.repo, &w) {
        let err = format!("{e:#}");
        w.put("error", json!(err));
        if !failing {
            let mut d = w.clone();
            d.put("reason", json!(format!("commit status {key} not posted: {err}")));
            let issue = w.at("n").as_u64();
            append(
                &ctx.state,
                &Event::new(ctx.now, fleet, issue, "status-failed", d).passive(),
            )?;
        }
    }
    all.put(&key, w);
    Ok(())
}

/// Post `context` as `state` with `desc` at `sha` of `fleet#issue`; a failed post is logged once and kept for `sync` to retry.
pub fn post(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    (fleet, issue): (&str, u64),
    sha: &str,
    (context, state, desc): (&str, &str, &str),
) -> Result<()> {
    let p = cache_path(ctx, "status", fleet, 0);
    let mut all = load::<Value>(&p).unwrap_or_else(|| json!({}));
    let w = json!({"n": issue, "sha": sha, "context": context, "state": state, "description": desc, "url": link(cfg, issue)});
    put(ctx, cfg, r, (fleet, &mut all), w)?;
    store(&p, &all)
}

/// Log verdict event `ev`, then show it at its sha: `drover/verify` for verify's, else `drover/review`, with its round and blocking count.
pub fn verdict(ctx: &Ctx, cfg: &Config, r: &dyn Runner, ev: Event) -> Result<()> {
    crate::events::emit(&ctx.state, cfg, ev.clone())?;
    let s = |k: &str| ev.str(k).unwrap_or_default();
    let (sha, pass) = (s("sha"), s("verdict") == "PASS");
    let blocking = crate::gate::findings(s("body")).iter().filter(|f| f.0).count();
    let tail = if pass {
        String::new()
    } else {
        format!(" · {blocking} blocking")
    };
    let desc = format!("{} · round {} · {sha:.7}{tail}", s("verdict"), ev.val("round"));
    let context = if s("stage") == "verify" { VERIFY } else { REVIEW };
    let outcome = if pass { "success" } else { "failure" };
    post(
        ctx,
        cfg,
        r,
        (&ev.fleet, ev.issue.unwrap_or_default()),
        sha,
        (context, outcome, &desc),
    )
}

/// The full drive pass: an open PR head with no verdict gets `drover/review` pending, each failed post is tried again, and a finished issue's posted ones are dropped.
pub fn sync(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn std::io::Write, fleet: &str) -> Result<()> {
    let open = crate::events::fold(&crate::events::working(&ctx.state)?);
    if let Err(e) = pending(ctx, cfg, r, fleet, open.values()) {
        writeln!(out, "{fleet} commit status: {e:#}")?;
    }
    Ok(())
}

fn pending<'a>(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    fleet: &str,
    open: impl Iterator<Item = &'a IssueState>,
) -> Result<()> {
    let p = cache_path(ctx, "status", fleet, 0);
    let mut all = load::<Value>(&p).unwrap_or_else(|| json!({}));
    let failed: Vec<Value> = all
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
        .filter(|w| w.at("error").is_string())
        .cloned()
        .collect();
    for mut w in failed {
        if let Some(m) = w.as_object_mut() {
            m.remove("error");
        }
        put(ctx, cfg, r, (fleet, &mut all), w)?;
    }
    let mut live = vec![];
    for s in open.filter(|s| s.fleet == fleet && !s.done()) {
        live.push(json!(s.issue));
        let pr = cached_pr(ctx, fleet, s.issue);
        let head = pr.at("headRefOid").as_str().unwrap_or_default().to_ascii_lowercase();
        let seen = all.at(format!("{head} {REVIEW}")).is_object();
        if pr.at("state") != "OPEN" || head.is_empty() || seen || s.reviewed(&head) {
            continue;
        }
        let desc = format!("awaiting review of {head:.7}");
        let w = json!({"n": s.issue, "sha": head, "context": REVIEW, "state": "pending", "description": desc,
            "url": link(cfg, s.issue)});
        put(ctx, cfg, r, (fleet, &mut all), w)?;
    }
    if let Some(m) = all.as_object_mut() {
        m.retain(|_, w| w.at("error").is_string() || live.contains(w.at("n")));
    }
    store(&p, &all)
}

/// Doctor's rows: each commit status that failed to post, with the fix.
pub fn doctor(ctx: &Ctx, cfg: &Config) -> Vec<String> {
    let fix = "gh auth refresh -s repo (a fine-grained token: Commit statuses: write)";
    cfg.fleet
        .keys()
        .filter_map(|n| load::<Value>(&cache_path(ctx, "status", n, 0)))
        .flat_map(|all| all.as_object().cloned().unwrap_or_default().into_values())
        .filter_map(|w| {
            let (s, sha) = (
                w.at("error").as_str()?.to_string(),
                w.at("sha").as_str().unwrap_or_default(),
            );
            Some(format!(
                "✗ commit status {} at {sha:.7} (#{}) not posted, retried each pass: {s} → {fix}",
                w.at("context").as_str().unwrap_or_default(),
                w.at("n")
            ))
        })
        .collect()
}
