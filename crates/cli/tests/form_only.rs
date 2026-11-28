mod common;
use common::*;
use drover::config::Config;
use drover::events::{append, fold, read, Event, IssueState};
use drover::thread::Target;
use serde_json::json;

const FORM: &str = "FAIL (form-only)\n\n- blocking [trailer] abc1234: `Co-Authored-By: an AI`\n  Proof: spec\n  \
> No AI co-author trailers in commits\n  abc1234:5\n- blocking [docs-fresh] docs/x.md:3: stale\n  \
Proof: run\n  ```\n  drover arch-check\n  ```\n  ```\n  docs/x.md:3: stale\n  ```\n- non-blocking src/y.rs:1: a nit\n";
const SPEC: &str = "  Proof: spec\n  > AC 1: it works\n";

fn fixture(name: &str, extra: &str) -> (drover::Ctx, std::path::PathBuf) {
    let d = tmpdir(name);
    let c = ctx(&d, &format!("{CONFIG}{extra}"));
    let log = |ts, role: &str| {
        let d = json!({"role": role, "agent": format!("sendmeter-{role}-12"), "branch": "issue-12"});
        append(&c.state, &Event::new(ts, "sendmeter", Some(12), "spawn", d)).unwrap();
    };
    log(1, "impl");
    log(2, "rev");
    (c, d)
}

fn verdict(c: &drover::Ctx, d: &std::path::Path, sha: char, body: &str) -> String {
    let (sha, file) = (sha.to_string().repeat(40), d.join("verdict.md"));
    std::fs::write(&file, body).unwrap();
    let r = Fake::default().reply("gh api", &sha);
    let (code, out) = run(
        c,
        &r,
        &[
            "verdict",
            "sendmeter",
            "12",
            "FAIL",
            "--sha",
            &sha,
            "--file",
            &file.to_string_lossy(),
        ],
    );
    assert_eq!(code, 0, "{out}");
    out
}

fn state(c: &drover::Ctx) -> IssueState {
    fold(&read(&c.state).unwrap())
        .remove(&("sendmeter".to_string(), 12))
        .unwrap()
}

fn logged(c: &drover::Ctx, kind: &str) -> usize {
    read(&c.state).unwrap().iter().filter(|e| e.kind == kind).count()
}

#[test]
fn a_fail_blocking_on_the_done_gates_paperwork_is_refused_naming_it() {
    let (c, d) = fixture("form-paperwork", "");
    let file = d.join("verdict.md");
    for tag in ["format", "pr-body", "why-note"] {
        let body = format!(
            "FAIL\n\n- blocking [{tag}] src/x.rs:3: paperwork\n{SPEC}- blocking [ac] src/x.rs:9: wrong\n{SPEC}"
        );
        std::fs::write(&file, body).unwrap();
        let sha = "a".repeat(40);
        let r = Fake::default().reply("gh api", &sha);
        let args = [
            "verdict",
            "sendmeter",
            "12",
            "FAIL",
            "--sha",
            &sha,
            "--file",
            &file.to_string_lossy(),
        ];
        let (code, out) = run(&c, &r, &args);
        assert_eq!(code, 3, "{tag}: {out}");
        assert!(
            out.contains("REFUSED PAPERWORK: `- blocking [")
                && out.contains(&format!("[{tag}] is `turn done`'s check")),
            "{tag}: {out}"
        );
        assert!(!out.contains("[code]"), "only the paperwork blocker is named: {out}");
    }
    assert_eq!(logged(&c, "verdict"), 0);
    let note = format!("FAIL\n\n- non-blocking [format] src/x.rs:3: a nit\n- blocking [ac] src/x.rs:9: wrong\n{SPEC}");
    let out = verdict(&c, &d, 'b', &note);
    assert!(out.contains("(round 1)"), "a paperwork note is fine: {out}");
}

#[test]
fn an_all_form_fail_is_not_counted() {
    let (c, d) = fixture("form-all", "");
    let out = verdict(&c, &d, 'a', FORM);
    assert!(
        out.contains("recorded FAIL (form-only)") && out.contains("not counted · form 1/3"),
        "{out}"
    );
    let s = state(&c);
    assert_eq!(
        (
            s.state.as_str(),
            s.form,
            s.stage_rounds.get("rev").copied().unwrap_or_default()
        ),
        ("fix(1·f)", 1, 0)
    );
    let ev = read(&c.state).unwrap();
    assert_eq!(ev.last().unwrap().val("form_only"), true);
    let out = verdict(
        &c,
        &d,
        'b',
        &format!("FAIL\n\n- blocking [ac] src/x.rs:9: wrong\n{SPEC}"),
    );
    assert!(out.contains("(round 1)"), "{out}");
    assert_eq!(state(&c).state, "fix(2)");
}

#[test]
fn one_substantive_or_untagged_blocker_counts() {
    let (c, d) = fixture("form-mixed", "");
    let out = verdict(
        &c,
        &d,
        'a',
        &format!("{FORM}- blocking [ac] src/x.rs:9: wrong result\n{SPEC}"),
    );
    assert!(out.contains("form-only-rejected") && out.contains("(round 1)"), "{out}");
    assert_eq!(
        (state(&c).state.as_str(), logged(&c, "form-only-rejected")),
        ("fix(1)", 1)
    );
    let out = verdict(
        &c,
        &d,
        'b',
        &format!("{FORM}- blocking [rules] src/x.rs:9: a rule\n{SPEC}"),
    );
    assert!(out.contains("(round 2)"), "{out}");
    let s = state(&c);
    assert_eq!((s.form, logged(&c, "form-only-rejected")), (0, 2));
    let ev = read(&c.state).unwrap();
    let last = ev.iter().rev().find(|e| e.kind == "verdict").unwrap();
    assert_eq!(last.val("form_only"), false);
}

#[test]
fn past_form_rounds_they_count_and_the_same_form_blocker_gets_stuck() {
    let (c, d) = fixture("form-bound", "form_rounds = 1\n");
    let rev = |ts| {
        let d = json!({"role": "rev", "agent": "sendmeter-rev-12", "branch": "issue-12"});
        append(&c.state, &Event::new(ts, "sendmeter", Some(12), "spawn", d)).unwrap();
    };
    assert!(verdict(&c, &d, 'a', FORM).contains("not counted · form 1/1"));
    rev(10);
    let out = verdict(&c, &d, 'b', FORM);
    assert!(
        out.contains("form-rounds-exhausted") && out.contains("(round 1)"),
        "{out}"
    );
    assert_eq!(logged(&c, "form-rounds-exhausted"), 1);
    rev(20);
    let out = verdict(&c, &d, 'c', FORM);
    assert!(
        out.contains("(round 2)") && out.contains("stuck → needs-human"),
        "{out}"
    );
    let ev = read(&c.state).unwrap();
    let nh = ev.iter().rfind(|e| e.kind == "needs-human").unwrap();
    assert_eq!(
        nh.str("reason"),
        Some("stuck: the same blocking finding `[docs-fresh] docs/x.md` in 3 FAILs in a row (2 fix rounds)")
    );
}

#[test]
fn the_stuck_card_on_an_issue_queued_from_a_thread_lands_with_the_owner() {
    let (c, d) = fixture("form-stuck-owner", "");
    let ev = |ts, n: Option<u64>, kind: &str, d: serde_json::Value| {
        append(&c.state, &Event::new(ts, "sendmeter", n, kind, d)).unwrap();
    };
    ev(3, None, "thread-new", json!({"thread": "t1", "title": "Run"}));
    ev(4, None, "queue", json!({"op": "add", "issues": [12], "thread": "t1"}));
    ev(
        5,
        Some(12),
        "needs-human",
        json!({"code": "RESPAWN_LOOP", "reason": "x"}),
    );
    let blocker = "FAIL\n\n- blocking [ac] src/a.rs:3: AC 1 missing\n";
    for (ts, sha) in [(10, 'a'), (20, 'b'), (30, 'c')] {
        let d2 = json!({"role": "rev", "agent": "sendmeter-rev-12", "branch": "issue-12"});
        ev(ts, Some(12), "spawn", d2);
        verdict(&c, &d, sha, &format!("{blocker}{SPEC}"));
    }
    let log = read(&c.state).unwrap();
    let to = |code: &str| {
        let nh = log
            .iter()
            .rfind(|e| e.kind == "needs-human" && e.str("code") == Some(code))
            .unwrap();
        let rows = drover::thread::rows(&log);
        rows.into_iter()
            .find(|(_, r)| r.seq == nh.seq)
            .map(|(_, r)| r.to)
            .unwrap()
    };
    assert_eq!(
        to("RESPAWN_LOOP"),
        Target::Thread { id: "t1".into() },
        "the issue's other cards go home"
    );
    assert_eq!(to("STUCK"), Target::Owner);
    assert!(!log.iter().any(|e| e.kind == "ask-owner" || e.kind == "thread-msg"));
}

#[test]
fn a_fleet_overrides_the_form_categories() {
    let cfg = Config::parse(&format!(
        "{CONFIG}form_categories = [\"lint\"]\n[fleet.other]\nrepo = \"o/o\"\nlocal = \"/tmp/o\"\nintegration = \"staging\"\n"
    ))
    .unwrap();
    assert_eq!(
        cfg.fleet["other"].form_categories.as_deref(),
        Some(&["lint".to_string()][..])
    );
    let own = CONFIG.replace(
        "integration = \"staging\"   # never main",
        "integration = \"staging\"\nform_categories = [\"lint\"]",
    );
    let d = tmpdir("form-fleet");
    let c = ctx(&d, &own);
    append(
        &c.state,
        &Event::new(
            2,
            "sendmeter",
            Some(12),
            "spawn",
            json!({"role": "rev", "agent": "sendmeter-rev-12"}),
        ),
    )
    .unwrap();
    let trailer = "FAIL (form-only)\n\n- blocking [trailer] abc1234: an AI trailer\n  Proof: spec\n  > No AI co-author trailers\n  abc1234:5\n";
    let out = verdict(&c, &d, 'a', trailer);
    assert!(out.contains("form-only-rejected"), "{out}");
    let out = verdict(
        &c,
        &d,
        'b',
        &format!("FAIL (form-only)\n- blocking [lint] src/x.rs:3: clippy\n{SPEC}"),
    );
    assert!(out.contains("not counted"), "{out}");
}

#[test]
fn form_only_parses_tags() {
    let cats = ["format".to_string(), "pr-body".to_string()];
    let ok = |b: &str| drover::gate::form_only(b, &cats);
    assert!(ok(
        "FAIL (form-only)\n- **blocking** [format] a.rs:1\n- non-blocking [ac] b.rs:2\n"
    ));
    assert!(!ok("FAIL (form-only)\n- non-blocking [format] a.rs:1\n"), "no blocker");
    assert!(!ok("FAIL (form-only)\n- blocking [ac] a.rs:1\n"));
    let line = "Tests checked against anti-patterns: yes\n";
    assert!(ok(&format!("FAIL (form-only)\n{line}- blocking [format] a.rs:1\n")));
}

#[test]
fn the_antipatterns_line_is_accepted() {
    let (c, d) = fixture("form-antipatterns", "");
    let body = format!(
        "{FORM}Reviewed head: {}\nTests checked against anti-patterns: yes\n",
        "a".repeat(40)
    );
    let out = verdict(&c, &d, 'a', &body);
    assert!(out.contains("recorded FAIL (form-only)"), "{out}");
    assert_eq!(state(&c).form, 1);
}

#[test]
fn the_queue_row_says_not_counted_in_amber() {
    let (mut c, d) = fixture("form-card", "");
    verdict(&c, &d, 'a', FORM);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let pr = json!({"number": 34, "state": "OPEN", "baseRefName": "staging", "headRefOid": "a".repeat(40), "statusCheckRollup": []});
    let cache = c.state.parent().unwrap().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let file = json!({"ts": 1_000_000, "branch": "issue-12", "pr": pr});
    std::fs::write(cache.join("pr-sendmeter-12.json"), file.to_string()).unwrap();
    c.now = 1_000_000;
    let r = OnShed::new(Fake::default());
    let s = drover::serve::Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    let q: serde_json::Value = serde_json::from_str(&api(&s, "127.0.0.1:7777", "", c.now, "queue")).unwrap();
    let pr = &q["fleets"][0]["rows"][0]["pr"];
    assert_eq!(
        (&pr["verdict"], &pr["vtone"], &pr["rounds"], &pr["form"]),
        (&json!("FAIL (form-only)"), &json!("a"), &json!(1), &json!("1/3")),
        "{q}"
    );
}

#[test]
fn a_blocking_heading_is_ignored() {
    let cats = ["pr-body".to_string(), "docs-fresh".to_string()];
    let ok = |b: &str| drover::gate::form_only(b, &cats);
    let head = "FAIL (form-only)\n\n## Blocking\n- blocking [pr-body] body lags the head\n";
    assert!(ok(&format!("{head}- blocking [docs-fresh] README\n")));
    assert!(!ok(&format!("{head}- blocking README is stale\n")));
}
