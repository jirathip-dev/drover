mod common;
use common::*;
use herdsman::events::{append, read, Event};
use serde_json::{json, Value};

const NOW: u64 = 1_000_000;

fn log(c: &herdsman::Ctx, ts: u64, issue: Option<u64>, kind: &str, d: Value) {
    append(&c.state, &Event::new(ts, "sendmeter", issue, kind, d)).unwrap();
}

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

fn design_ask(name: &str, line: &str) -> herdsman::Ctx {
    let c = ctx(&tmpdir(name), CONFIG);
    log(
        &c,
        NOW - 200,
        None,
        "thread-new",
        json!({"thread": "t1", "title": "t1"}),
    );
    let d = json!({"id": "q1", "question": "Design for #60 is ready: approve, revise or reject?",
        "options": ["approve", "revise", "reject"], "recommend": null, "kind": "owner", "design": true, "thread": "t1", "role": "design"});
    log(&c, NOW - 90, Some(60), "needs-decision", d);
    let said = json!({"thread": "t1", "from": "owner", "text": line});
    log(&c, NOW - 60, None, "thread-msg", said);
    c
}

fn edited(r: &Fake) -> Option<String> {
    let calls = r.calls.borrow();
    let edit = calls.iter().rfind(|c| c.starts_with("gh issue edit 60 "))?;
    Some(edit.split_once(" --body ")?.1.to_string())
}

#[test]
fn a_typed_approve_with_a_note_approves_and_writes_the_note_under_amendments() {
    let line = "Approve with note for me";
    let c = design_ask("typed-approve", line);
    let note = "keep the meter grey";
    let r = Fake::default()
        .reply("gh issue view 60", &json!({"body": "Draw the meter.\n"}).to_string())
        .reply("gh issue edit 60", "");
    let args = ["answer", "sendmeter", "q1", "approve", "--typed", line, "--note", note];
    let (code, out) = run(&c, &r, &args);
    assert_eq!(code, 0, "{out}");
    assert!(kinds(&c, "design-revise").is_empty(), "approve is no revise");
    assert_eq!(kinds(&c, "design-approved").len(), 1);
    let d = &kinds(&c, "decision")[0];
    assert_eq!(
        (d.str("choice"), d.str("by"), d.str("note")),
        (Some("approve"), Some("owner (typed)"), Some(note))
    );
    assert_eq!(
        edited(&r).as_deref(),
        Some(format!("Draw the meter.\n\n## Amendments\n- **Amendment (q1)**: {note}\n").as_str())
    );
}

#[test]
fn a_typed_revise_takes_its_note_and_free_text_alone_stays_revise() {
    let line = "Not yet, see my note";
    let c = design_ask("typed-revise", line);
    let note = "make the needle thinner";
    let args = ["answer", "sendmeter", "q1", "2", "--typed", line, "--note", note];
    let (code, out) = run(&c, &Fake::default(), &args);
    assert_eq!(code, 0, "{out}");
    assert_eq!(kinds(&c, "design-revise")[0].str("note"), Some(note));

    let line = "make it blue";
    let c = design_ask("typed-free", line);
    let (code, out) = run(&c, &Fake::default(), &["answer", "sendmeter", "q1", "--typed", line]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(kinds(&c, "design-revise")[0].str("note"), Some(line));
}

#[test]
fn an_ask_answered_by_the_owner_typed_is_an_amend_basis() {
    let c = ctx(&tmpdir("typed-amend"), CONFIG);
    log(&c, 1, Some(60), "needs-decision", json!({"id": "q1", "question": "?"}));
    log(
        &c,
        2,
        Some(60),
        "decision",
        json!({"id": "q1", "choice": "yes", "by": "owner (typed)"}),
    );
    let r = Fake::default()
        .reply("gh issue view 60", &json!({"body": "Tidy the tab.\n"}).to_string())
        .reply("gh issue edit 60", "");
    let (code, out) = run(
        &c,
        &r,
        &["amend", "sendmeter", "60", "--basis", "q1", "scope: tab only"],
    );
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        edited(&r).as_deref(),
        Some("Tidy the tab.\n\n## Amendments\n- **Amendment (q1)**: scope: tab only\n")
    );
}

#[test]
fn a_typed_reject_with_a_note_rejects_and_keeps_the_note() {
    let line = "No, start over";
    let c = design_ask("typed-reject", line);
    let pre = "a".repeat(40);
    let lane = json!({"agent": "sendmeter-design-60", "pane": "p1", "worktree": "/w", "branch": "issue-60"});
    log(&c, NOW - 80, Some(60), "spawn", lane);
    let review = json!({"agent": "sendmeter-design-60", "pre": pre, "adr": null});
    log(&c, NOW - 70, Some(60), "design-review", review);
    let note = "wrong direction";
    let r = Fake::default().reply("git", "b");
    let args = ["answer", "sendmeter", "q1", "reject", "--typed", line, "--note", note];
    let (code, out) = run(&c, &r, &args);
    assert_eq!(code, 0, "{out}");
    assert!(kinds(&c, "design-revise").is_empty(), "reject is no revise");
    assert_eq!(kinds(&c, "design-rejected")[0].str("pre"), Some(pre.as_str()));
    let d = &kinds(&c, "decision")[0];
    assert_eq!((d.str("choice"), d.str("note")), (Some("reject"), Some(note)));
}
