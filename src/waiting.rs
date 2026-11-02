//! #46: what an idle lane waits on (drive | owner | ci | none), worked out from the log, git and the cached PR.
use crate::config::{Config, Fleet};
use crate::events::{age, fold, last_spawn, open_asks, Event, IssueState};
use crate::runner::{ok, Runner};
use crate::{gh, Ctx};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Waiting {
    pub fleet: String,
    pub issue: u64,
    /// The lane this row is for; every lane of an issue gets one.
    pub lane: String,
    /// drive | owner | ci | none
    pub who: &'static str,
    pub what: String,
    pub since: u64,
    pub next_cmd: Option<String>,
}

impl Waiting {
    /// `idle · <what> (<age>)`
    pub fn label(&self, now: u64) -> String {
        format!("idle · {} ({})", self.what, age(now.saturating_sub(self.since)))
    }
    /// Page tone: owner red, drive amber past 5 min, an impl idle without pushing amber at once, the rest dim.
    pub fn tone(&self, now: u64) -> &'static str {
        match self.who {
            "owner" => "r",
            "drive" if now.saturating_sub(self.since) > 300 => "a",
            "none" if self.what.contains("idle without") => "a",
            _ => "m",
        }
    }
}

/// Pure: the issue's wait reason. `head` = the lane worktree's (sha, commit time); `pr` = the (cached) PR view;
/// `rev` = the rev worktree's verdict.md (absolute path, first line) when written since the rev spawn.
/// #173: each next step is the one `fc`'s pipeline maps the outcome to.
pub fn waiting_on(
    events: &[Event],
    s: &IssueState,
    head: Option<(&str, u64)>,
    pr: &Value,
    fc: &Fleet,
    rev: Option<(&str, &str)>,
) -> Waiting {
    let (f, n, req, p) = (&s.fleet, s.issue, &fc.required_checks, fc.flow());
    // The role an outcome of `stage` goes to (a build for a fix, a check for a review).
    let to = |stage: &str, o: &str| {
        p.next(stage, o)
            .and_then(|t| p.role(t.0))
            .unwrap_or(p.start())
            .to_string()
    };
    let build = p.stage_of(s.build.as_deref().unwrap_or(p.start()));
    let passed = s.verdicts.last().filter(|v| v.1 == "PASS").map(|v| v.2.as_str());
    let then = passed.and_then(|c| p.next(c, "PASS")).map(|t| t.0);
    let next_check = then.filter(|t| p.get(t).is_some_and(|c| c.kind == "check"));
    let gate = then
        .filter(|t| p.get(t).is_some_and(|g| g.kind == "gate"))
        .unwrap_or(p.gate());
    let lane = s.lane.clone().unwrap_or_default();
    let w = |who, what: &str, since, next: Option<String>| Waiting {
        fleet: f.clone(),
        issue: n,
        lane: lane.clone(),
        who,
        what: what.into(),
        since,
        next_cmd: next,
    };
    let cmd = |verb: &str, tail: &str| Some(format!("herdsman {verb} {f} {n}{tail}"));
    let mine =
        |a: &&Event| a.fleet == *f && (a.issue == Some(n) || s.lanes.values().any(|l| a.str("agent") == Some(l)));
    if let Some(a) = open_asks(events).into_iter().find(mine) {
        return w(
            "owner",
            &format!("waiting on your answer ({})", a.str("id").unwrap_or("?")),
            a.ts,
            None,
        );
    }
    if s.state == "merged" || pr["state"] == "MERGED" {
        return w("none", "nothing to do (merged): clean me", s.last_ts, None);
    }
    if s.state == "closed" || pr["state"] == "CLOSED" {
        return w("none", "nothing to do (closed): clean me", s.last_ts, None);
    }
    // #104: an impl/fix/review lane whose account capped since its spawn respawns on a free one (`kill` pushes `wip/`).
    let active = matches!(s.state.split('(').next(), Some("impl" | "fix" | "review"));
    let spawned = last_spawn(events, &lane).filter(|_| active);
    let since_spawn = |e: &&Event| spawned.is_some_and(|sp| e.ts >= sp.ts && e.str("account") == sp.str("account"));
    if let Some(c) = events.iter().filter(since_spawn).find(|e| e.is("account-capped", f, n)) {
        let role = spawned.and_then(|sp| sp.str("role")).unwrap_or_default();
        let what = "account capped, waiting for drive to respawn on a free one";
        return w("drive", what, c.ts, cmd("spawn", &format!(" {role} --supersede")));
    }
    let pushed = pr["headRefOid"].as_str().unwrap_or_default();
    // #129: an auto-update moves the head without new work, so a review of the head it updated still stands.
    let mut base = pushed;
    let update = |to: &str| {
        events
            .iter()
            .find(|e| e.is("auto-updated", f, n) && e.str("to") == Some(to))
    };
    while let Some(e) = update(base) {
        base = e.str("from").unwrap_or_default();
    }
    let handed = |h: &str| events.iter().any(|e| e.is("pushed", f, n) && e.str("sha") == Some(h));
    // #102: a PASSed PR conflicting with / behind integration gets an update lane; its pushed head is reviewed first.
    let how = match (pr["mergeable"].as_str(), pr["mergeStateStatus"].as_str()) {
        (Some("CONFLICTING"), _) => "conflicts with",
        (_, Some("BEHIND")) => "is behind",
        _ => "",
    };
    // #148: a PASSed item blocked by CI, mergeability or host load is retried as ready on every pass.
    let block = events
        .iter()
        .rev()
        .find(|e| e.is("blocked", f, n))
        .and_then(|e| e.str("code"));
    let retry = s.state == "blocked" && s.verdicts.last().is_some_and(|v| v.1 == "PASS");
    let st = match block {
        Some("CI_NOT_GREEN" | "NOT_MERGEABLE" | "HOST_BUSY") if retry => "ready",
        // #165: a head with no verdict of its own (a non-pure merge) gets a review round, not a block.
        Some("STALE_VERDICT") if s.state == "blocked" => "stale",
        _ => s.state.split('(').next().unwrap_or_default(),
    };
    match st {
        // #173: a PASS that goes on to another check: that check reviews the same head.
        "ready" if next_check.is_some() => {
            let c = next_check.unwrap_or_default();
            let what = format!("{} PASS, waiting for drive to spawn {c}", passed.unwrap_or_default());
            w(
                "drive",
                &what,
                s.last_ts,
                cmd("spawn", &format!(" {}", p.role(c).unwrap_or(c))),
            )
        }
        "ready" if !how.is_empty() => {
            let what = format!(
                "conflict: PR #{} {how} {}",
                pr["number"],
                pr["baseRefName"].as_str().unwrap_or("?")
            );
            let b = to(gate, "conflict");
            w(
                "drive",
                &what,
                s.last_ts,
                cmd("spawn", &format!(" {b} --supersede --update")),
            )
        }
        "ready" if gh::ci(&gh::required(&pr["statusCheckRollup"], req)) == "green" => {
            w("drive", "PASS + green, waiting for merge", s.last_ts, cmd("merge", ""))
        }
        "ready" => w("ci", "waiting for CI", s.last_ts, None),
        "stale" => w(
            "drive",
            "no verdict for the PR head: review again",
            s.last_ts,
            cmd("spawn", &format!(" {} --supersede", to(build, "push"))),
        ),
        // The exact command, or none (no nudge) while the verdict word or the head sha is unknown.
        "review" => match rev {
            // #92: a verdict is for the head the rev lane was spawned at; a moved PR head gets a fresh reviewer.
            Some(_) if head.is_some_and(|h| !pushed.is_empty() && h.0 != base) => {
                let what = format!(
                    "verdict is for {:.7}, the PR head moved to {pushed:.7}: review again",
                    head.unwrap_or_default().0
                );
                let c = s.check.as_deref().unwrap_or("rev");
                w("drive", &what, s.last_ts, cmd("spawn", &format!(" {c} --supersede")))
            }
            Some((path, v)) => {
                let at = head.is_some_and(|h| h.0 == base);
                let next = (matches!(v, "PASS" | "FAIL") && at)
                    .then(|| format!("herdsman verdict {f} {n} {v} --sha {pushed} --file {path}"));
                w(
                    "drive",
                    "verdict written, waiting for drive to record it",
                    s.last_ts,
                    next,
                )
            }
            None => w("none", "rev idle without a verdict (check the pane)", s.last_ts, None),
        },
        "fix" if s.last_event == "verdict" => {
            // The FAIL's stage: a check's FAIL, or a gate's CI red (logs before #173 name none: `rev`).
            let v = events.iter().rev().find(|e| e.is("verdict", f, n));
            let by = v.and_then(|e| e.str("stage")).unwrap_or("rev");
            let o = if p.get(by).is_some_and(|g| g.kind == "gate") {
                "ci_red"
            } else {
                "FAIL"
            };
            let what = "review FAIL, waiting for drive to start fix round";
            w(
                "drive",
                what,
                s.last_ts,
                cmd("spawn", &format!(" {} --supersede", to(by, o))),
            )
        }
        // #182: a head the PR handed off (`pushed`, logged by drive) is due too, whatever the worktree holds.
        "impl" | "fix"
            if !pushed.is_empty() && !s.reviewed(pushed) && (handed(pushed) || head.is_none_or(|h| h.0 == pushed)) =>
        {
            let what = format!(
                "pushed {}{}, waiting for drive to spawn review",
                if s.rounds > 0 { "fix " } else { "" },
                &pushed[..7.min(pushed.len())]
            );
            w(
                "drive",
                &what,
                head.map_or(s.last_ts, |h| h.1.max(s.last_ts)),
                cmd("spawn", &format!(" {}", to(build, "push"))),
            )
        }
        "impl" | "fix" if s.last_event == "lane-killed" => w(
            "drive",
            "lane killed, waiting for drive to respawn impl",
            s.last_ts,
            cmd("spawn", &format!(" {}", p.role(build).unwrap_or(p.start()))),
        ),
        "impl" | "fix" => w("none", "idle without pushing (check the pane)", s.last_ts, None),
        st => w("none", &format!("nothing to do ({st})"), s.last_ts, None),
    }
}

/// One row per lane herdr does not show `working`: the latest lane's reason (`s.lane == w.lane`), shared by the
/// issue's older lanes, which wait on the latest one while it works. `live` refreshes the PR read, else the cache only.
pub fn scan(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    events: &[Event],
    agents: &[Value],
    live: bool,
) -> Vec<(IssueState, Waiting)> {
    let mut out = vec![];
    let busy = |l: &str| agents.iter().any(|a| a["name"] == l && a["agent_status"] == "working");
    for s in fold(events).into_values().filter(|s| cfg.fleet.contains_key(&s.fleet)) {
        // #43: after a `lane-killed`, the issue's surviving lane carries its reason (a killed rev: spawn rev again).
        let Some(lane) = s.lane.clone().or_else(|| {
            s.build
                .as_ref()
                .and_then(|b| s.lanes.get(b))
                .or(s.check_lane())
                .cloned()
        }) else {
            continue;
        };
        let s = IssueState {
            lane: Some(lane.clone()),
            ..s
        };
        let (f, branch) = (&cfg.fleet[&s.fleet], crate::gate::branch_of(Some(&s), s.issue));
        // #169: no PR yet: the issue branch's latest run backs the `checks` link (the tick reads it, never a request).
        if live && !matches!(s.state.as_str(), "merged" | "closed") && gh::cached_pr(ctx, &s.fleet, s.issue).is_null() {
            gh::run(ctx, r, f, (&s.fleet, s.issue), &branch, ctx.now);
        }
        if busy(&lane) {
            let who = if s.check_lane() == Some(&lane) {
                "reviewer"
            } else {
                "none"
            };
            let w = Waiting {
                fleet: s.fleet.clone(),
                issue: s.issue,
                lane: lane.clone(),
                who,
                what: format!("waiting on {lane} (working)"),
                since: s.last_ts,
                next_cmd: None,
            };
            out.extend(s.lanes.values().filter(|l| !busy(l)).map(|l| {
                (
                    s.clone(),
                    Waiting {
                        lane: l.clone(),
                        ..w.clone()
                    },
                )
            }));
            continue;
        }
        let pr = match (s.state.as_str(), live) {
            ("merged" | "closed", _) => Value::Null,
            (_, true) => gh::pr(ctx, r, f, (&s.fleet, s.issue), &branch, ctx.now).unwrap_or_default(),
            _ => gh::cached_pr(ctx, &s.fleet, s.issue),
        };
        let wt = last_spawn(events, &lane)
            .and_then(|e| e.str("worktree"))
            .filter(|_| s.state != "merged");
        let git = wt.and_then(|wt| ok(r, "git", &["-C", wt, "log", "-1", "--format=%H %ct"]).ok());
        let head = git
            .as_deref()
            .and_then(|g| g.trim().split_once(' '))
            .and_then(|(h, t)| Some((h, t.parse().ok()?)));
        let spawn = s.check_lane().and_then(|l| last_spawn(events, l));
        let path = spawn
            .and_then(|e| e.str("worktree"))
            .map(|wt| format!("{wt}/verdict.md"));
        let fresh = |p: &String| {
            std::fs::metadata(p)
                .and_then(|m| m.modified())
                .is_ok_and(|t| t > std::time::UNIX_EPOCH + std::time::Duration::from_secs(spawn.map_or(0, |e| e.ts)))
        };
        let word = path
            .as_ref()
            .filter(|p| fresh(p))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|v| v.lines().next().unwrap_or_default().trim().to_string());
        // In review, `head` is the one the rev lane was spawned at (logged since #92; else its worktree's).
        let at = spawn
            .filter(|_| s.check_lane() == Some(&lane))
            .and_then(|e| Some((e.str("sha")?, e.ts)));
        let mut w = waiting_on(events, &s, at.or(head), &pr, f, path.as_deref().zip(word.as_deref()));
        // #150: a PASS whose PR lacks the merge label never gets its full CI run.
        if let Some(l) = f
            .merge_label
            .as_deref()
            .filter(|l| w.who == "ci" && !gh::has_label(&pr, l))
        {
            w.what = format!("PASS, PR lacks label `{l}`, so the full CI has not run");
        }
        w.what = w.what.replace("for drive", "for drive (next pass ≤ 5 min)");
        // #83: a paused fleet's lane with no pane and a clean, pushed worktree is parked until `resume`.
        let safe = || wt.is_some_and(|wt| crate::spawn::dirty(r, std::path::Path::new(wt)).is_none());
        if f.paused
            && !agents
                .iter()
                .any(|a| a["name"] == lane.as_str() && a["agent_status"] != "parked")
            && safe()
        {
            let pr = pr["number"]
                .as_u64()
                .map_or(String::new(), |p| format!(" (resume to review PR #{p})"));
            (w.who, w.what, w.next_cmd) = ("none", format!("parked: fleet paused{pr}"), None);
        }
        out.extend(s.lanes.values().filter(|l| **l != lane && !busy(l)).map(|l| {
            (
                s.clone(),
                Waiting {
                    lane: l.clone(),
                    ..w.clone()
                },
            )
        }));
        out.push((s, w));
    }
    out
}
