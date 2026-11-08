//! #302: a role's `fallback` chain: an unavailable agent rests, the next `harness[:account]` carries on.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use serde_json::{json, Value};
use std::path::Path;

/// Claude impl role with `role_extra` lines, `[accounts]` A/B (tokens in `fake`'s store), local repo `<dir>/repo`.
fn config(d: &Path, role_extra: &str) -> String {
    let repo = d.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    CONFIG
        .replace(
            "harness  =  \"hermes\"    # toggled in tests",
            &format!("harness = \"claude\"\n{role_extra}"),
        )
        .replace("/tmp/does-not-matter/sendmeter", repo.to_str().unwrap())
        + "\n[accounts]\nA = \"CLAUDE_CODE_OAUTH_TOKEN_A\"\nB = \"CLAUDE_CODE_OAUTH_TOKEN_B\"\n"
}

const CHAIN: &str = "fallback = [\"claude:A\", \"claude:B\", \"dsh\"]";

/// Faked herdr/gh/git for a real impl spawn of issue `n` in pane `w9:p<n>`.
fn fake(c: &herdsman::Ctx, n: u64) -> Fake {
    let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
    std::fs::create_dir_all(&wt).unwrap();
    let pane = format!("w9:p{n}");
    Fake::default()
        .stored("herdsman/all/CLAUDE_CODE_OAUTH_TOKEN_A", "a")
        .stored("herdsman/all/CLAUDE_CODE_OAUTH_TOKEN_B", "b")
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
    // #640: an account's sign-out is its own state, without a reset time.
    let u = ev.iter().rfind(|e| e.kind == "account-signed-out").unwrap();
    assert_eq!(
        (u.str("account"), u.str("agent")),
        (Some("A"), Some("sendmeter-impl-12"))
    );
    assert!(u.data["until"].is_null());

    spawn(&c, 12, &["--supersede"]);
    let brief = std::fs::read_to_string(c.worktrees.join("sendmeter/issue-12/.brief.md")).unwrap();
    assert!(
        brief.contains(HONEST) && brief.contains("Continue; don't redo."),
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
    let ev: Vec<_> = read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| !["spawn-started", "intent", "failed"].contains(&e.kind.as_str())) // #509: around each try
        .collect();
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
    // No chain: a cap is the account cap it always was; #640: a sign-out is its own state.
    assert!(check(&c, "Please run /login").contains("account-signed-out"));
    assert!(check(&c, "Claude usage limit reached.").contains("account-capped"));
    assert_eq!(dry(&c, 13), (json!("claude"), json!("B")));
    spawn(&c, 12, &["--supersede"]);
    let brief = std::fs::read_to_string(c.worktrees.join("sendmeter/issue-12/.brief.md")).unwrap();
    // #353: a superseded lane's successor still gets the handover note.
    assert!(brief.contains(HONEST), "{brief}");
    let ev = read(&c.state).unwrap();
    assert!(!ev
        .iter()
        .any(|e| matches!(e.kind.as_str(), "fallback" | "agent-unavailable")));
    assert!(ev.iter().rfind(|e| e.kind == "spawn").unwrap().data["resting"].is_null());
}

/// #353: the sentence every handover note carries.
const HONEST: &str = "Summary of the previous lane; its tool state and approvals are not carried over.";

/// The brief of issue 12's lane.
fn brief12(c: &herdsman::Ctx) -> String {
    std::fs::read_to_string(c.worktrees.join("sendmeter/issue-12/.brief.md")).unwrap()
}

/// Lane `sendmeter-impl-12` signed out (via `herdsman check`), then superseded by the next entry with `r` as the fake.
fn sign_out_and_supersede(c: &herdsman::Ctx, r: &Fake) {
    assert!(check(c, "API Error: 401 · Please run /login").contains("signed out"));
    let (code, out) = run(c, r, &["spawn", "sendmeter", "12", "impl", "--supersede"]);
    assert_eq!(code, 0, "{out}");
}

fn ask(c: &herdsman::Ctx, kind: &str, mut d: Value) {
    if kind == "needs-decision" {
        d["agent"] = json!("sendmeter-impl-12");
        d["recommend"] = json!(1);
        d["options"] = json!(["a", "b"]);
    }
    append(&c.state, &Event::new(c.now, "sendmeter", Some(12), kind, d)).unwrap();
}

#[test]
fn the_fallback_brief_carries_a_capped_handover_from_the_transcript() {
    let d = tmpdir("ho-transcript");
    let c = ctx(&d, &config(&d, CHAIN));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, 12, &[]);
    let wt = c.worktrees.join("sendmeter/issue-12");
    let dir = c.home.join(".claude/projects").join(
        wt.to_str()
            .unwrap()
            .replace(|ch: char| !ch.is_ascii_alphanumeric(), "-"),
    );
    std::fs::create_dir_all(&dir).unwrap();
    let (long, exact) = (format!("Pushed   the\n\nfix. {}", "x".repeat(300)), "y".repeat(240));
    let rows = [
        json!({"type": "user", "message": {"content": "Read the brief\n  and execute it."}}),
        json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Bash", "input": {"command": "cargo test  -q"}}]}}),
        json!({"type": "user", "message": {"content": exact}}),
        json!({"type": "assistant", "message": {"content": [{"type": "text", "text": long}]}}),
    ];
    let jsonl: String = rows.iter().map(|r| format!("{r}\n")).collect();
    std::fs::write(dir.join("s-1.jsonl"), jsonl).unwrap();
    ask(&c, "needs-decision", json!({"id": "q1", "question": "which way?"}));
    ask(&c, "decision", json!({"id": "q1", "choice": "a", "by": "owner"}));
    let diff = format!("git -C {} diff --name-only origin/staging", wt.display());
    sign_out_and_supersede(&c, &fake(&c, 12).once(&diff, 0, "src/a.rs\nsrc/b.rs\n"));

    let brief = brief12(&c);
    let note = &brief[brief.find("Handover:").expect(&brief)..];
    let capped = format!("- Pushed the fix. {}...", "x".repeat(237 - "Pushed the fix. ".len()));
    let want = [
        &format!("Handover: {HONEST}"),
        "Issue: t",
        "Prompts:\n- Read the brief and execute it.",
        &format!("- {exact}"),
        &format!("Final replies:\n{capped}"),
        "Commands run:\n- cargo test -q",
        "Files changed:\n- src/a.rs\n- src/b.rs",
        "Asks and answers:\n- sendmeter #12 needs you (sendmeter-impl-12 asks q1): which way? 1) a (recommended) 2) b",
        "sendmeter #12 decision q1 by owner: a",
    ];
    for w in want {
        assert!(note.contains(w), "{w:?} not in {note}");
    }
    let order: Vec<usize> = [
        "Issue:",
        "Prompts:",
        "Final replies:",
        "Commands run:",
        "Files changed:",
        "Asks and answers:",
    ]
    .iter()
    .map(|h| note.find(h).unwrap())
    .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{note}");
    assert!(note.lines().all(|l| l.chars().count() <= 242), "{note}");
    // The note is logged on the spawn: the next handover lists it among the prior notes.
    let ev = read(&c.state).unwrap();
    let sp = ev.iter().rfind(|e| e.kind == "spawn").unwrap();
    assert!(sp.str("handover").unwrap().starts_with("Handover: "), "{sp:?}");
}

#[test]
fn without_a_transcript_the_handover_reads_the_pane_log() {
    let d = tmpdir("ho-panelog");
    let c = ctx(&d, &config(&d, CHAIN));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, 12, &[]);
    let log = herdsman::lanes::log(&c.state, "sendmeter-impl-12").unwrap();
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(
        &log,
        "❯ Read the brief\n⏺ Bash(cargo build)\n  ⎿ ok\n\n⏺ Built it\n  and pushed.\n\n❯ \n",
    )
    .unwrap();
    // check rolls the pane into the log too; the lines above are what it had before.
    assert!(check(&c, "API Error: 401 · Please run /login").contains("signed out"));
    let (code, out) = run(&c, &fake(&c, 12), &["spawn", "sendmeter", "12", "impl", "--supersede"]);
    assert_eq!(code, 0, "{out}");
    let brief = brief12(&c);
    for w in [
        HONEST,
        "Prompts:\n- Read the brief\n",
        "Final replies:\n- Built it and pushed.",
        "Commands run:\n- cargo build",
    ] {
        assert!(brief.contains(w), "{w:?} not in {brief}");
    }
}

#[test]
fn an_entry_that_ran_before_gets_only_the_delta() {
    let d = tmpdir("ho-delta");
    let mut c = ctx(&d, &config(&d, CHAIN));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, 12, &[]); // claude:A
    ask(&c, "needs-decision", json!({"id": "early", "question": "q?"}));
    c.now += 60;
    sign_out_and_supersede(&c, &fake(&c, 12)); // claude:B: A never handed over before, the full note
    let full = brief12(&c);
    assert!(
        full.contains("asks early") && !full.contains("Only what happened since"),
        "{full}"
    );

    c.now += 4000; // #640: A was signed in again; B goes unavailable
    assert_eq!(run(&c, &Fake::default(), &["accounts", "reset", "A"]).0, 0);
    ask(&c, "needs-decision", json!({"id": "late", "question": "q?"}));
    ask(
        &c,
        "agent-unavailable",
        json!({"agent": "sendmeter-impl-12", "harness": "claude", "account": "B", "until": c.now + 99_999}),
    );
    let (code, out) = run(&c, &fake(&c, 12), &["spawn", "sendmeter", "12", "impl", "--supersede"]);
    assert_eq!(code, 0, "{out}");
    let ev = read(&c.state).unwrap();
    assert_eq!(
        ev.iter().rfind(|e| e.kind == "spawn").unwrap().str("account"),
        Some("A")
    );
    let delta = brief12(&c);
    assert!(delta.contains(HONEST), "{delta}");
    assert!(
        delta.contains("Only what happened since claude:A's last lane on this issue was replaced"),
        "{delta}"
    );
    assert!(delta.contains("asks late") && !delta.contains("asks early"), "{delta}");
}
