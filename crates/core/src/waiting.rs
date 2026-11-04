//! #46: what an idle lane waits on (drive | owner | ci | none), worked out from the log, git and the cached PR.
use crate::config::{Config, Fleet};
use crate::events::{age, fold, last_spawn, open_asks, Event, IssueState};
use crate::runner::{ok, Runner};
use crate::{gh, Ctx};
use serde::Serialize;
use serde_json::Value;

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(optional_fields = nullable))]
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
    /// The same row for another lane of the issue.
    fn on(&self, lane: &str) -> Waiting {
        Waiting {
            lane: lane.into(),
            ..self.clone()
        }
    }
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

/// #217: an extra round is recorded for a lane that has not started yet (`extend --no-spawn` asks for none).
pub fn unspawned(events: &[Event], s: &IssueState) -> bool {
    let last = events
        .iter()
        .rev()
        .find(|e| e.fleet == s.fleet && e.issue == Some(s.issue));
    s.last_event == "extend-failed"
        || last.is_some_and(|e| e.kind == "round-extended" && e.data.get("spawn") == Some(&Value::Bool(true)))
}

/// #327: the fix lane died (`lane-dead`) or its supersede never spawned, with no new head: the fix is still due.
fn lost(events: &[Event], s: &IssueState, pushed: &str) -> bool {
    let mut since = events.iter().rev().take_while(|e| !e.is("spawn", &s.fleet, s.issue));
    let dead = |e: &Event| e.is("lane-dead", &s.fleet, s.issue) || e.is("supersede", &s.fleet, s.issue);
    (pushed.is_empty() || s.reviewed(pushed)) && since.any(dead)
}

/// The 40-hex sha on verdict.md's `Reviewed head` line (#293).
pub fn reviewed_head(t: &str) -> Option<&str> {
    let l = t.lines().find(|l| l.contains("Reviewed head"))?;
    l.split(|c: char| !c.is_ascii_hexdigit()).find(|w| w.len() == 40)
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
    let (f, n, req, p) = (&s.fleet, s.issue, fc.checks(pr), fc.flow());
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
    // #350: a reverted landing's issue is in a fix round while its old PR stays merged.
    if s.state == "merged" || pr["state"] == "MERGED" && !s.state.starts_with("fix") {
        return w("none", "nothing to do (merged): clean me", s.last_ts, None);
    }
    if s.state == "closed" || pr["state"] == "CLOSED" {
        return w("none", "nothing to do (closed): clean me", s.last_ts, None);
    }
    // #294: a lane held for a serial slot waits for drive's update round once the holder merges.
    if let Some(g) = fc
        .serial
        .iter()
        .filter(|g| crate::queue::held(events, f, n, &g.name))
        .min_by_key(|g| &g.name)
    {
        let st = crate::events::fold(events);
        let h = crate::queue::holders(events, &st, fc, f, &g.name);
        let who: Vec<_> = h.iter().map(|s| format!("#{}", s.issue)).collect();
        let by = if who.is_empty() {
            "for a slot".into()
        } else {
            format!("held by {}", who.join(" "))
        };
        let what = format!("waits: {} {by} ({}/{})", g.name, h.len(), g.max);
        return w("drive", &what, s.last_ts, None);
    }
    // #104: an impl/fix/review lane whose account capped since its spawn respawns on a free one (`kill` pushes `wip/`).
    let active = matches!(s.state.split('(').next(), Some("impl" | "fix" | "review"));
    let spawned = last_spawn(events, &lane).filter(|_| active);
    let since_spawn = |e: &&Event| spawned.is_some_and(|sp| e.ts >= sp.ts && e.str("account") == sp.str("account"));
    let rest = |e: &&Event| e.is("account-capped", f, n) || e.is("agent-unavailable", f, n); // #302
    if let Some(c) = events.iter().filter(since_spawn).find(rest) {
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
    let checks = gh::required(&pr["statusCheckRollup"], req);
    let how = match (pr["mergeable"].as_str(), pr["mergeStateStatus"].as_str()) {
        (Some("CONFLICTING"), _) => "conflicts with",
        (_, Some("BEHIND")) => "behind",
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
        // #214: blocked with an open PR is no resting state: a conflict is updated, anything else reviewed again.
        _ if s.state == "blocked" && pr["state"] == "OPEN" => {
            if how.is_empty() {
                "stale"
            } else {
                "ready"
            }
        }
        _ => s.state.split('(').next().unwrap_or_default(),
    };
    let at_head = |h: &str| h.eq_ignore_ascii_case(base) || h.eq_ignore_ascii_case(pushed);
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
            let base = pr["baseRefName"].as_str().unwrap_or("?");
            let what = format!("PASS · {how} {base} · catching up next pass (≤ 5 min)");
            let b = to(gate, "conflict");
            w(
                "drive",
                &what,
                s.last_ts,
                cmd("spawn", &format!(" {b} --supersede --update")),
            )
        }
        // #212: CI read live by the fast step, which merges a green PASS within a minute.
        "ready" if gh::ci(&checks) == "green" => w(
            "drive",
            "PASS · CI green · merging within 1 min",
            s.last_ts,
            cmd("merge", ""),
        ),
        "ready" => {
            let all = checks.as_array().map(Vec::as_slice).unwrap_or_default();
            let n = all.iter().filter(|c| gh::check_state(c) == "green").count();
            let what = match all.iter().find(|c| gh::check_state(c) == "failed") {
                Some(c) => format!("PASS · CI red: {}", gh::check_name(c)),
                None => format!("PASS · CI running {n}/{}", all.len()),
            };
            w("ci", &what, s.last_ts, None)
        }
        "stale" => w(
            "drive",
            "no verdict for the PR head: review again",
            s.last_ts,
            cmd("spawn", &format!(" {} --supersede", to(build, "push"))),
        ),
        // The exact command, or none (no nudge) while the verdict word or the head sha is unknown.
        "review" => match rev {
            // #92: a verdict is for the head the rev lane was spawned at; a moved PR head gets a fresh reviewer.
            Some(_) if head.is_some_and(|h| !pushed.is_empty() && !at_head(h.0)) => {
                let what = format!(
                    "review was for {:.7}, head moved to {pushed:.7} (not a clean merge): review again",
                    head.unwrap_or_default().0
                );
                let c = s.check.as_deref().unwrap_or("rev");
                w("drive", &what, s.last_ts, cmd("spawn", &format!(" {c} --supersede")))
            }
            Some((path, v)) => {
                let at = head.is_some_and(|h| at_head(h.0));
                let next = (matches!(v, "PASS" | "FAIL") && at).then(|| {
                    format!(
                        "herdsman verdict {f} {n} {v} --sha {} --file {path}",
                        head.unwrap_or_default().0
                    )
                });
                let what = match next {
                    Some(_) => "review done · recording within 1 min",
                    None => "verdict written, waiting for drive to record it",
                };
                w("drive", what, s.last_ts, next)
            }
            None => w("none", "rev idle without a verdict (check the pane)", s.last_ts, None),
        },
        // #217: an extra round whose lane never started (a failed or deferred spawn) is due the same way.
        "fix" if s.last_event == "verdict" || unspawned(events, s) || lost(events, s, pushed) => {
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
        // #319: so is the FAILed head again after a fix round that changed nothing (`fix-no-change`).
        "impl" | "fix"
            if !pushed.is_empty()
                && (!s.reviewed(pushed) || s.last_event == "fix-no-change")
                && (handed(pushed) || head.is_none_or(|h| h.0 == pushed)) =>
        {
            let mut what = format!(
                "pushed {}{}, waiting for drive to spawn review",
                if s.rounds > 0 { "fix " } else { "" },
                &pushed[..7.min(pushed.len())]
            );
            // #212: drive's last try at this review was refused: say why in plain words.
            let tried = events.iter().rfind(|e| e.is("driven", f, n));
            let refused =
                |e: &&Event| e.str("step") == Some("review") && !matches!(e.str("code"), None | Some("EXIT" | "ERROR"));
            if let Some(e) = tried.filter(refused) {
                let why = crate::drive::words(e.str("code"), "review", e.str("result").unwrap_or_default());
                what = format!("waiting for a reviewer slot: {why}");
            }
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
            out.extend(s.lanes.values().filter(|l| !busy(l)).map(|l| (s.clone(), w.on(l))));
            continue;
        }
        let mut pr = match (s.state.as_str(), live) {
            ("merged" | "closed", _) => Value::Null,
            (_, true) => gh::pr(ctx, r, f, (&s.fleet, s.issue), &branch, ctx.now)
                .unwrap_or_else(|_| gh::cached_pr(ctx, &s.fleet, s.issue)),
            _ => gh::cached_pr(ctx, &s.fleet, s.issue),
        };
        crate::land::mark(ctx, None, f, (&s.fleet, s.issue), events, &mut pr);
        let sp = last_spawn(events, &lane);
        let wt = sp.and_then(|e| e.str("worktree")).filter(|_| !s.done());
        let on = crate::hosts::On::of(r, sp);
        let spawn = s.check_lane().and_then(|l| last_spawn(events, l));
        // #337: the worktree head only where it decides something: an impl/fix not seen pushed, a review with no sha.
        let pushed = pr["headRefOid"].as_str().unwrap_or_default();
        let seen = events
            .iter()
            .any(|e| e.is("pushed", &s.fleet, s.issue) && e.str("sha") == Some(pushed));
        let need = match s.state.split('(').next() {
            Some("impl" | "fix") => pushed.is_empty() || !seen,
            Some("review") => spawn.and_then(|e| e.str("sha")).is_none(),
            _ => false,
        };
        let git = (wt.filter(|_| need)).and_then(|wt| ok(&on, "git", &["-C", wt, "log", "-1", "--format=%H %ct"]).ok());
        let head = git
            .as_deref()
            .and_then(|g| g.trim().split_once(' '))
            .and_then(|(h, t)| Some((h, t.parse().ok()?)));
        let path = spawn
            .and_then(|e| e.str("worktree"))
            .map(|wt| format!("{wt}/verdict.md"));
        // #376: an adopted orphan's verdict counts from its `spawn-started` (none logged: any).
        let since = spawn.map_or(0, |e| e.data.get("since").and_then(Value::as_u64).unwrap_or(e.ts));
        let fresh = |p: &String| {
            std::fs::metadata(p)
                .and_then(|m| m.modified())
                .is_ok_and(|t| t > std::time::UNIX_EPOCH + std::time::Duration::from_secs(since))
        };
        // #157: on another host the rev spawn removed any older verdict.md, so one there is this round's.
        let there = crate::hosts::On::of(r, spawn);
        // #337: a verdict recorded since the rev spawn stands for its file, which is then never read again.
        let kept = spawn.and_then(|sp| {
            events
                .iter()
                .rfind(|e| e.is("verdict", &s.fleet, s.issue) && e.ts >= sp.ts)
        });
        let text = path
            .as_ref()
            .filter(|_| kept.is_none())
            .and_then(|p| match there.machine {
                Some(_) => there.read(std::path::Path::new(p)),
                None => std::fs::read_to_string(p).ok().filter(|_| fresh(p)),
            });
        let word = text
            .as_deref()
            .map(|v| v.lines().next().unwrap_or_default().trim().to_string())
            // #228: a form-only FAIL is recorded as `FAIL`; `verdict` reads the tag from the file.
            .map(|v| if v == "FAIL (form-only)" { "FAIL".into() } else { v })
            .or_else(|| kept.and_then(|e| e.str("verdict")).map(String::from));
        let named = text
            .as_deref()
            .and_then(reviewed_head)
            .or_else(|| kept.and_then(|e| e.str("sha")));
        // In review, `head` is the sha verdict.md names (#293), or the rev spawn head (#92; else its worktree's).
        let at = spawn
            .filter(|_| s.check_lane() == Some(&lane))
            .and_then(|e| Some((named.or(e.str("sha"))?, e.ts)));
        // #207: a PR head that merges the reviewed one with integration and adds nothing carries its verdict, as a
        // logged auto-update does: read as one, the verdict is recorded at the reviewed sha.
        let updated = |e: &Event| e.is("auto-updated", &s.fleet, s.issue) && e.str("to") == Some(pushed);
        let carry = at.filter(|(x, _)| {
            word.is_some()
                && !pushed.is_empty()
                && !x.eq_ignore_ascii_case(pushed)
                && !events.iter().any(updated)
                && crate::overlap::carries(r, f, &s, x, pushed)
        });
        let ev = carry.map(|(x, _)| {
            let d = serde_json::json!({"from": x, "to": pushed});
            [
                events,
                &[Event::new(ctx.now, &s.fleet, Some(s.issue), "auto-updated", d)],
            ]
            .concat()
        });
        let ev = ev.as_deref().unwrap_or(events);
        let mut w = waiting_on(ev, &s, at.or(head), &pr, f, path.as_deref().zip(word.as_deref()));
        // #285: behind integration under a non-strict rule is no catch-up: the PR merges as it is.
        if w.what.starts_with("PASS · behind ") && !gh::update_ready(ctx, None, f, &s.fleet) {
            let green = gh::ci(&gh::required(&pr["statusCheckRollup"], f.checks(&pr))) == "green";
            let merge = format!("herdsman merge {} {}", s.fleet, s.issue);
            (w.who, w.next_cmd) = if green { ("drive", Some(merge)) } else { ("ci", None) };
            w.what = format!("PASS · behind {} (not required) · merging when CI green", f.integration);
        }
        // #249: a review on another host names it.
        if let Some(h) = spawn.filter(|_| there.machine.is_some()).and_then(|e| e.str("host")) {
            w.what = w.what.replace("review done", &format!("review done on {h}"));
        }
        // #214: a reviewer whose lane exited (no herdr agent) without a verdict gets a new review round.
        if w.what.starts_with("rev idle without") && !agents.iter().any(|a| a["name"] == lane.as_str()) {
            let c = s.check.as_deref().unwrap_or("rev");
            w.who = "drive";
            w.what = "reviewer exited without a verdict: reviewing again".into();
            w.next_cmd = Some(format!("herdsman spawn {} {} {c} --supersede", s.fleet, s.issue));
        }
        // #388: a PASS whose head never got a check says CI is missing or broken, never `running 0/0`.
        let grace = cfg.limits.ci_start_minutes * 60;
        let gone = w
            .what
            .starts_with("PASS · CI running")
            .then(|| gh::missing(ctx, live.then_some(r), f, (&s.fleet, s.issue), events, &pr, grace));
        if let Some(m) = gone.flatten() {
            w.what = format!("PASS · {m}");
        }
        // #150: a PASS whose PR lacks the merge label never gets its full CI run.
        if let Some(l) = f
            .merge_label
            .as_deref()
            .filter(|l| w.who == "ci" && !gh::has_label(&pr, l))
        {
            w.what = format!("PASS, PR lacks label `{l}`, so the full CI has not run");
        }
        w.what = w.what.replace("for drive", "for drive (next pass ≤ 5 min)");
        // #212: a CI/merge line from a PR read older than 2 min (no fast step since) says when it was taken.
        let at = gh::cached_at(ctx, &s.fleet, s.issue).filter(|t| ctx.now > t + 120);
        if let Some(t) = at.filter(|_| w.what.starts_with("PASS · ")) {
            w.what += &format!(" · checked {} ago", age(ctx.now - t));
        }
        // #83: a paused fleet's lane with no pane and a clean, pushed worktree is parked until `resume`.
        let safe = || wt.is_some_and(|wt| crate::spawn::dirty(&on, std::path::Path::new(wt)).is_none());
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
        out.extend(
            s.lanes
                .values()
                .filter(|l| **l != lane && !busy(l))
                .map(|l| (s.clone(), w.on(l))),
        );
        out.push((s, w));
    }
    out
}
