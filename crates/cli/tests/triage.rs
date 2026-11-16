mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::{json, Value};

const REPO: &str = "-R jirathip-dev/sendmeter";
const LANE: &str = "sendmeter:90:triage";
const AMENDED: &str =
    "Do X.\n\n## Amendments\n- **Amendment (q1)**: a\n- **Amendment (q2)**: not a\n- **Amendment (q3)**: b\n";

fn ev(c: &drover::Ctx, issue: Option<u64>, kind: &str, d: Value) {
    append(&c.state, &Event::new(c.now, "sendmeter", issue, kind, d)).unwrap();
}

fn issue(body: &str, labels: &[&str]) -> String {
    let labels: Vec<Value> = labels.iter().map(|l| json!({"name": l})).collect();
    json!({"title": "an issue", "body": body, "labels": labels}).to_string()
}

fn fake() -> Fake {
    Fake::default()
        .reply(
            "gh issue create",
            "https://github.com/jirathip-dev/sendmeter/issues/90\n",
        )
        .reply("gh issue view 90", &issue("the triage template", &[]))
        .reply("gh issue view 5 ", &issue("Fix the thing", &[]))
        .reply("gh issue view 7 ", &issue(AMENDED, &[]))
        .reply("gh issue view 8 ", &issue(AMENDED, &["keep"]))
        .reply("gh issue view 9 ", &issue("Maybe?", &[]))
        .reply("gh issue view 12 ", &issue(AMENDED, &[]))
        .reply("gh issue close", "")
        .reply("gh issue edit", "")
        .reply(
            "git -C /tmp/does-not-matter/sendmeter merge-base --is-ancestor abc1234 origin/staging",
            "",
        )
        .once(
            "git -C /tmp/does-not-matter/sendmeter merge-base --is-ancestor def5678",
            1,
            "",
        )
        .reply("git -C", "0\n")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

#[test]
fn a_triage_lane_closes_done_folds_amended_skips_queued_and_reports_each_undo() {
    std::env::set_var("DROVER_LANE", LANE);
    let c = ctx(&tmpdir("triage-run"), CONFIG);
    let sh = FakeShed::attached(&c);
    ev(&c, None, "thread-new", json!({"thread": "t1", "title": "chat"}));
    let r = fake();
    let (code, out) = run(&c, &r, &["triage", "sendmeter"]);
    assert!(code == 0 || code == 4, "{code} {out}");
    assert!(
        out.contains("filed sendmeter#90: triage: sendmeter week of 1970-01-12"),
        "{out}"
    );
    assert_eq!(sh.last("run")["name"], "sendmeter-triage-90", "{:?}", sh.methods());
    let events = read(&c.state).unwrap();
    let spawn = events.iter().find(|e| e.is("spawn", "sendmeter", 90)).expect("a lane");
    assert_eq!(
        (spawn.str("role"), spawn.str("profile")),
        (Some("triage"), Some("fleet-impl"))
    );
    let brief = std::fs::read_to_string(c.worktrees.join("sendmeter/issue-90/.brief.md")).unwrap();
    assert!(
        brief.contains("# ROLE: TRIAGE") && brief.contains("drover triage close|fold|label sendmeter"),
        "{brief}"
    );
    assert!(!brief.contains("open a PR to"), "{brief}");
    run(&c, &r, &["queue", "sendmeter", "add", "12"]);

    let close = |n: &str, ev: &str| {
        run(
            &c,
            &r,
            &["triage", "close", "sendmeter", n, "--why", "done", "--evidence", ev],
        )
    };
    let (code, out) = close("5", "abc1234");
    assert_eq!(code, 0, "{out}");
    let commit = "https://github.com/jirathip-dev/sendmeter/commit/abc1234";
    let want = format!(
        "gh issue close 5 {REPO} --reason completed --comment triage: done: {commit}\n\nUndo: `gh issue reopen 5 {REPO}`"
    );
    assert!(r.called(&want), "{:?}", r.calls.borrow());
    let (code, out) = close("6", "def5678");
    assert!(code == 3 && out.contains("NOT_ON_INTEGRATION"), "{code} {out}");
    for (n, why) in [("12", "is queued"), ("8", "is labeled keep")] {
        let (code, out) = close(n, "abc1234");
        assert!(
            code == 3 && out.contains("PROTECTED") && out.contains(why),
            "{n}: {code} {out}"
        );
        assert!(!r.called(&format!("gh issue close {n} ")), "{n} untouched");
    }

    let spec = c.worktrees.join("spec.md");
    std::fs::write(&spec, "Do X with b.\n").unwrap();
    let fold_ = |n: &str| {
        run(
            &c,
            &r,
            &["triage", "fold", "sendmeter", n, "--spec", spec.to_str().unwrap()],
        )
    };
    let (code, out) = fold_("7");
    assert!(
        code == 0 && out.contains("folded sendmeter#7: 3 amendments"),
        "{code} {out}"
    );
    let body = format!(
        "gh issue edit 7 {REPO} --body ## Spec (current)\n\nDo X with b.\n\n<details><summary>Earlier text, folded by \
         triage 1970-01-12</summary>\n\n{}\n\n</details>\n",
        AMENDED.trim_end()
    );
    assert!(r.called(&body), "{:?}", r.calls.borrow());
    let (code, out) = fold_("5");
    assert!(code == 3 && out.contains("NOTHING_TO_FOLD"), "{code} {out}");
    let folded = body.split_once("--body ").unwrap().1.to_string() + "\n## Amendments\n- **Amendment (q4)**: c\n";
    assert_eq!(
        drover::triage::live(&folded).matches("## Amendments").count(),
        1,
        "the folded text is not live"
    );

    let (code, out) = run(&c, &r, &["triage", "label", "sendmeter", "9", "stale?"]);
    assert_eq!(code, 0, "{out}");
    assert!(r.called(&format!("gh issue edit 9 {REPO} --add-label stale?")));

    let (code, out) = run(&c, &r, &["done", "sendmeter", "90", "--no-change", "read 6 issues"]);
    assert_eq!(code, 0, "{out}");
    let events = read(&c.state).unwrap();
    let msg = events
        .iter()
        .rfind(|e| e.kind == "thread-msg")
        .expect("a summary in the thread");
    let text = msg.str("text").unwrap();
    for line in [
        "triage sendmeter#90: 1 closed, 1 folded, 1 flagged",
        &format!("- #5 closed (done: {commit}) · undo: `gh issue reopen 5 {REPO}`"),
        "- #7 folded (3 amendments) · undo: `its earlier text is in the folded block`",
        &format!("- #9 labeled [\"stale?\"] · undo: `gh issue edit 9 {REPO} --remove-label stale?`"),
        "note: read 6 issues",
    ] {
        assert!(text.contains(line), "missing {line:?}:\n{text}");
    }
    assert_eq!(msg.str("thread"), Some("t1"));
    assert!(r.called(&format!("gh issue close 90 {REPO} --comment {text}")));
    assert_eq!(fold(&events)[&("sendmeter".into(), 90)].state, "closed");
}

#[test]
fn tick_runs_triage_weekly_only_where_the_fleet_turns_it_on() {
    std::env::set_var("DROVER_LANE", LANE);
    let on = CONFIG.replace(
        "integration = \"staging\"   # never main",
        "integration = \"staging\"\ntriage = \"weekly\"",
    );
    let filed = |cfg: &str, name: &str, days: &[u64]| -> Vec<usize> {
        let c = ctx(&tmpdir(name), cfg);
        let _sh = FakeShed::attached(&c);
        let tick = |now: u64| {
            let r = fake()
                .reply("herdr status", "server:\n  status: running\n")
                .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n");
            let (code, out) = run(&drover::Ctx { now, ..c.clone() }, &r, &["tick"]);
            assert_eq!(code, 0, "{out}");
            r.count("gh issue create -R jirathip-dev/sendmeter --title triage: ")
        };
        days.iter().map(|d| tick(c.now + d * 86400)).collect()
    };
    assert_eq!(filed(&on, "triage-tick", &[0, 6, 7]), [1, 0, 1]);
    assert_eq!(filed(CONFIG, "triage-off", &[0, 7]), [0, 0]);
}
