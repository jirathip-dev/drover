mod common;
use common::*;
use herdsman::config::Config;
use herdsman::events::{append, read, Event};
use serde_json::{json, Value};
use std::sync::Mutex;

const NOW: u64 = 1_000_000;
static ENV: Mutex<()> = Mutex::new(());

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

fn ev(c: &herdsman::Ctx, ts: u64, kind: &str, d: Value) {
    append(&c.state, &Event::new(ts, "sendmeter", None, kind, d)).unwrap();
}

#[test]
fn the_old_names_load_and_act_the_same_with_one_doctor_line() {
    let _env = ENV.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let old = format!(
        "{CONFIG}\n[fleet.sendmeter.role.conductor]\nharness = \"claude\"\nmodel = \"sonnet\"\n\n[conductor]\nallow = [\"design\"]\n"
    );
    let cfg = Config::parse(&old).unwrap();
    let role = &cfg.fleet["sendmeter"].role["orchestrator"];
    assert_eq!(
        (role.model.as_deref(), role.default),
        (Some("sonnet"), false),
        "role.conductor is the role"
    );
    assert!(!cfg.fleet["sendmeter"].role.contains_key("conductor"));
    assert_eq!(cfg.orchestrator.groups(), ["design"], "[conductor] is [orchestrator]");

    let c = ctx(&tmpdir("orch-old"), &old);
    let (_, doc) = run(&c, &Fake::default(), &["doctor"]);
    let lines: Vec<&str> = doc.lines().filter(|l| l.contains("conductor")).collect();
    assert_eq!(
        lines,
        ["! config — [conductor] is renamed (#780): call it [orchestrator] and role.orchestrator; the old names are \
          read for one release"]
    );
    let new = format!("{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\n");
    let c2 = ctx(&tmpdir("orch-new"), &new);
    assert!(!run(&c2, &Fake::default(), &["doctor"]).1.contains("[conductor]"));

    ev(&c, NOW - 300, "thread-new", json!({"thread": "t1", "title": "Grill"}));
    ev(
        &c,
        NOW - 250,
        "conductor-start",
        json!({"thread": "t1", "agent": "conductor-t1", "pane": "w1:p1"}),
    );
    ev(
        &c,
        NOW - 240,
        "thread-msg",
        json!({"thread": "t1", "from": "conductor", "text": "Hi"}),
    );
    ev(
        &c,
        NOW - 230,
        "conductor-sleep",
        json!({"thread": "t1", "agent": "conductor-t1", "why": "idle"}),
    );
    assert_eq!(
        (
            kinds(&c, "orchestrator-start").len(),
            kinds(&c, "orchestrator-sleep").len()
        ),
        (1, 1)
    );
    assert!(
        kinds(&c, "conductor-start").is_empty(),
        "old types read as the new ones"
    );
    assert!(run(&c, &Fake::default(), &["thread", "show", "t1"])
        .1
        .contains("\nOrchestrator: Hi\n"));
    let ls: Vec<Value> = serde_json::from_str(&run(&c, &Fake::default(), &["thread", "ls", "--json"]).1).unwrap();
    assert_eq!(ls[0]["state"], "asleep", "{ls:?}");
    for (q, t) in [("q1", None), ("q2", Some("t1"))] {
        let d = json!({"id": q, "question": "Which?", "options": ["a", "b"], "thread": t, "kind": "owner"});
        ev(&c, NOW - 200, "needs-decision", d);
    }
    ev(
        &c,
        NOW - 60,
        "decision",
        json!({"id": "q1", "choice": "a", "by": "conductor-t1"}),
    );
    let d = kinds(&c, "decision");
    assert_eq!(
        d[0].str("by"),
        Some("orchestrator-t1"),
        "an old event's by reads as the new one"
    );
    let (code, out) = run(
        &c,
        &Fake::default(),
        &["answer", "sendmeter", "q1", "2", "--by", "owner"],
    );
    assert_eq!(code, 0, "{out}");
    assert_eq!(kinds(&c, "decision").last().unwrap().str("choice"), Some("b"));
    let (code, out) = run(
        &c,
        &Fake::default(),
        &["ask", "withdraw", "sendmeter", "q2", "--by", "conductor-t1"],
    );
    assert_eq!(code, 0, "{out}");
    assert_eq!(kinds(&c, "ask-withdrawn")[0].str("by"), Some("orchestrator-t1"));
}

fn two(name: &str) -> herdsman::Ctx {
    let cfg = format!(
        "{CONFIG}\n[orchestrator.ada]\nname = \"Ada\"\nfleets = [\"sendmeter\"]\n\n[orchestrator.bob]\nname = \"Bob\"\nfleets = [\"all\"]\n"
    );
    ctx(&tmpdir(name), &cfg)
}

fn as_orch(c: &herdsman::Ctx, r: &Fake, id: &str, args: &[&str]) -> (i32, String) {
    std::env::set_var("HERDSMAN_LANE", "sendmeter:-:orchestrator");
    std::env::set_var("HERDSMAN_ORCHESTRATOR", id);
    let out = run(c, r, args);
    std::env::remove_var("HERDSMAN_LANE");
    std::env::remove_var("HERDSMAN_ORCHESTRATOR");
    out
}

fn spawnable() -> Fake {
    Fake::default()
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "")
        .reply("herdr agent list", NO_AGENTS)
        .reply(
            "herdr worktree create",
            r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#,
        )
        .reply(
            "herdr pane list",
            r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#,
        )
        .reply("herdr pane run", "{}")
        .reply("herdr agent start", "{}")
        .reply("herdr agent wait", "{}")
}

#[test]
fn two_orchestrators_on_one_fleet_both_act_each_named_in_the_log_and_the_gates_still_serialize() {
    let _env = ENV.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let c = two("orch-two");
    let r = Fake::default();
    let new = |o: &[&str]| run(&c, &r, &[&["thread", "new", "sendmeter", "Plan"], o].concat());
    assert_eq!(new(&["--orchestrator", "ada"]), (0, "t1\n".into()));
    assert_eq!(new(&["--orchestrator", "bob"]), (0, "t2\n".into()));
    assert_eq!(new(&[]).1, "t3\n", "unnamed: the first whose fleets cover it");
    let (code, out) = run(&c, &r, &["thread", "new", "all", "Plan", "--orchestrator", "ada"]);
    assert_eq!(code, 3, "Ada's fleets do not cover `all`: {out}");
    let of: Vec<_> = kinds(&c, "thread-new")
        .iter()
        .map(|t| t.str("orchestrator").map(String::from))
        .collect();
    assert_eq!(of, [Some("ada".into()), Some("bob".into()), Some("ada".into())]);

    for (id, n) in [("ada", "12"), ("bob", "13")] {
        let (code, out) = as_orch(&c, &r, id, &["queue", "sendmeter", "add", n]);
        assert_eq!(code, 0, "{out}");
    }
    let queued: Vec<_> = kinds(&c, "queue")
        .iter()
        .map(|e| (e.data["issues"].clone(), e.data["orchestrator"].clone()))
        .collect();
    assert_eq!(queued, [(json!([12]), json!("ada")), (json!([13]), json!("bob"))]);

    let f = spawnable();
    let (code, out) = as_orch(&c, &f, "ada", &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{out} {:?}", f.calls.borrow());
    let (code, out) = as_orch(&c, &spawnable(), "bob", &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED LANE_LIVE"), "{out}");
    let (code, out) = as_orch(&c, &spawnable(), "bob", &["spawn", "sendmeter", "13", "impl"]);
    assert_eq!(code, 0, "{out}");
    let spawns: Vec<_> = kinds(&c, "spawn")
        .iter()
        .map(|e| (e.issue, e.data["orchestrator"].clone()))
        .collect();
    assert_eq!(spawns, [(Some(12), json!("ada")), (Some(13), json!("bob"))]);
    assert_eq!(run(&c, &r, &["queue", "sendmeter", "add", "14"]).0, 0);
    assert_eq!(kinds(&c, "queue").last().unwrap().data.get("orchestrator"), None);

    let (_, ls) = run(&c, &r, &["thread", "ls", "sendmeter", "--json"]);
    let rows: Vec<Value> = serde_json::from_str(&ls).unwrap();
    let names: Vec<_> = rows.iter().map(|t| (t["thread"].clone(), t["name"].clone())).collect();
    assert_eq!(
        names,
        [
            (json!("t1"), json!("Ada")),
            (json!("t2"), json!("Bob")),
            (json!("t3"), json!("Ada"))
        ]
    );
}

#[test]
fn renaming_an_orchestrator_changes_every_label_and_no_other_config() {
    let _env = ENV.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let cfg = format!(
        "{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\n\n[orchestrator]\nname = \"Ada\"\n"
    );
    let c = ctx(&tmpdir("orch-rename"), &cfg);
    let r = Fake::default();
    assert_eq!(run(&c, &r, &["thread", "new", "sendmeter", "Grill"]).1, "t1\n");
    assert_eq!(run(&c, &r, &["thread", "reply", "t1", "Which scope?"]).0, 0);
    let ask = ["ask", "sendmeter", "-", "How fast?", "--option", "a", "--thread", "t1"];
    assert_eq!(run(&c, &r, &ask).1, "q1\n");
    let labels = |c: &herdsman::Ctx| {
        let show = run(c, &r, &["thread", "show", "t1"]).1;
        let ls = run(c, &r, &["thread", "ls"]).1;
        format!("{show}{ls}")
    };
    let before = labels(&c);
    for l in ["Ada: Which scope?", "Ada asks q1: How fast?", ", Ada "] {
        assert!(before.contains(l), "{l} in {before}");
    }
    assert!(
        !before.contains("conductor") && !before.contains("orchestrator"),
        "{before}"
    );

    let renamed = cfg.replace("name = \"Ada\"", "name = \"Bob\"");
    let changed: Vec<_> = cfg.lines().zip(renamed.lines()).filter(|(a, b)| a != b).collect();
    assert_eq!(changed, [("name = \"Ada\"", "name = \"Bob\"")], "one field");
    std::fs::write(&c.config, &renamed).unwrap();
    let after = labels(&c);
    assert_eq!(after, before.replace("Ada", "Bob"));
}
