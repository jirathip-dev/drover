use crate::devices::write0600;
use crate::events::{finding, plain, working, Event};
use anyhow::{bail, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use web_push_native::jwt_simple::algorithms::{ECDSAP256PublicKeyLike, ES256KeyPair};
use web_push_native::p256::PublicKey;
use web_push_native::{Auth, WebPushBuilder};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Sub {
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
    pub label: String,
    pub created: u64,
    #[serde(default)]
    pub words: BTreeMap<String, String>,
}

impl Sub {
    /// What the page shows and removes by: the endpoint itself never leaves the Mac.
    pub fn id(&self) -> String {
        crate::devices::sha256(self.endpoint.as_bytes())[..16].into()
    }
}

fn dir(log: &Path) -> PathBuf {
    log.parent().unwrap_or(log).join("webpush")
}

/// The `push` rows of the database.
pub fn load(log: &Path) -> Vec<Sub> {
    crate::db::list(log, "push")
}

fn store(log: &Path, s: &[Sub]) -> Result<()> {
    crate::db::store(log, "push", s)
}

fn b64(s: &str) -> Result<Vec<u8>> {
    Ok(B64.decode(s.trim_end_matches('='))?)
}

/// The uncompressed public point (65 bytes).
pub fn public(k: &ES256KeyPair) -> Vec<u8> {
    k.public_key().public_key().to_bytes_uncompressed()
}

/// The VAPID key, made on first use: `vapid.key`, 0600, the base64url scalar.
pub fn vapid(log: &Path) -> Result<ES256KeyPair> {
    let p = dir(log).join("vapid.key");
    if p.exists() {
        let raw = b64(std::fs::read_to_string(&p)?.trim())?;
        if raw.len() != 32 {
            bail!("{}: not a P-256 scalar", p.display());
        }
        return ES256KeyPair::from_bytes(&raw);
    }
    let k = ES256KeyPair::generate();
    write0600(&p, B64.encode(k.to_bytes()).as_bytes())?;
    Ok(k)
}

fn auth(s: &str) -> Result<Auth> {
    Auth::from_exact_iter(b64(s)?).ok_or_else(|| anyhow::anyhow!("auth must be 16 bytes"))
}

/// What a subscribe says with no webpush notifier: the device is kept, nothing is sent.
pub const OFF: &str = "alerts are saved but off: add `[[notify]] type = \"webpush\"` to fleet.toml (docs/serve.md)";

/// `POST /push/subscribe` (endpoint, p256dh, auth; `words` JSON, `quiet`: no push): add or replace this endpoint's subscription, then push "alerts are on" to it alone; with no webpush notifier (`sub`), say the alerts are saved but off.
pub fn subscribe(log: &Path, f: &BTreeMap<String, String>, label: &str, now: u64, sub: Option<&str>) -> Result<String> {
    let g = |k: &str| f.get(k).cloned().unwrap_or_default();
    let s = Sub {
        endpoint: g("endpoint"),
        p256dh: g("p256dh"),
        auth: g("auth"),
        label: label.into(),
        created: now,
        words: serde_json::from_str(&g("words")).unwrap_or_default(),
    };
    if !s.endpoint.starts_with("https://") || s.endpoint.chars().any(char::is_whitespace) {
        bail!("endpoint must be an https:// URL");
    }
    if PublicKey::from_sec1_bytes(&b64(&s.p256dh)?).is_err() || auth(&s.auth).is_err() {
        bail!("bad p256dh or auth key");
    }
    let mut all = load(log);
    all.retain(|x| x.endpoint != s.endpoint);
    all.push(s);
    store(log, &all)?;
    let sub = sub.filter(|_| !f.contains_key("quiet"));
    let Some(sub) = sub else { return Ok(OFF.into()) };
    let ev = Event::new(
        now,
        "drover",
        None,
        "push-test",
        serde_json::json!({"reason": "alerts are on"}),
    );
    let sent = send(log, sub, &ev, Some(&g("endpoint")));
    Ok(sent.map_or_else(
        |e| format!("alerts on; the test push failed: {e:#}"),
        |_| "alerts on".into(),
    ))
}

/// Drop the subscriptions with these ids.
pub fn remove(log: &Path, ids: &[String]) -> Result<usize> {
    let mut all = load(log);
    let n = all.len();
    all.retain(|s| !ids.contains(&s.id()));
    store(log, &all)?;
    Ok(n - all.len())
}

fn post(s: &Sub, k: &ES256KeyPair, sub: &str, payload: &str) -> Result<u16> {
    let ua = PublicKey::from_sec1_bytes(&b64(&s.p256dh)?)?;
    let req = WebPushBuilder::new(s.endpoint.parse()?, ua, auth(&s.auth)?)
        .with_vapid(k, sub)
        .build(payload)?;
    let heads: Vec<String> = req
        .headers()
        .iter()
        .map(|(n, v)| format!("{n}: {}", v.to_str().unwrap_or_default()))
        .collect();
    let mut args = vec![
        "-sS",
        "--max-time",
        "10",
        "-X",
        "POST",
        "-o",
        "/dev/null",
        "-w",
        "%{http_code}",
    ];
    args.extend(["--data-binary", "@-"]);
    for h in &heads {
        args.extend(["-H", h]);
    }
    args.push(&s.endpoint);
    let o = crate::runner::Real.within(
        "curl",
        &crate::runner::argv(&args),
        req.body(),
        std::time::Duration::from_secs(crate::config::limits().notify_secs),
    )?;
    Ok(o.stdout.trim().parse().unwrap_or(0))
}

/// Push `ev` once to every subscription, or only to endpoint `only` (no retry): a 404/410 prunes it, any other failure is the error. A delivery logs one passive `push-sent` with the device count.
pub fn send(log: &Path, sub: &str, ev: &Event, only: Option<&str>) -> Result<String> {
    let all: Vec<Sub> = load(log)
        .into_iter()
        .filter(|s| only.is_none_or(|e| s.endpoint == e))
        .collect();
    if all.is_empty() {
        return Ok("no device has alerts on".into());
    }
    let k = vapid(log)?;
    let tab = if matches!(ev.kind.as_str(), "stall" | "lane-dead") {
        "lanes"
    } else {
        "queue"
    };
    let mut url = ev.issue.map_or(String::new(), |n| format!("?issue={n}")) + "#" + tab;
    let n = ev.issue.filter(|_| ev.kind == "needs-human");
    let kinds = ["thread-msg", "needs-decision", "ask-owner"];
    let thread = ev.str("thread").filter(|_| kinds.contains(&ev.kind.as_str()));
    let why = ev.str("reason").unwrap_or("?");
    let evs = if n.is_some() || thread.is_some() {
        working(log)?
    } else {
        Arc::default()
    };
    let first = n.and_then(|n| finding(&evs, &ev.fleet, n)).unwrap_or("");
    let (title, mut body) = match n {
        Some(n) => (format!("#{n} needs you: {why}"), format!("{why}\n{first}")),
        None => ("drover".into(), plain(ev)),
    };
    if let Some(t) = thread.and_then(|t| crate::orchestrator::find(&evs, t).ok()) {
        let repo = if t.fleet == "all" { "all repos" } else { &t.fleet };
        let head = format!(
            "{repo} · {}",
            crate::orchestrator::title(&evs, t.str("thread").unwrap_or_default())
        );
        let text = ev.str("text").and_then(|x| x.lines().find(|l| !l.trim().is_empty()));
        body = match ev.str("question") {
            Some(q) => format!("{head} asks: {q}"),
            None => format!("{head}: {}", text.unwrap_or_default()),
        };
        url = format!("#threads/{}", t.str("thread").unwrap_or_default());
    }
    let (wk, said) = match (ev.kind.as_str(), ev.issue) {
        ("merged", Some(n)) => ("merged", format!("#{n} merged")),
        ("needs-human" | "needs-decision", Some(n)) => ("needs", format!("{} #{n} needs you", ev.fleet)),
        _ => ("", String::new()),
    };
    let pay = |t: &str| serde_json::json!({"title": t, "body": body.trim(), "url": format!("/{url}")}).to_string();
    let (mut sent, mut gone, mut errs) = (0, vec![], vec![]);
    for s in &all {
        let payload = pay(&s.words.get(wk).map_or(title.clone(), |w| format!("{w}: {said}")));
        match post(s, &k, sub, &payload) {
            Ok(200..=299) => sent += 1,
            Ok(404 | 410) => gone.push(s.id()),
            Ok(code) => errs.push(format!("{}: HTTP {code}", s.label)),
            Err(e) => errs.push(format!("{}: {e:#}", s.label)),
        }
    }
    if !gone.is_empty() {
        remove(log, &gone)?;
    }
    if sent > 0 {
        let d = serde_json::json!({"devices": sent});
        crate::events::append(log, &Event::new(ev.ts, &ev.fleet, None, "push-sent", d))?;
    }
    if !errs.is_empty() {
        bail!("webpush: {}", errs.join("; "));
    }
    Ok(format!("sent to {sent} device(s), pruned {}", gone.len()))
}
