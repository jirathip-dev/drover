use crate::model::{Ev, Model, Script, FLEET};
use crate::scenario::{args, config, timers, T0};
use crate::sched::{Rng, World};

/// #959's "sometimes" states: the gate scenarios fail the run unless one of them reaches each.
pub const SOMETIMES: &[&str] = &["round-limit", "head-moved-after-pass", "merge-after-retried-verdict"];

/// #959: a gate that dies instead of answering, one scenario per seed (`world`).
pub const SCENARIOS: &[(u64, &str)] = &[
    (9591, "a rev lane exits before it writes a verdict"),
    (9592, "a rev lane hangs past its limit"),
    (9593, "a verdict file cut off mid-write"),
    (9594, "a verdict for an older head than the PR's"),
    (9595, "a verify lane dies after rev's PASS"),
    (9596, "CI turns red after the PASS, before the merge"),
    (9597, "drive dies between GitHub's merge call and the merged event"),
    (9598, "every reviewer FAILs: the limit of 3 refuses round 3"),
    (9599, "the impl pushes again right after the PASS, before the merge"),
];

const N: u64 = 959;

/// What an issue's head does once rev's PASS lands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AfterPass {
    CiRed,
    Push,
}

fn review(pass: bool) -> Script {
    Script::Review {
        secs: 300,
        pass,
        proof: true,
        heeds: true,
    }
}

fn fleet(seed: u64, issues: &[u64], flags: &[&str], rev: Vec<Script>) -> Model {
    let mut rng = Rng::new(seed);
    let mut m = Model::default();
    let s = m.sha();
    m.origin.insert("staging".into(), s);
    m.load = 1;
    m.ci_secs = Some(300);
    timers(&mut m, &mut rng, T0 + 1);
    let mut add = args(&["queue", FLEET, "add"]);
    add.extend(issues.iter().map(ToString::to_string));
    add.extend(args(flags));
    m.at(T0, Ev::Run(add));
    let push = Script::Push {
        secs: 600,
        green: true,
        ci: None,
    };
    m.scripts.insert("impl".into(), vec![push; 6]);
    m.scripts.insert("rev".into(), rev);
    m
}

/// Scenario `seed`'s fleet and fleet.toml.
pub fn world(seed: u64) -> (Model, String) {
    let one = |rev| fleet(seed, &[N], &[], rev);
    let then_pass = |first| vec![first, review(true)];
    let mut cfg = config(4, 3);
    let m = match seed {
        9591 => one(then_pass(Script::Die { secs: 120 })),
        9592 => one(then_pass(Script::Hang)),
        9593 => one(then_pass(Script::Bad {
            secs: 200,
            stale: false,
        })),
        9594 => one(then_pass(Script::Bad { secs: 200, stale: true })),
        9595 => {
            cfg += &format!("\n[fleet.{FLEET}.role.verify]\nharness = \"claude\"\nprofile = \"fleet-verify\"\n");
            let mut m = fleet(seed, &[N], &["--verify"], vec![review(true)]);
            m.scripts.insert("verify".into(), then_pass(Script::Die { secs: 120 }));
            m
        }
        9596 => {
            let mut m = one(vec![review(true)]);
            m.after_pass.push((N, AfterPass::CiRed));
            m
        }
        9597 => {
            let mut m = one(vec![review(true)]);
            m.crash_on_merge = true;
            m
        }
        9598 => one(vec![review(false); 6]),
        9599 => {
            let mut m = one(vec![review(true); 6]);
            m.after_pass.push((N, AfterPass::Push));
            m
        }
        _ => panic!("no gate scenario for seed {seed}"),
    };
    (m, cfg)
}

/// After each append: the scenario's faults keyed to it, and the "sometimes" states it reaches.
pub fn on_append(w: &mut World) {
    let Some(ev) = w.wrote.last().map(|x| x.ev.clone()) else {
        return;
    };
    let n = ev.issue.unwrap_or(0);
    let data = serde_json::to_string(&ev.data).unwrap_or_default();
    if ev.kind == "needs-human" && (data.contains("ROUND_LIMIT") || data.contains("review rounds ran out (3")) {
        w.model.reached.insert("round-limit");
    }
    match ev.kind.as_str() {
        "verdict" if ev.str("verdict") == Some("PASS") => {
            let Some(at) = w.model.after_pass.iter().position(|(i, _)| *i == n) else {
                return;
            };
            let (_, fault) = w.model.after_pass.remove(at);
            let sha = ev.str("sha").unwrap_or_default().to_string();
            match fault {
                AfterPass::CiRed => drop(w.model.ci.insert(sha.clone(), Some(false))),
                AfterPass::Push => push(w, n),
            }
            w.model.version += 1;
            w.log(&format!("model: after the PASS at {sha:.7}: {fault:?}"));
        }
        "merged" => {
            let mut agents: Vec<&str> = (w.wrote.iter())
                .filter(|x| x.ev.kind == "spawn" && x.ev.issue == Some(n))
                .filter(|x| matches!(x.ev.str("role"), Some("rev" | "verify")))
                .filter_map(|x| x.ev.str("agent"))
                .collect();
            let all = agents.len();
            agents.sort_unstable();
            agents.dedup();
            if agents.len() < all {
                w.model.reached.insert("merge-after-retried-verdict");
            }
        }
        _ => {}
    }
}

/// The impl's late push to issue `n`'s PR: a new head, its CI green.
fn push(w: &mut World, n: u64) {
    let branch = format!("issue-{n}");
    let sha = w.model.sha();
    let on = w.model.origin.get("staging").cloned().unwrap_or_default();
    w.model.base.insert(sha.clone(), on);
    w.model.origin.insert(branch.clone(), sha.clone());
    let Some(old) = (w.model.prs.get_mut(&branch)).map(|p| std::mem::replace(&mut p.head, sha.clone())) else {
        return;
    };
    let t = w.model.ci_start(&sha, w.now, None, &mut w.rng);
    w.model.at(t, Ev::Ci { sha, green: true });
    moved(w, &branch, &old);
}

/// `branch`'s PR head moved off `old`: reached "head-moved-after-pass" if `old` had a PASS.
pub fn moved(w: &mut World, branch: &str, old: &str) {
    let n: u64 = branch.trim_start_matches("issue-").parse().unwrap_or(0);
    let pass = |x: &&crate::sched::Wrote| {
        x.ev.kind == "verdict"
            && x.ev.issue == Some(n)
            && x.ev.str("sha") == Some(old)
            && x.ev.str("verdict") == Some("PASS")
    };
    if w.wrote.iter().any(|x| pass(&x)) {
        w.model.reached.insert("head-moved-after-pass");
    }
}

/// Each of `want` no run in `outs` reached, as a failure line.
pub fn unreached(outs: &[crate::Outcome], want: &[&str]) -> Vec<String> {
    let seeds: Vec<u64> = outs.iter().map(|o| o.seed).collect();
    (want.iter())
        .filter(|s| !outs.iter().any(|o| o.reached.contains(*s)))
        .map(|s| format!("sometimes {s}: never reached in seeds {seeds:?}"))
        .collect()
}
