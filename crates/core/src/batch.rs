use crate::config::{Check, Config, Fleet};
use crate::drive::Lock;
use crate::events::{emit, fold, working, Event, Json};
use crate::gate::{refuse, Refusal};
use crate::overlap::git;
use crate::runner::{json, Runner};
use crate::Ctx;
use anyhow::Result;
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;

/// A PR that passed every merge gate at `head`: what `gate::eligible` returns.
#[derive(Clone)]
pub struct Ready {
    pub issue: u64,
    pub pr: Value,
    pub num: String,
    pub head: String,
    pub fast: bool,
    pub rounds: u32,
    pub passed_at: u64,
}

struct Layer {
    member: Ready,
    commit: String,
    tree: String,
}

struct Stack {
    wt: PathBuf,
    base: String,
    layers: Vec<Layer>,
}

enum Checked {
    Green(Option<String>),
    Culprit { at: usize, check: Check, output: String },
}

fn serial(events: &[Event], f: &Fleet, fleet: &str, n: u64) -> Option<String> {
    let q = crate::queue::replay(events, fleet);
    let gs = crate::queue::groups(&q, events, f, fleet, n);
    gs.into_iter().find(|g| !crate::queue::hot(g)).map(|g| g.name)
}

pub(crate) fn plan(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    events: &[Event],
    fleet: &str,
    lead: &Ready,
) -> Result<Vec<Ready>> {
    let f = cfg.fleet(fleet)?;
    if f.riders() == 0 {
        return Ok(vec![]);
    }
    let mut sts = fold(events);
    let strict = || {
        crate::gh::update_ready(
            ctx,
            Some(r),
            &Fleet {
                update_ready: None,
                ..f.clone()
            },
            fleet,
        )
    };
    let alone = match sts.get(&(fleet.into(), lead.issue)) {
        s if s.is_none_or(|s| s.state != "ready") => Some("it is not ready".into()),
        _ => serial(events, f, fleet, lead.issue)
            .map(|g| format!("serial group {g}"))
            .or_else(|| strict().then(|| format!("{}'s branch rule is strict", f.integration))),
    };
    if let Some(why) = alone {
        writeln!(out, "batch: #{} merges alone: {why}", lead.issue)?;
        return Ok(vec![]);
    }
    let ids: Vec<u64> = (sts.values())
        .filter(|s| s.fleet == fleet && s.state == "ready" && s.issue != lead.issue)
        .map(|s| s.issue)
        .collect();
    let mut riders = vec![];
    for n in ids {
        if serial(events, f, fleet, n).is_some() || crate::gate::unlogged_merge(events, fleet, n).is_some() {
            continue;
        }
        let st = sts.remove(&(fleet.into(), n));
        if let Ok((_, Ok(m))) = crate::gate::eligible(ctx, r, f, events, (fleet, n), st) {
            riders.push(m);
        }
    }
    riders.sort_by_key(|m| (m.passed_at, m.issue));
    riders.truncate(f.riders());
    Ok(riders)
}

pub(crate) fn run(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, lock): (&str, &Lock),
    lead: Ready,
    riders: Vec<Ready>,
) -> Result<Result<(), Refusal>> {
    let f = cfg.fleet(fleet)?;
    let checks = crate::gate::premerge_checks(f);
    let rules = std::fs::read_to_string(crate::repo_path(&f.local_path().join(".drover/rules.md"))).unwrap_or_default();
    if riders.is_empty() && checks.is_empty() && crate::lesson::checks(&rules).iter().all(|c| c.active) {
        return crate::gate::land_one(ctx, cfg, r, out, fleet, &lead, (None, None)).map(Ok);
    }
    let (dir, into) = (f.local_path(), format!("origin/{}", f.integration));
    let tip = || crate::clone::fetch_branches(ctx, r, &dir).and_then(|_| git(r, &dir, &["rev-parse", &into]));
    let members: Vec<Ready> = std::iter::once(lead).chain(riders).collect();
    for _ in 0..2 {
        let base = tip()?;
        let s = match build(ctx, cfg, r, (fleet, &dir, &into), base, &members)? {
            Ok(v) => v,
            Err(rf) => return Ok(Err(rf)),
        };
        let id = s.layers.last().map(|l| l.tree.clone()).unwrap_or_default();
        let t = crate::clock::instant();
        let checked = check(ctx, cfg, r, (fleet, lock, &id), &s, &checks)?;
        let secs = t.elapsed().as_secs();
        let (k, culprit, outcome) = match checked {
            Checked::Green(flaky) => {
                crate::lesson::record(ctx, r, &s.wt, (fleet, s.layers.first().map_or(0, |l| l.member.issue)))?;
                if tip()? != s.base {
                    continue;
                }
                let n = s.layers.len();
                (n, None, if flaky.is_some() { "flaky" } else { "landed" })
            }
            Checked::Culprit { at, check, output } => (at, Some((check, output)), "charged"),
        };
        let (landed, stop) = land(ctx, cfg, r, out, fleet, (&s, &id), k, secs)?;
        let blamed = culprit.as_ref().and(s.layers.get(k)).map(|l| l.member.issue);
        let left: Vec<u64> = (s.layers.iter().map(|l| l.member.issue))
            .filter(|n| !landed.contains(n) && Some(*n) != blamed)
            .collect();
        let outcome = if stop.is_some() { "stopped" } else { outcome };
        let d =
            json!({"batch": id, "outcome": outcome, "landed": landed, "left": left, "culprit": blamed, "why": stop});
        log(ctx, cfg, (fleet, s.layers.len()), "batch-landed", d)?;
        let (Some((c, o)), Some(l)) = (culprit, s.layers.get(k)) else {
            return Ok(Ok(()));
        };
        charge(ctx, cfg, r, out, fleet, (&s, k, secs), &l.member, (&c, &o))?;
        return Ok(Ok(()));
    }
    Ok(Err(refuse(
        "STAGING_MOVED",
        format!("{into} moved during the premerge checks twice"),
    )))
}

fn build(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    (fleet, dir, into): (&str, &std::path::Path, &str),
    base: String,
    members: &[Ready],
) -> Result<Result<Stack, Refusal>> {
    let wt = ctx.state.with_file_name(format!("premerge-{fleet}"));
    crate::gate::scratch(r, dir, &wt, &base)?;
    let mut s = Stack {
        wt,
        base,
        layers: vec![],
    };
    let mut dropped = vec![];
    for m in members {
        if git(r, &s.wt, &["merge", "--no-ff", "--no-edit", &m.head]).is_err() {
            if s.layers.is_empty() {
                return Ok(Err(refuse(
                    "NOT_MERGEABLE",
                    format!("{:.7} conflicts with {into}", m.head),
                )));
            }
            git(r, &s.wt, &["merge", "--abort"]).ok();
            dropped.push(json!({"issue": m.issue, "why": "conflict"}));
            continue;
        }
        let o = git(r, &s.wt, &["rev-parse", "HEAD", "HEAD^{tree}"])?;
        let (commit, tree) = o.split_once('\n').unwrap_or_default();
        let (member, commit, tree) = (m.clone(), commit.trim().into(), tree.trim().into());
        s.layers.push(Layer { member, commit, tree });
    }
    let id = s.layers.last().map(|l| l.tree.clone()).unwrap_or_default();
    let ms: Vec<Value> = (s.layers.iter().map(|l| &l.member))
        .map(|m| json!({"issue": m.issue, "pr": m.num, "head": m.head}))
        .collect();
    let d = json!({"batch": id, "base": s.base, "lead": members.first().map(|m| m.issue), "members": ms, "dropped": dropped});
    log(ctx, cfg, (fleet, s.layers.len()), "batch-planned", d)?;
    Ok(Ok(s))
}

fn charge(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    fleet: &str,
    (s, k, secs): (&Stack, usize, u64),
    m: &Ready,
    (c, o): (&Check, &str),
) -> Result<()> {
    let into = format!("origin/{}", cfg.fleet(fleet)?.integration);
    let under: Vec<String> = (s.layers.iter().take(k))
        .map(|l| format!("#{}", l.member.issue))
        .collect();
    let on = if under.is_empty() {
        String::new()
    } else {
        format!(" on {}", under.join(", "))
    };
    let at = format!("{:.7} merged with {into} @ {:.7}{on}, {secs}s", m.head, s.base);
    let rf = crate::gate::premerge_failed(ctx, cfg, r, out, (fleet, m.issue), (&m.head, &at), (c, o))?;
    if k == 0 {
        return Err(rf.into());
    }
    Ok(())
}

fn log(ctx: &Ctx, cfg: &Config, (fleet, n): (&str, usize), kind: &str, d: Value) -> Result<()> {
    if n < 2 {
        return Ok(());
    }
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, None, kind, d).passive())
}

fn check(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    (fleet, lock, id): (&str, &Lock, &str),
    s: &Stack,
    checks: &[Check],
) -> Result<Checked> {
    let (n, heavy, events) = (
        s.layers.len(),
        crate::heavy::Heavy::at(cfg, cfg.fleet(fleet)?, None),
        working(&ctx.state)?,
    );
    let green = |t: &str| {
        (events.iter()).any(|e| e.kind == "batch-checked" && e.str("tree") == Some(t) && e.val("green") == &json!(true))
    };
    let run = |k: usize| -> Result<Option<(Check, String)>> {
        let Some(l) = s.layers.get(k.saturating_sub(1)).filter(|l| n < 2 || !green(&l.tree)) else {
            return Ok(None);
        };
        if n > 1 {
            git(r, &s.wt, &["reset", "-q", "--hard", &l.commit])?;
        }
        lock.renew(ctx, cfg.limits.gate_check_minutes * 60);
        let t = crate::clock::instant();
        let fail = crate::gate::run_checks(r, (&heavy, &ctx.exe), &s.wt, &s.base, checks).map(|(c, o)| (c.clone(), o));
        let name = fail.as_ref().map(|f| f.0.name.clone());
        let d = json!({"batch": id, "tree": l.tree, "upto": k, "green": fail.is_none(), "secs": t.elapsed().as_secs(), "check": name});
        log(ctx, cfg, (fleet, n), "batch-checked", d)?;
        Ok(fail)
    };
    let Some(first) = run(n)? else {
        return Ok(Checked::Green(None));
    };
    let red = if n < 2 { Some(first.clone()) } else { run(n)? };
    let Some(mut red) = red else {
        return Ok(Checked::Green(Some(first.0.name)));
    };
    let (mut lo, mut hi) = (0, n);
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        match run(mid)? {
            Some(f) => (hi, red) = (mid, f),
            None => lo = mid,
        }
    }
    Ok(Checked::Culprit {
        at: hi - 1,
        check: red.0,
        output: red.1,
    })
}

fn land(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    fleet: &str,
    (s, id): (&Stack, &str),
    k: usize,
    secs: u64,
) -> Result<(Vec<u64>, Option<&'static str>)> {
    let f = cfg.fleet(fleet)?;
    let (dir, into) = (f.local_path(), format!("origin/{}", f.integration));
    let tip = || crate::clone::fetch_branches(ctx, r, &dir).and_then(|_| git(r, &dir, &["rev-parse", &into]));
    let batch = (s.layers.len() > 1).then_some(id);
    let (mut landed, mut last) = (vec![], None);
    for (i, l) in s.layers.iter().take(k).enumerate() {
        let m = &l.member;
        if let Some(why) = last
            .as_ref()
            .map(|t| settled(r, f, m, tip()? == *t))
            .transpose()?
            .flatten()
        {
            return Ok((landed, Some(why)));
        }
        let res = crate::gate::land_one(ctx, cfg, r, out, fleet, m, ((i == 0).then_some(secs), batch));
        match res {
            Err(e) if i == 0 => return Err(e),
            Err(e) => {
                writeln!(out, "batch: #{} did not land: {e:#}", m.issue)?;
                return Ok((landed, Some("refused")));
            }
            Ok(()) => landed.push(m.issue),
        }
        if batch.is_none() {
            break;
        }
        let t = tip()?;
        if git(r, &dir, &["rev-parse", &format!("{t}^{{tree}}")])? != l.tree {
            return Ok((landed, Some("tree differs")));
        }
        last = Some(t);
    }
    Ok((landed, None))
}

fn settled(r: &dyn Runner, f: &Fleet, m: &Ready, tip_same: bool) -> Result<Option<&'static str>> {
    if !tip_same {
        return Ok(Some("tip moved"));
    }
    for _ in 0..=crate::config::limits().batch_settle_secs / 5 {
        let v = json(
            r,
            "gh",
            &["pr", "view", &m.num, "-R", &f.repo, "--json", "headRefOid,mergeable"],
        )?;
        if v.at("headRefOid") != m.head.as_str() {
            return Ok(Some("head moved"));
        }
        if v.at("mergeable") == "MERGEABLE" {
            return Ok(None);
        }
        r.sleep(5);
    }
    Ok(Some("GitHub not settled"))
}
