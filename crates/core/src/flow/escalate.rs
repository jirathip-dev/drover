use super::episode::{Episode, Level};
use super::table::outcomes;
use super::Step;
use crate::config::Config;
use crate::drive::Pass;
use crate::events::{append, emit, of_issue, open_asks, working, Event, IssueState};
use crate::runner::Runner;
use crate::thread::route::{home, lane_ask, route, to_owner};
use crate::thread::Target;
use anyhow::{Context, Result};
use serde_json::json;
use std::io::Write;

const LIMITS: [&str; 1] = ["STUCK"];

fn log(pass: &Pass, s: &IssueState, kind: &str, d: serde_json::Value) -> Result<()> {
    let e = Event::new(pass.ctx().now, &s.fleet, Some(s.issue), kind, d).passive();
    append(&pass.ctx().state, &e)
}

fn thread(cfg: &Config, events: &[Event], s: &IssueState) -> Option<String> {
    match home(events, &s.fleet, Some(s.issue)) {
        Target::Thread { id } => Some(id),
        Target::Desk { .. } => route(cfg, events, &s.fleet).map(String::from),
        Target::Owner => None,
    }
}

fn card(events: &[Event], s: &IssueState, ep: &Episode, step: &Step, code: Option<&str>) -> (String, Vec<String>) {
    let mine = of_issue(events, &s.fleet, s.issue);
    let tried = mine.iter().filter(|e| e.ts >= ep.began && e.kind == "row-tried");
    let lines: Vec<String> = (tried.filter(|e| e.str("row") == Some(&step.row)))
        .map(|e| {
            let at = |k| e.str(k).unwrap_or_default();
            format!("try {} {}: {} {}", e.val("try"), at("action"), at("code"), at("why"))
        })
        .collect();
    let last = lines.get(lines.len().saturating_sub(3)..).unwrap_or_default();
    let mut opts = vec!["retry".to_string(), "park".to_string()];
    if code.is_some_and(|c| LIMITS.contains(&c)) {
        opts.push("one more round".into());
    }
    (format!("{}: {}", step.row, last.join("; ")), opts)
}

/// Raises issue `s`'s row to the level `decide` put `step` at, `code` its last try's refusal: one ask in the issue's orchestrator thread, then one owner card with [retry] and [park]; a host's refusal raises nothing. The level the issue is at after.
pub fn escalate(
    pass: &Pass,
    cfg: &Config,
    r: &dyn Runner,
    (s, ep): (&IssueState, &Episode),
    step: &Step,
    code: Option<&str>,
) -> Result<Level> {
    let held = code.map(|c| outcomes().map(|o| o.class(c) == "held")).transpose()?;
    if step.level <= ep.level || held == Some(true) {
        return Ok(ep.level.clone());
    }
    let (ctx, events) = (pass.ctx(), working(&pass.ctx().state)?);
    let open = open_asks(&events);
    let mine = |e: &&&Event| e.fleet == s.fleet && e.issue == Some(s.issue);
    let (q, opts) = card(&events, s, ep, step, code);
    let to = thread(cfg, &events, s).filter(|_| matches!(step.level, Level::Orchestrator { .. }));
    let (ask, new) = match (&ep.level, open.iter().find(mine)) {
        (Level::Orchestrator { ask }, _) if !ask.is_empty() => (ask.clone(), false),
        (_, Some(a)) => (a.str("id").unwrap_or_default().to_string(), false),
        _ => {
            let d =
                json!({"question": q, "options": opts, "recommend": 1, "kind": "owner", "row": step.row, "routed": to});
            let ev = Event::new(ctx.now, &s.fleet, Some(s.issue), "needs-decision", d);
            let key = format!("row:{}:{}:{}:{}", s.fleet, s.issue, step.row, ep.began);
            let ev = crate::events::ask(&ctx.state, cfg, vec![ev], Some(&key))?.pop();
            (
                ev.and_then(|e| e.str("id").map(String::from))
                    .context("card not logged")?,
                true,
            )
        }
    };
    let fresh = working(&ctx.state)?;
    let a = crate::events::ask_event(&fresh, "needs-decision", &s.fleet, &ask).context("card not logged")?;
    let at = a.str("routed").map(String::from).or(to);
    let reached = match (&step.level, &at) {
        (Level::Orchestrator { .. }, Some(_)) if !new => Ok(()),
        (Level::Orchestrator { .. }, Some(t)) => lane_ask(ctx, cfg, r, t, a).map(drop),
        (Level::Orchestrator { .. }, None) => Err(anyhow::anyhow!("no orchestrator thread")),
        _ => Ok(()),
    };
    let level = match reached {
        Ok(()) if matches!(step.level, Level::Orchestrator { .. }) => Level::Orchestrator { ask: ask.clone() },
        Ok(()) => Level::Owner { card: ask.clone() },
        Err(e) => {
            if let Some(t) = &at {
                to_owner(ctx, cfg, t, a, Some(1), &format!("orchestrator not reached: {e:#}"))?;
            }
            Level::Owner { card: ask.clone() }
        }
    };
    let carded = (of_issue(&fresh, &s.fleet, s.issue).iter())
        .any(|e| e.kind == "needs-human" && e.str("ask") == Some(ask.as_str()));
    if matches!(level, Level::Owner { .. }) && !carded {
        if let (Some(t), Level::Orchestrator { .. }) = (&at, &ep.level) {
            to_owner(ctx, cfg, t, a, Some(1), "escalate_minutes passed")?;
        }
        let d = json!({"code": code.unwrap_or("ROW_ESCALATED"), "reason": q, "row": step.row, "ask": ask});
        emit(
            &ctx.state,
            cfg,
            Event::new(ctx.now, &s.fleet, Some(s.issue), "needs-human", d),
        )?;
    }
    let d = json!({"row": step.row, "level": level.name(), "ask": ask, "thread": at});
    log(pass, s, "row-escalated", d)?;
    Ok(level)
}

/// The answer `choice` by `by` to issue `s`'s card on row `row`: [retry] zeroes the row's tries (an orchestrator's once per row and episode), [park] parks, [one more round] extends. Whether it applied.
pub fn settle(
    pass: &Pass,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (s, ep): (&IssueState, &Episode),
    (row, choice, by): (&str, &str, &str),
) -> Result<bool> {
    let orchestrator = by.starts_with("orchestrator-");
    let ctx = pass.ctx();
    match choice {
        "retry" if orchestrator && ep.answered.contains(row) => Ok(false),
        "retry" => log(pass, s, "row-retry", json!({"row": row, "by": by})).map(|()| true),
        "park" if !orchestrator => {
            crate::gate::park(ctx, cfg, r, out, (&s.fleet, s.issue), by, Some(row)).map(|()| true)
        }
        "one more round" if !orchestrator => {
            crate::gate::extend_ask(ctx, cfg, r, out, (&s.fleet, s.issue), (true, by)).map(|()| true)
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::table::Owner;
    use crate::runner::Real;
    use crate::Ctx;

    const ROW: &str = "issue/build/lane-ended";

    struct Fx {
        dir: std::path::PathBuf,
        ctx: Ctx,
        cfg: Config,
        s: IssueState,
    }

    impl Fx {
        fn new(name: &str, thread: bool) -> Fx {
            let dir = std::env::temp_dir().join(format!("drover-escalate-{name}-{}", crate::runner::pid()));
            std::fs::remove_dir_all(&dir).ok();
            std::fs::create_dir_all(&dir).unwrap();
            let ctx = Ctx {
                config: dir.join("fleet.toml"),
                state: dir.join("events.jsonl"),
                worktrees: dir.join("w"),
                exe: "drover".into(),
                now: 10_000,
                home: dir.clone(),
                hints: vec![],
            };
            let s = IssueState {
                fleet: "f".into(),
                issue: 7,
                state: "impl".into(),
                ..IssueState::default()
            };
            let fx = Fx {
                dir,
                ctx,
                cfg: Config::default(),
                s,
            };
            if thread {
                fx.add(None, "orchestrator-attached", json!({"thread": "t1"}));
                fx.add(None, "queue", json!({"op": "add", "issues": [7], "thread": "t1"}));
            }
            fx.add(Some(7), "spawn", json!({"role": "impl"}));
            fx
        }

        fn add(&self, n: Option<u64>, kind: &str, d: serde_json::Value) {
            append(&self.ctx.state, &Event::new(self.ctx.now, "f", n, kind, d)).unwrap();
        }

        fn events(&self) -> Vec<Event> {
            working(&self.ctx.state).unwrap().to_vec()
        }

        fn ep(&self) -> Episode {
            Episode::of(&self.events(), &self.s)
        }

        fn up(&self, row: &str, level: Level, code: Option<&str>) -> Level {
            let step = Step {
                row: row.into(),
                owner: Owner::Drive,
                act: None,
                tries: 4,
                deadline: None,
                expired: false,
                level,
                say: String::new(),
            };
            let pass = Pass::test(&self.ctx, "f");
            escalate(&pass, &self.cfg, &Real, (&self.s, &self.ep()), &step, code).unwrap()
        }

        fn count(&self, kind: &str) -> usize {
            self.events().iter().filter(|e| e.kind == kind).count()
        }

        fn last(&self, kind: &str) -> Event {
            self.events().into_iter().rfind(|e| e.kind == kind).unwrap()
        }
    }

    impl Drop for Fx {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.dir).ok();
        }
    }

    fn orch() -> Level {
        Level::Orchestrator { ask: String::new() }
    }

    fn owner() -> Level {
        Level::Owner { card: String::new() }
    }

    #[test]
    fn a_row_out_of_tries_goes_to_its_thread_once_then_to_the_owner_as_one_card() {
        let fx = Fx::new("ladder", true);
        let try_ =
            json!({"row": ROW, "try": 3, "action": "respawn", "outcome": "failed", "code": "EXIT", "why": "lane died"});
        fx.add(Some(7), "row-tried", try_);
        let at = fx.up(ROW, orch(), Some("EXIT"));
        let ask = fx.last("needs-decision");
        let id = ask.str("id").unwrap().to_string();
        assert_eq!(at, Level::Orchestrator { ask: id.clone() });
        assert_eq!((ask.str("routed"), ask.str("row")), (Some("t1"), Some(ROW)));
        assert_eq!(*ask.val("options"), json!(["retry", "park"]));
        assert!(ask
            .str("question")
            .unwrap()
            .starts_with("issue/build/lane-ended: try 3 respawn: EXIT lane died"));
        let msg = fx.last("thread-msg");
        assert_eq!(msg.str("thread"), Some("t1"));
        assert!(msg.str("text").unwrap().contains(&id));
        let e = fx.last("row-escalated");
        assert_eq!(
            (e.str("level"), e.str("ask"), e.str("thread")),
            (Some("orchestrator"), Some(id.as_str()), Some("t1"))
        );
        assert_eq!(fx.count("needs-human"), 0);

        assert_eq!(
            fx.up(ROW, orch(), Some("EXIT")),
            at,
            "the same level twice raises nothing"
        );
        assert_eq!((fx.count("needs-decision"), fx.count("row-escalated")), (1, 1));

        let card = fx.up(ROW, owner(), Some("EXIT"));
        assert_eq!(card, Level::Owner { card: id.clone() });
        assert_eq!(fx.count("needs-decision"), 1, "the thread's ask is the card");
        assert_eq!(fx.last("ask-owner").str("id"), Some(id.as_str()));
        let nh = fx.last("needs-human");
        assert_eq!((nh.str("row"), nh.str("code")), (Some(ROW), Some("EXIT")));
        assert_eq!(fx.last("row-escalated").str("level"), Some("owner"));
        assert_eq!(fx.up(ROW, owner(), Some("EXIT")), card);
        assert_eq!(
            (fx.count("needs-human"), fx.count("row-escalated")),
            (1, 2),
            "never a second card"
        );
    }

    #[test]
    fn with_no_orchestrator_thread_the_owner_gets_the_card_and_a_second_row_adds_to_it() {
        let fx = Fx::new("no-thread", false);
        let at = fx.up(ROW, orch(), Some("STUCK"));
        let ask = fx.last("needs-decision");
        let id = ask.str("id").unwrap().to_string();
        assert_eq!(at, Level::Owner { card: id.clone() });
        assert_eq!(ask.str("routed"), None);
        assert_eq!(*ask.val("options"), json!(["retry", "park", "one more round"]));
        assert_eq!(fx.last("needs-human").str("row"), Some(ROW));

        let other = "issue/build/pr-closed";
        let mut s = fx.s.clone();
        s.state = "impl".into();
        let step = Step {
            row: other.into(),
            owner: Owner::Drive,
            act: None,
            tries: 4,
            deadline: None,
            expired: false,
            level: owner(),
            say: String::new(),
        };
        let pass = Pass::test(&fx.ctx, "f");
        escalate(&pass, &fx.cfg, &Real, (&s, &Episode::default()), &step, None).unwrap();
        assert_eq!(
            (fx.count("needs-decision"), fx.count("needs-human")),
            (1, 1),
            "one open card per issue"
        );
        let e = fx.last("row-escalated");
        assert_eq!((e.str("row"), e.str("ask")), (Some(other), Some(id.as_str())));
    }

    #[test]
    fn a_start_the_host_refuses_raises_no_ask_no_thread_line_and_no_card() {
        let fx = Fx::new("held", true);
        assert_eq!(fx.up("issue/queued/start", owner(), Some("HOST_BUSY")), Level::Auto);
        for kind in [
            "needs-decision",
            "thread-msg",
            "needs-human",
            "row-escalated",
            "ask-owner",
        ] {
            assert_eq!(fx.count(kind), 0, "{kind}");
        }
    }

    #[test]
    fn retry_zeroes_the_row_once_per_episode_for_the_orchestrator_and_always_for_the_owner() {
        let fx = Fx::new("retry", true);
        fx.add(
            Some(7),
            "row-tried",
            json!({"row": ROW, "try": 3, "outcome": "final", "code": "EXIT"}),
        );
        let pass = Pass::test(&fx.ctx, "f");
        let mut out = Vec::new();
        let mut go = |by: &str| {
            let ep = fx.ep();
            settle(&pass, &fx.cfg, &Real, &mut out, (&fx.s, &ep), (ROW, "retry", by)).unwrap()
        };
        assert!(go("orchestrator-t1"));
        assert_eq!(fx.ep().tries.get(ROW), None);
        assert!(fx.ep().answered.contains(ROW));
        assert!(
            !go("orchestrator-t1"),
            "the orchestrator answers a row at most once per episode"
        );
        assert_eq!(fx.count("row-retry"), 1);
        assert!(go("owner"));
        assert_eq!(fx.last("row-retry").str("by"), Some("owner"));
        assert!(fx.ep().answered.is_empty(), "the owner's retry begins a new episode");
    }
}
