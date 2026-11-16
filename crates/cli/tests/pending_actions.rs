mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::runner::{Out, Runner};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const IMPL: &str = "sendmeter-impl-12";
const ADD: &str = "git -C /tmp/does-not-matter/sendmeter worktree add";

fn sha(p: &str) -> String {
    format!("{p}{}", "0".repeat(40 - p.len()))
}

fn ev(c: &drover::Ctx, ts: u64, kind: &str, d: Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

fn started(sh: &FakeShed) -> usize {
    sh.all("run").iter().filter(|p| p["name"] == IMPL).count()
}

fn fake(level: &str) -> Fake {
    let pr = json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": sha("3"),
        "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]});
    Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", &format!("{level}\n"))
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("gh pr view issue-12", &pr.to_string())
        .once("git -C /tmp/does-not-matter/sendmeter rev-parse --verify", 1, "")
        .reply("git -C", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("kill -0", "")
}

fn get(c: &drover::Ctx, r: &Fake, path: &str, now: u64) -> Value {
    let sh = drover::config::Config::load(&c.config).unwrap().shed().unwrap();
    let r = drover::shed::With(r, sh);
    let s = Server::new(c, &r, Some(ADDR.into()));
    serde_json::from_str(&api(&s, ADDR, "", now, path)).unwrap()
}

fn cards(c: &drover::Ctx, r: &Fake, now: u64) -> Vec<(u64, String)> {
    let n = get(c, r, "needs", now);
    let i = n["issues"].as_array().unwrap().iter();
    i.map(|i| (i["issue"].as_u64().unwrap(), i["why"].as_str().unwrap().into()))
        .collect()
}

fn row(c: &drover::Ctx, r: &Fake) -> Value {
    let s = get(c, r, "state", c.now);
    s["queue"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["issue"] == 12)
        .unwrap()
        .clone()
}

fn spawns(c: &drover::Ctx) -> usize {
    read(&c.state).unwrap().iter().filter(|e| e.kind == "spawn").count()
}

fn pending(c: &drover::Ctx) -> Option<drover::pending::Pending> {
    drover::pending::of(&read(&c.state).unwrap(), "sendmeter", 12)
}

struct Hooked<'a> {
    fake: &'a Fake,
    at: &'a str,
    hook: &'a (dyn Fn() + Sync),
    fired: Shared<bool>,
}

impl Runner for Hooked<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        let fire = format!("{prog} {}", args.join(" ")).starts_with(self.at) && !self.fired.get();
        self.fired.set(self.fired.get() || fire);
        if fire {
            (self.hook)();
        }
        self.fake.run(prog, args)
    }
}

#[test]
fn an_answer_for_a_gone_lane_is_pending_and_two_racing_passes_start_it_once() {
    let c = ctx(&tmpdir("pending-answer"), CONFIG);
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let ask = json!({"id": "q1", "question": "Which DB?", "options": ["sqlite", "postgres"], "kind": "owner",
        "agent": "sendmeter-impl-12"});
    ev(&c, 20, "needs-decision", ask);
    let (busy, calm) = (fake("4"), fake("1"));
    let (code, out) = run(&c, &busy, &["answer", "sendmeter", "q1", "1"]);
    assert!(code == 0 && out.contains("asking agent gone"), "{out}");
    assert_eq!(run(&c, &busy, &["drive"]).0, 0);
    let p = pending(&c).expect("pending");
    assert_eq!((p.action.as_str(), p.role.as_str()), ("answer", "impl"));
    assert_eq!(row(&c, &calm)["status"], "starting, waiting for host: HOST_BUSY · 0s");

    let second = Shared::<Vec<String>>::default();
    let race = || {
        std::thread::scope(|s| {
            let b = s.spawn(|| run(&c, &calm, &["kick", "sendmeter", "12", "--now"]));
            second.borrow_mut().push(b.join().unwrap().1);
        });
    };
    let a = Hooked {
        fake: &calm,
        at: ADD,
        hook: &race,
        fired: Shared::new(false),
    };
    let (code, out) = run(&c, &a, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        second.borrow().concat().contains("REFUSED IN_FLIGHT"),
        "{:?}",
        second.borrow()
    );
    assert_eq!(started(&sh), 1);
    assert_eq!((spawns(&c), pending(&c)), (1, None));
    let failed = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == "pending-action");
    assert!(
        failed.into_iter().all(|e| e.val("failed") == false),
        "the skip failed nothing"
    );
}

#[test]
fn a_start_held_by_host_load_alone_never_goes_back_to_the_owner_and_starts_once_admitted() {
    let mut c = ctx(&tmpdir("pending-load"), CONFIG);
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let ask = json!({"id": "q1", "question": "Which DB?", "options": ["sqlite", "postgres"], "kind": "owner",
        "agent": "sendmeter-impl-12"});
    ev(&c, 20, "needs-decision", ask);
    let load = Out {
        stdout: "{ 40.00 3.00 2.00 }\n".into(),
        ..Out::default()
    };
    let swapd = Out {
        stdout: "  104     1 900.0 /usr/sbin/swapd\n".into(),
        ..Out::default()
    };
    let loaded = Fake {
        replies: [
            vec![
                ("sysctl -n vm.loadavg".to_string(), load),
                ("ps -Ao".to_string(), swapd),
            ],
            fake("1").replies,
        ]
        .concat(),
        ..Fake::default()
    };
    let calm = fake("1");
    let (code, out) = run(&c, &loaded, &["answer", "sendmeter", "q1", "1"]);
    assert!(code == 0 && out.contains("asking agent gone"), "{out}");
    c.now += 7200;
    let (code, out) = run(&c, &loaded, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(spawns(&c), 0, "nothing started on a loaded host");
    assert_eq!(pending(&c).unwrap().reason.as_deref(), Some(drover::pending::LOAD));
    assert_eq!(cards(&c, &calm, c.now), [], "two hours on load alone: no card");
    let (code, out) = run(&c, &calm, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(started(&sh), 1, "{out}");
    assert_eq!((spawns(&c), pending(&c)), (1, None));
}
