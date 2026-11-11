//! #817: a conductor acts on design and architect reviews and on needs-human's [one more round] in its repo's thread:
//! the owner's typed line (`by: owner (typed)`) or a spec basis (`by: conductor-<t>`, an FYI line the owner may
//! [change] for 30 min; an extend's [change] parks). Another repo's conductor is refused. Its own test binary:
//! `HERDSMAN_LANE` and `HERDSMAN_THREAD` are process-wide.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use herdsman::serve::Server;
use serde_json::{json, Value};
use std::sync::{Mutex, MutexGuard};

const NOW: u64 = 1_000_000;
const ADDR: &str = "127.0.0.1:7777";
const SPEC: &str = "Split the sender.\n\nKeep the old sender behind a flag\nuntil the owner says so.";
const QUOTE: &str = "Keep the old sender behind a flag";

fn env() -> MutexGuard<'static, ()> {
    static ENV: Mutex<()> = Mutex::new(());
    ENV.lock().unwrap_or_else(|e| e.into_inner())
}

fn log(c: &herdsman::Ctx, ts: u64, fleet: &str, issue: Option<u64>, kind: &str, d: Value) {
    append(&c.state, &Event::new(ts, fleet, issue, kind, d)).unwrap();
}

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

fn at(c: &herdsman::Ctx, now: u64) -> herdsman::Ctx {
    herdsman::Ctx { now, ..c.clone() }
}

/// Fleets sendmeter (thread t1) and other (thread t2), each with a conductor; #36 on sendmeter.
fn setup(name: &str) -> herdsman::Ctx {
    let other = "[fleet.other]\nrepo = \"jirathip-dev/other\"\nlocal = \"/tmp/does-not-matter/other\"\n\
                 integration = \"staging\"\n\n[fleet.other.role.impl]\nharness = \"hermes\"\n\n\
                 [fleet.other.role.conductor]\nharness = \"claude\"\n";
    let conf = format!("{CONFIG}\n[fleet.sendmeter.role.conductor]\nharness = \"claude\"\n\n{other}");
    let c = ctx(&tmpdir(name), &conf);
    log(
        &c,
        NOW - 500,
        "sendmeter",
        None,
        "thread-new",
        json!({"thread": "t1", "title": "Run"}),
    );
    log(
        &c,
        NOW - 500,
        "other",
        None,
        "thread-new",
        json!({"thread": "t2", "title": "Other"}),
    );
    let d = json!({"role": "impl", "agent": "sendmeter-impl-36", "pane": "w1:p1"});
    log(&c, NOW - 400, "sendmeter", Some(36), "spawn", d);
    c
}

/// The owner's line `text` in thread `t`.
fn said(c: &herdsman::Ctx, ts: u64, t: &str, text: &str) {
    let fleet = if t == "t1" { "sendmeter" } else { "other" };
    log(
        c,
        ts,
        fleet,
        None,
        "thread-msg",
        json!({"thread": t, "from": "owner", "text": text}),
    );
}

/// #36's design ask q1 (no thread: the board card).
fn design_ask(c: &herdsman::Ctx, ts: u64) {
    let d = json!({"id": "q1", "question": "Design for #36 is ready (abc1234): approve, revise (type a note) or reject?",
        "options": ["approve", "revise", "reject"], "recommend": null, "kind": "owner", "design": true, "role": "design"});
    log(c, ts, "sendmeter", Some(36), "needs-decision", d);
}

fn needs_human(c: &herdsman::Ctx, ts: u64) {
    let d = json!({"code": "ROUND_LIMIT", "reason": "review rounds exceed 3"});
    log(c, ts, "sendmeter", Some(36), "needs-human", d);
}

/// The issue spec of #36, and herdr quiet.
fn host() -> Fake {
    let issue = json!({"title": "Split the sender", "body": SPEC, "updatedAt": "t1", "comments": []});
    Fake::default()
        .reply("gh issue view 36", &issue.to_string())
        .reply("herdr agent list", NO_AGENTS)
        .reply("herdr", "")
}

/// `args` run by thread `t`'s conductor (no thread: the owner) at `now`.
fn by(c: &herdsman::Ctx, t: Option<&str>, now: u64, args: &[&str]) -> (i32, String) {
    match t {
        Some(t) => {
            std::env::set_var("HERDSMAN_LANE", "sendmeter:-:conductor");
            std::env::set_var("HERDSMAN_THREAD", t);
        }
        None => {
            std::env::remove_var("HERDSMAN_LANE");
            std::env::remove_var("HERDSMAN_THREAD");
        }
    }
    let got = run(&at(c, now), &host(), args);
    std::env::remove_var("HERDSMAN_LANE");
    std::env::remove_var("HERDSMAN_THREAD");
    got
}

fn chat(c: &herdsman::Ctx, now: u64) -> String {
    let r = Fake::default();
    let c = at(c, now);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", now);
    let [_, _, chat] = parts(&s, ADDR, &cookie, now);
    chat
}

#[test]
fn a_typed_design_answer_needs_the_owners_line_in_the_conductors_thread_after_the_ask() {
    let _env = env();
    let c = setup("typed-design");
    said(&c, NOW - 300, "t1", "approve");
    design_ask(&c, NOW - 200);
    let typed = ["answer", "sendmeter", "q1", "--typed", "approve"];
    let (code, out) = by(&c, Some("t1"), NOW, &typed);
    assert!(
        code == 3 && out.contains("REFUSED NOT_TYPED: that line came before q1"),
        "{out}"
    );
    // Another repo's conductor, with the owner's line in its own thread.
    said(&c, NOW - 100, "t2", "approve");
    let (code, out) = by(&c, Some("t2"), NOW, &typed);
    assert!(code == 3 && out.contains("REFUSED NOT_TYPED"), "{out}");
    assert!(kinds(&c, "decision").is_empty() && kinds(&c, "design-approved").is_empty());

    said(&c, NOW - 50, "t1", "approve");
    let (code, out) = by(&c, Some("t1"), NOW, &typed);
    assert_eq!(code, 0, "{out}");
    let d = &kinds(&c, "decision")[0];
    assert_eq!((d.str("choice"), d.str("by")), (Some("approve"), Some("owner (typed)")));
    assert_eq!(kinds(&c, "design-approved")[0].str("by"), Some("owner (typed)"));
}

#[test]
fn a_typed_extend_grants_one_round_after_the_card_only() {
    let _env = env();
    let c = setup("typed-extend");
    said(&c, NOW - 300, "t1", "one more round");
    needs_human(&c, NOW - 200);
    let typed = ["extend", "sendmeter", "36", "--no-spawn", "--typed", "one more round"];
    let (code, out) = by(&c, Some("t1"), NOW, &typed);
    assert!(
        code == 3 && out.contains("REFUSED NOT_TYPED"),
        "a line before the card: {out}"
    );
    said(&c, NOW - 100, "t2", "one more round");
    let (code, out) = by(&c, Some("t2"), NOW, &typed);
    assert!(
        code == 3 && out.contains("REFUSED OWNER_ASK: #36 is not on t2's repo"),
        "{out}"
    );
    let (code, out) = by(&c, Some("t1"), NOW, &["extend", "sendmeter", "36", "--no-spawn"]);
    assert!(
        code == 3 && out.contains("REFUSED OWNER_ASK: no --basis"),
        "neither: {out}"
    );
    assert!(kinds(&c, "round-extended").is_empty());

    said(&c, NOW - 50, "t1", "one more round");
    let (code, out) = by(&c, Some("t1"), NOW, &typed);
    assert_eq!(code, 0, "{out}");
    let ext = kinds(&c, "round-extended");
    assert_eq!((ext.len(), ext[0].str("by")), (1, Some("owner (typed)")));
    let (code, out) = by(&c, Some("t1"), NOW, &typed);
    assert!(code == 3 && out.contains("REFUSED NOT_NEEDS_HUMAN"), "{out}");
    assert_eq!(kinds(&c, "round-extended").len(), 1);
}

#[test]
fn a_basis_answers_a_design_ask_with_an_fyi_and_refuses_a_non_quote_or_owner_topic() {
    let _env = env();
    let c = setup("basis-design");
    design_ask(&c, NOW - 200);
    let (code, out) = by(
        &c,
        Some("t2"),
        NOW,
        &["answer", "sendmeter", "q1", "1", "--basis", QUOTE],
    );
    assert!(
        code == 3 && out.contains("REFUSED OWNER_ASK: q1 is no lane ask on t2's repo"),
        "{out}"
    );
    let (code, out) = by(
        &c,
        Some("t1"),
        NOW,
        &["answer", "sendmeter", "q1", "revise: drop the main page"],
    );
    assert!(
        code == 3 && out.contains("REFUSED OWNER_ASK: it names an owner topic"),
        "{out}"
    );
    let (code, out) = by(
        &c,
        Some("t1"),
        NOW,
        &["answer", "sendmeter", "q1", "1", "--basis", "Keep the new sender"],
    );
    assert!(
        code == 3 && out.contains("REFUSED OWNER_ASK: --basis is neither"),
        "{out}"
    );
    assert!(kinds(&c, "decision").is_empty());
    let up = kinds(&c, "ask-owner");
    assert_eq!(
        (up.len(), up[0].str("thread"), up[0].str("id")),
        (1, Some("t1"), Some("q1"))
    );

    let (code, out) = by(
        &c,
        Some("t1"),
        NOW,
        &["answer", "sendmeter", "q1", "1", "--basis", QUOTE],
    );
    assert_eq!((code, out.as_str()), (0, "sendmeter q1: approve\n"));
    let d = &kinds(&c, "decision")[0];
    assert_eq!((d.str("by"), d.str("basis")), (Some("conductor-t1"), Some(QUOTE)));
    assert_eq!(kinds(&c, "design-approved")[0].str("by"), Some("conductor-t1"));
    let fyi = kinds(&c, "thread-msg").pop().unwrap();
    assert_eq!(
        (fyi.str("thread"), fyi.str("text"), fyi.str("change")),
        (
            Some("t1"),
            Some(&*format!("answered q1 for #36: approve, because {QUOTE}")),
            Some("q1")
        )
    );
}

#[test]
fn a_basis_extend_posts_an_fyi_whose_change_parks_within_30_minutes_only() {
    let _env = env();
    let c = setup("basis-extend");
    needs_human(&c, NOW - 200);
    let ext = |b: &str| {
        by(
            &c,
            Some("t1"),
            NOW,
            &["extend", "sendmeter", "36", "--no-spawn", "--basis", b],
        )
    };
    let (code, out) = ext("Keep the new sender");
    assert!(
        code == 3 && out.contains("REFUSED OWNER_ASK: --basis is neither"),
        "{out}"
    );
    let (code, out) = by(&c, Some("t2"), NOW, &["extend", "sendmeter", "36", "--basis", QUOTE]);
    assert!(
        code == 3 && out.contains("REFUSED OWNER_ASK: #36 is not on t2's repo"),
        "{out}"
    );
    let up = kinds(&c, "ask-owner");
    assert_eq!(
        (up.len(), up[0].str("thread"), up[0].data["recommend"].clone()),
        (1, Some("t1"), json!(1))
    );
    assert!(kinds(&c, "round-extended").is_empty());

    let (code, out) = ext(QUOTE);
    assert_eq!(code, 0, "{out}");
    let r = kinds(&c, "round-extended");
    assert_eq!((r.len(), r[0].str("by")), (1, Some("conductor-t1")));
    let fyi = kinds(&c, "thread-msg").pop().unwrap();
    assert_eq!(
        (fyi.str("thread"), fyi.str("text")),
        (
            Some("t1"),
            Some(&*format!("answered q1 for #36: one more round, because {QUOTE}"))
        )
    );
    assert!(chat(&c, NOW + 60).contains("<form class=chq data-id=\"q1\""));
    assert!(
        !chat(&c, NOW + 30 * 60).contains("class=chg"),
        "no [change] after 30 min"
    );
    let (code, out) = by(&c, None, NOW + 30 * 60, &["answer", "sendmeter", "q1", "park"]);
    assert!(code == 3 && out.contains("REFUSED ALREADY_ANSWERED"), "{out}");
    assert!(kinds(&c, "parked").is_empty());

    let (code, out) = by(&c, None, NOW + 29 * 60, &["answer", "sendmeter", "q1", "park"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(kinds(&c, "parked")[0].str("by"), Some("owner"));
    let st = herdsman::events::fold(&read(&c.state).unwrap());
    assert_eq!(st[&("sendmeter".to_string(), 36)].state, "parked");
}
