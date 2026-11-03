//! #302: a role's `fallback` chain: an unavailable agent rests, the next `harness[:account]` carries on.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use serde_json::{json, Value};
use std::path::Path;

/// Claude impl role with `role_extra` lines, `[accounts]` A/B from `<dir>/tokens.env`, local repo `<dir>/repo`.
fn config(d: &Path, role_extra: &str) -> String {
    let env = d.join("tokens.env");
    std::fs::write(&env, "CLAUDE_CODE_OAUTH_TOKEN_A=a\nCLAUDE_CODE_OAUTH_TOKEN_B=b\n").unwrap();
    let repo = d.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    CONFIG
        .replace("harness  =  \"hermes\"    # toggled in tests", &format!("harness = \"claude\"\n{role_extra}"))
        .replace("/tmp/does-not-matter/sendmeter", repo.to_str().unwrap())
        + &format!(
            "\n[accounts]\naccounts_env = \"{}\"\nA = \"CLAUDE_CODE_OAUTH_TOKEN_A\"\nB = \"CLAUDE_CODE_OAUTH_TOKEN_B\"\n",
            env.display()
        )
}

const CHAIN: &str = "fallback = [\"claude:A\", \"claude:B\", \"dsh\"]";

/// Faked herdr/gh/git for a real impl spawn of issue `n` in pane `w9:p<n>`.
fn fake(c: &herdsman::Ctx, n: u64) -> Fake {
    let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
    std::fs::create_dir_all(&wt).unwrap();
    let pane = format!("w9:p{n}");
    Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .reply(&format!("gh issue view {n}"), r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "")
        .reply(
            "herdr worktree open",
            r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#,
        )
        .reply(
            "herdr pane list --workspace w9",
            &format!(r#"{{"result":{{"panes":[{{"pane_id":"{pane}","agent":null}}]}}}}"#),
        )
        .reply(&format!("herdr pane run {pane} export"), "{}")
        .reply("herdr agent start", "{}")
        .reply("herdr agent wait", "{}")
}

/// A real (faked) impl spawn of issue `n` that succeeds.
fn spawn(c: &herdsman::Ctx, n: u64, extra: &[&str]) {
    let r = fake(c, n);
    let n = n.to_string();
    let (code, out) = run(c, &r, &[&["spawn", "sendmeter", &n, "impl"][..], extra].concat());
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
}

/// The dry-run plan of an impl spawn of issue `n`: (harness, account).
fn dry(c: &herdsman::Ctx, n: u64) -> (Value, Value) {
    let args = ["spawn", "sendmeter", &n.to_string(), "impl", "--dry-run", "--json"];
    let (code, out) = run(c, &Fake::default(), &args);
    assert_eq!(code, 0, "{out}");
    let p: Value = serde_json::from_str(&out).unwrap();
    (p["harness"].clone(), p["account"].clone())
}

/// `herdsman check` with lane `sendmeter-impl-12` (pane `w9:p12`) showing `screen`.
fn check(c: &herdsman::Ctx, screen: &str) -> String {
    let wt = c.worktrees.join("sendmeter/issue-12");
    let r = Fake::default()
        .reply(
            "herdr agent list",
            &agents(&[("sendmeter-impl-12", "idle", wt.to_str().unwrap(), "w9:p12")]),
        )
        .reply("herdr pane read w9:p12", screen);
    let (code, out) = run(c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    out
}

fn rest(c: &herdsman::Ctx, d: Value) {
    append(&c.state, &Event::new(c.now, "sendmeter", None, "agent-unavailable", d)).unwrap();
}

#[test]
fn usage_limit_rests_a_and_the_next_spawn_uses_b() {
    let d = tmpdir("fb-usage");
    let c = ctx(&d, &config(&d, CHAIN));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, 12, &[]);
    assert_eq!(read(&c.state).unwrap().last().unwrap().str("account"), Some("A"));
    assert!(check(&c, "Claude usage limit reached.").contains("account-capped"));
    assert_eq!(dry(&c, 13), (json!("claude"), json!("B")));
}

#[test]
fn all_claude_accounts_resting_carries_on_with_the_next_harness() {
    let d = tmpdir("fb-all");
    let c = ctx(&d, &config(&d, CHAIN));
    for a in ["A", "B"] {
        rest(&c, json!({"harness": "claude", "account": a, "until": c.now + 600}));
    }
    assert_eq!(dry(&c, 13), (json!("dsh"), Value::Null));
    // The dsh harness itself resting too: nothing is left.
    rest(&c, json!({"harness": "dsh", "account": null, "until": c.now + 600}));
    let (code, out) = run(&c, &Fake::default(), &["spawn", "sendmeter", "13", "impl", "--dry-run"]);
    assert!(code == 3 && out.contains("ALL_CAPPED"), "{code} {out}");
}

#[test]
fn a_rest_that_expired_goes_back_to_a_at_the_next_round() {
    let d = tmpdir("fb-back");
    let mut c = ctx(&d, &config(&d, CHAIN));
    rest(&c, json!({"harness": "claude", "account": "A", "until": c.now + 600}));
    assert_eq!(dry(&c, 13), (json!("claude"), json!("B")));
    c.now += 601;
    assert_eq!(dry(&c, 13), (json!("claude"), json!("A")));
}

#[test]
fn a_signed_out_lane_rests_and_its_successor_gets_the_carry_over() {
    let d = tmpdir("fb-carry");
    let c = ctx(&d, &config(&d, CHAIN));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, 12, &[]);
    let out = check(&c, "API Error: 401 · Please run /login");
    assert!(out.contains("signed out"), "{out}");
    let ev = read(&c.state).unwrap();
    let u = ev.iter().rfind(|e| e.kind == "agent-unavailable").unwrap();
    assert_eq!(
        (u.str("account"), u.str("agent")),
        (Some("A"), Some("sendmeter-impl-12"))
    );
    assert!(u.data["until"].as_u64().unwrap() > c.now);

    spawn(&c, 12, &["--supersede"]);
    let brief = std::fs::read_to_string(c.worktrees.join("sendmeter/issue-12/.brief.md")).unwrap();
    assert!(
        brief.contains("Carry-over:") && brief.contains("Continue; don't redo."),
        "{brief}"
    );
    let ev = read(&c.state).unwrap();
    let f = ev.iter().rfind(|e| e.kind == "fallback").unwrap();
    assert_eq!((f.str("from"), f.str("to")), (Some("claude:A"), Some("claude:B")));
    let sp = ev.iter().rfind(|e| e.kind == "spawn").unwrap();
    assert!(
        sp.str("resting")
            .unwrap()
            .starts_with("on claude:B (claude:A resting until "),
        "{sp:?}"
    );
}

#[test]
fn a_harness_that_wont_start_twice_rests() {
    let d = tmpdir("fb-start");
    let c = ctx(&d, &config(&d, CHAIN));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    for _ in 0..2 {
        let r = fake(&c, 12).once_err("herdr agent start", 1, "claude: command not found");
        assert_eq!(run(&c, &r, &["spawn", "sendmeter", "12", "impl"]).0, 1);
    }
    let ev = read(&c.state).unwrap();
    let kinds: Vec<_> = ev.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(kinds, ["spawn-failed", "agent-unavailable"]);
    assert_eq!(
        (ev[1].str("account"), ev[1].str("reason")),
        (Some("A"), Some("won't start"))
    );
    assert_eq!(dry(&c, 12), (json!("claude"), json!("B")));
}

#[test]
fn default_config_is_unchanged() {
    let d = tmpdir("fb-default");
    let c = ctx(&d, &config(&d, "accounts = [\"A\", \"B\"]"));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    assert_eq!(dry(&c, 13), (json!("claude"), json!("A")));
    spawn(&c, 12, &[]);
    // No chain: a sign-out is not detected, and a cap is the account cap it always was.
    assert_eq!(check(&c, "Please run /login"), "");
    assert!(check(&c, "Claude usage limit reached.").contains("account-capped"));
    assert_eq!(dry(&c, 13), (json!("claude"), json!("B")));
    spawn(&c, 12, &["--supersede"]);
    let brief = std::fs::read_to_string(c.worktrees.join("sendmeter/issue-12/.brief.md")).unwrap();
    assert!(!brief.contains("Carry-over:"), "{brief}");
    let ev = read(&c.state).unwrap();
    assert!(!ev
        .iter()
        .any(|e| matches!(e.kind.as_str(), "fallback" | "agent-unavailable")));
    assert!(ev.iter().rfind(|e| e.kind == "spawn").unwrap().data["resting"].is_null());
}
