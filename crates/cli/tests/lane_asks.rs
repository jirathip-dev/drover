//! #541: a lane's ask goes to its repo's conductor thread first (the pinned one, else the newest; none: the owner);
//! the conductor answers only with a basis and never an owner topic; its answer posts an FYI line the owner may
//! [change] for 30 min. Its own test binary: `HERDSMAN_LANE`, `HERDSMAN_THREAD` and `PATH` are process-wide.
mod common;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use common::*;
use herdsman::events::{append, read, Event};
use herdsman::serve::Server;
use herdsman::webpush;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Mutex, MutexGuard};

const NOW: u64 = 1_000_000;
const ADDR: &str = "127.0.0.1:7777";
const UA_PRIVATE: &str = "q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94";
const LANE: &str = "sendmeter-impl-36";
const SPEC: &str = "Split the sender.\n\nKeep the old sender behind a flag\nuntil the owner says so.";

/// Held for the test: the env this binary's tests set is shared.
fn env() -> MutexGuard<'static, ()> {
    static ENV: Mutex<()> = Mutex::new(());
    ENV.lock().unwrap_or_else(|e| e.into_inner())
}

fn as_lane() {
    std::env::set_var("HERDSMAN_LANE", "sendmeter:36:impl");
    std::env::remove_var("HERDSMAN_THREAD");
}

fn as_orchestrator(t: &str) {
    std::env::set_var("HERDSMAN_LANE", "sendmeter:-:orchestrator");
    std::env::set_var("HERDSMAN_THREAD", t);
}

fn as_owner() {
    std::env::remove_var("HERDSMAN_LANE");
    std::env::remove_var("HERDSMAN_THREAD");
}

fn at(c: &herdsman::Ctx, now: u64) -> herdsman::Ctx {
    herdsman::Ctx { now, ..c.clone() }
}

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

fn setup(name: &str, extra: &str, threads: &[&str]) -> (herdsman::Ctx, FakeShed) {
    let d = tmpdir(name);
    let conf = CONFIG.replace(
        "integration = \"staging\"",
        &format!("integration = \"staging\"\n{extra}"),
    );
    let conf = format!(
        "{conf}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\naccount = \"C\"\n\n[[notify]]\ntype = \"webpush\"\nsub = \"mailto:owner@example.com\"\n"
    );
    let sh = FakeShed::new();
    let conf = format!(
        "{conf}\n[fleet.sendmeter.runtime]\nkind = \"shed-acp\"\nsocket = \"{}\"\n\n[accounts]\nC = \"C\"\n",
        sh.sock.display()
    );
    let mut c = ctx(&d, &conf);
    c.exe = env!("CARGO_BIN_EXE_herdsman").into();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    sh.lane(LANE, "idle", "/x");
    let bin = d.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let script = format!(
        "#!/bin/sh\ncat > /dev/null\necho \"$@\" >> '{0}/calls'\necho 201\n",
        d.display()
    );
    std::fs::write(bin.join("curl"), script).unwrap();
    std::fs::set_permissions(bin.join("curl"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{path}", bin.display()));
    let ua = p256::SecretKey::from_slice(&B64.decode(UA_PRIVATE).unwrap()).unwrap();
    let f = [
        ("endpoint", "https://push.example/ep".to_string()),
        ("p256dh", B64.encode(webpush::public(&ua))),
        ("auth", "BTBZMqHH6r4Tts7J_aSIgg".into()),
    ];
    let f = f.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    webpush::subscribe(&c.state, &f, "Mac", 1, None).unwrap();
    as_owner();
    for t in threads {
        assert_eq!(run(&c, &Fake::default(), &["thread", "new", "sendmeter", t]).0, 0);
    }
    let d = json!({"role": "impl", "agent": LANE, "pane": LANE});
    append(&c.state, &Event::new(NOW - 60, "sendmeter", Some(36), "spawn", d)).unwrap();
    (c, sh)
}

/// Web pushes sent so far.
fn pushes(c: &herdsman::Ctx) -> usize {
    let calls = c.state.parent().unwrap().parent().unwrap().join("calls");
    std::fs::read_to_string(calls).unwrap_or_default().lines().count()
}

fn host() -> Fake {
    let issue = json!({"title": "Split the sender", "body": SPEC, "updatedAt": "t1", "comments": [
        {"body": "**Amendment** no new dependency for it", "authorAssociation": "OWNER", "url": "u1",
         "author": {"login": "owner"}, "createdAt": "t1"}]});
    let agents = json!([{"id": "claude-acp", "version": "0.101.0", "distribution": "npx", "cmd": ["echo"]}]);
    Fake::default()
        .reply("shed agents --json", &agents.to_string())
        .reply("shed run", "")
        .reply("shed send", "")
        .reply("gh issue view 36", &issue.to_string())
}

/// The lane's ask `q`, its id.
fn lane_ask(c: &herdsman::Ctx, sh: &FakeShed, q: &str) -> String {
    as_lane();
    let args = [
        "ask",
        "sendmeter",
        "36",
        q,
        "--option",
        "yes",
        "--option",
        "no",
        "--recommend",
        "2",
    ];
    let (code, out) = run(c, &sh.cli(&host()), &args);
    assert_eq!(code, 0, "{out}");
    out.trim().into()
}

fn orchestrator_answer(c: &herdsman::Ctx, sh: &FakeShed, t: &str, id: &str, n: &str, basis: &str) -> (i32, String) {
    as_orchestrator(t);
    let (code, out) = run(c, &sh.cli(&host()), &["answer", "sendmeter", id, n, "--basis", basis]);
    as_owner();
    (code, out)
}

fn typed(sh: &FakeShed, text: &str) -> bool {
    let enter = sh
        .all("keys")
        .iter()
        .any(|p| p["name"] == LANE && p["keys"] == json!(["Enter"]));
    sh.sent(LANE).iter().any(|t| t == text) && enter
}

/// The [chat] tab as the board draws it at `now`.
fn chat(c: &herdsman::Ctx, now: u64) -> String {
    let r = Fake::default();
    let c = at(c, now);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", now);
    let [_, _, chat] = parts(&s, ADDR, &cookie, now);
    chat
}

#[test]
fn a_lane_ask_wakes_the_newest_thread_and_stays_open_without_a_push() {
    let _env = env();
    let (c, sh) = setup("lane-ask-newest", "", &["Old", "New"]);
    assert_eq!(lane_ask(&c, &sh, "Split it?"), "q1");
    let a = &kinds(&c, "needs-decision")[0];
    assert_eq!((a.str("routed"), a.str("thread")), (Some("t2"), None));
    let thread = c.state.with_file_name("orchestrator").join("t2/.thread.md");
    assert_eq!(
        std::fs::read_to_string(thread).unwrap(),
        "system: lane ask q1 on #36 from sendmeter-impl-36 (owner): Split it? [1] yes [2] no (recommended). \
         Answer it only if the issue spec or a past decision settles it: `herdsman answer sendmeter q1 <n> --basis \
         \"<verbatim spec quote | q<id>>\"`; else run it without --basis and it goes to the owner.\n"
    );
    let st = &kinds(&c, "orchestrator-start")[0];
    assert_eq!((st.str("thread"), st.str("runtime")), (Some("t2"), Some("shed-acp")));
    let (_, asks) = run(&c, &Fake::default(), &["asks", "sendmeter", "--json"]);
    let asks: serde_json::Value = serde_json::from_str(&asks).unwrap();
    assert_eq!(
        (asks[0]["id"].clone(), asks[0]["closed"].clone()),
        (json!("q1"), json!(null))
    );
    assert_eq!(pushes(&c), 0, "a routed ask does not push");
    let chat = chat(&c, NOW + 5);
    assert!(!chat.contains("lane ask q1"), "the board hides the wake line: {chat}");
}

#[test]
fn the_pinned_thread_wins_over_the_newest() {
    let _env = env();
    let (c, sh) = setup("lane-ask-pinned", "ask_thread = \"t1\"", &["Pinned", "Newer"]);
    lane_ask(&c, &sh, "Split it?");
    assert_eq!(kinds(&c, "needs-decision")[0].str("routed"), Some("t1"));
    let wake = kinds(&c, "thread-msg");
    assert_eq!((wake.len(), wake[0].str("thread")), (1, Some("t1")));
}

#[test]
fn with_no_thread_the_ask_goes_to_the_owner_as_before() {
    let _env = env();
    let (c, sh) = setup("lane-ask-none", "", &[]);
    lane_ask(&c, &sh, "Split it?");
    assert_eq!(kinds(&c, "needs-decision")[0].str("routed"), None);
    assert!(kinds(&c, "thread-msg").is_empty() && kinds(&c, "orchestrator-start").is_empty());
    assert_eq!(pushes(&c), 1, "the owner's ask pushes");
}

#[test]
fn a_spec_quote_answer_reaches_the_lane_posts_an_fyi_and_pushes_nothing() {
    let _env = env();
    let (c, sh) = setup("lane-ask-answer", "", &["Run"]);
    lane_ask(&c, &sh, "Split it?");
    // Line breaks in the spec don't matter; the words must be verbatim.
    let quote = "Keep the old sender behind a flag until the owner says so.";
    let (code, out) = orchestrator_answer(&c, &sh, "t1", "q1", "2", quote);
    assert_eq!((code, out.as_str()), (0, "sendmeter q1: no -> sendmeter-impl-36\n"));
    assert!(typed(&sh, "DECISION q1: no"), "{:?}", sh.methods());
    let d = &kinds(&c, "decision")[0];
    assert_eq!((d.str("by"), d.str("basis")), (Some("orchestrator-t1"), Some(quote)));
    let fyi = kinds(&c, "thread-msg").pop().unwrap();
    assert_eq!(
        (fyi.str("thread"), fyi.str("from"), fyi.str("text")),
        (
            Some("t1"),
            Some("system"),
            Some(&*format!("answered q1 for #36: no, because {quote}"))
        )
    );
    assert_eq!(pushes(&c), 0, "an orchestrator's answer pushes nothing");

    // A past decision's id is a basis too; the ask's own id is not.
    let q2 = lane_ask(&c, &sh, "Split the tests too?");
    let (code, out) = orchestrator_answer(&c, &sh, "t1", &q2, "1", &q2);
    assert!(code == 3 && out.contains("REFUSED OWNER_ASK"), "{out}");
    let (code, out) = orchestrator_answer(&c, &sh, "t1", &q2, "1", "q1");
    assert_eq!((code, out.as_str()), (0, "sendmeter q2: yes -> sendmeter-impl-36\n"));
}

#[test]
fn an_owner_topic_is_refused_and_put_to_the_owner_in_the_thread() {
    let _env = env();
    let (c, sh) = setup("lane-ask-owner-topic", "", &["Run"]);
    let quote = "Keep the old sender behind a flag";
    for (q, id) in [("Promote staging→main now?", "q1"), ("Rotate the secret first?", "q2")] {
        lane_ask(&c, &sh, q);
        let (code, out) = orchestrator_answer(&c, &sh, "t1", id, "1", quote);
        assert_eq!(
            (code, out.as_str()),
            (3, "REFUSED OWNER_ASK: it names an owner topic; ask the owner\n")
        );
        assert!(sh.sent(LANE).is_empty(), "nothing reaches the lane");
    }
    assert!(kinds(&c, "decision").is_empty());
    let up = kinds(&c, "ask-owner");
    assert_eq!(
        up.iter()
            .map(|e| (e.str("id"), e.str("thread"), e.data["recommend"].clone()))
            .collect::<Vec<_>>(),
        [(Some("q1"), Some("t1"), json!(1)), (Some("q2"), Some("t1"), json!(1))]
    );
    assert_eq!(pushes(&c), 2, "each pushes as an ask");
    // The owner's choice row in the thread: the conductor's pick first; the tap answers the lane.
    let chat = chat(&c, NOW + 5);
    assert!(chat.contains("data-id=\"q1\""), "{chat}");
    as_owner();
    let (code, out) = run(&c, &sh.cli(&host()), &["answer", "sendmeter", "q1", "2"]);
    assert_eq!((code, out.as_str()), (0, "sendmeter q1: no -> sendmeter-impl-36\n"));
}

#[test]
fn an_empty_or_unmatched_basis_is_refused() {
    let _env = env();
    let (c, sh) = setup("lane-ask-basis", "", &["Run"]);
    lane_ask(&c, &sh, "Split it?");
    for (basis, why) in [
        ("", "no --basis"),
        ("Keep the new sender behind a flag", "--basis is neither"),
        ("behind a", "--basis is neither"),
        ("q9", "--basis is neither"),
    ] {
        let (code, out) = orchestrator_answer(&c, &sh, "t1", "q1", "2", basis);
        assert!(
            code == 3 && out.starts_with(&format!("REFUSED OWNER_ASK: {why}")),
            "{basis}: {out}"
        );
    }
    assert!(kinds(&c, "decision").is_empty());
    assert_eq!(kinds(&c, "ask-owner").len(), 1, "put to the owner once");
}

#[test]
fn the_owner_changes_a_orchestrator_answer_within_30_minutes_only() {
    let _env = env();
    let (c, sh) = setup("lane-ask-change", "", &["Run"]);
    let quote = "Keep the old sender behind a flag";
    lane_ask(&c, &sh, "Split it?");
    assert_eq!(orchestrator_answer(&c, &sh, "t1", "q1", "2", quote).0, 0);
    let board = chat(&c, NOW + 60);
    assert!(board.contains("answered q1 for #36: no, because Keep the old sender behind a flag</p><details class=chg><summary>change</summary><form class=chq data-id=\"q1\""), "{board}");
    // A conductor may not change it.
    let (code, out) = orchestrator_answer(&c, &sh, "t1", "q1", "1", quote);
    assert!(code == 3 && out.contains("REFUSED ALREADY_ANSWERED"), "{out}");
    as_owner();
    let (code, out) = run(
        &at(&c, NOW + 29 * 60),
        &sh.cli(&host()),
        &["answer", "sendmeter", "q1", "1"],
    );
    assert_eq!((code, out.as_str()), (0, "sendmeter q1: yes -> sendmeter-impl-36\n"));
    assert!(typed(&sh, "DECISION q1: yes"), "{:?}", sh.methods());
    assert_eq!(kinds(&c, "decision").last().unwrap().str("by"), Some("owner"));
    assert!(
        !chat(&c, NOW + 29 * 60 + 5).contains("class=chg"),
        "changed: no more [change]"
    );

    // After 30 min: no [change], and the owner's answer is refused.
    lane_ask(&c, &sh, "Split the tests too?");
    assert_eq!(orchestrator_answer(&c, &sh, "t1", "q2", "2", quote).0, 0);
    assert!(chat(&c, NOW + 60).contains("<form class=chq data-id=\"q2\""));
    let late = NOW + 30 * 60;
    let typed_before = sh.sent(LANE);
    assert!(!chat(&c, late).contains("class=chg"), "no [change] after 30 min");
    let (code, out) = run(&at(&c, late), &sh.cli(&host()), &["answer", "sendmeter", "q2", "1"]);
    assert!(code == 3 && out.contains("REFUSED ALREADY_ANSWERED"), "{out}");
    assert_eq!(sh.sent(LANE), typed_before, "the refused answer reaches no lane");
}
