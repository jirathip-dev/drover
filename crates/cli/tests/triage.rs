mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::{json, Value};

const REPO: &str = "-R jirathip-dev/sendmeter";
const LANE: &str = "sendmeter:90:triage";
const AMENDED: &str =
    "Do X.\n\n## Amendments\n- **Amendment (q1)**: a\n- **Amendment (q2)**: not a\n- **Amendment (q3)**: b\n";

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
        .reply("gh issue view 13 ", &issue("Old", &[]))
        .reply("gh issue view 14 ", &issue("Busy", &[]))
        .reply("gh issue view 15 ", &issue("In review", &[]))
        .reply("gh issue view 20 ", &issue("Root cause: #3", &[]))
        .reply("gh pr view issue-15", r#"{"number": 40, "state": "OPEN"}"#)
        .reply(
            "gh issue list -R jirathip-dev/sendmeter --state open",
            &json!([{"number": 20, "body": "**Root cause:** #3", "labels": []},
                    {"number": 21, "body": "Root cause: #4", "labels": []},
                    {"number": 22, "body": "Root cause: #3", "labels": [{"name": "stale?"}]}])
            .to_string(),
        )
        .reply(
            "gh api graphql -f owner=jirathip-dev -f name=sendmeter -f query=",
            r#"{"data": {"repository": {"i3": {"__typename": "Issue", "state": "CLOSED"},
                "i4": {"__typename": "Issue", "state": "OPEN"}}}}"#,
        )
        .reply("gh issue close", "")
        .reply("gh issue list", "[]")
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
fn a_triage_lane_closes_done_and_queued_skips_live_work_flags_orphans_and_reports_each_undo() {
    set_env("DROVER_LANE", LANE);
    let c = ctx(&tmpdir("triage-run"), CONFIG);
    let sh = FakeShed::attached(&c);
    let r = fake();
    let (code, out) = run(&c, &r, &["job", "sendmeter", "triage"]);
    assert!(code == 0 || code == 4, "{code} {out}");
    assert!(
        out.contains("filed sendmeter#90: triage: sendmeter 1970-01-12"),
        "{out}"
    );
    assert!(sh.has("sendmeter-triage-90"), "{:?}", sh.methods());
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
    run(&c, &r, &["queue", "sendmeter", "add", "12", "13", "14"]);
    for (n, agent) in [(13, "sendmeter-impl-13"), (14, "sendmeter-impl-14")] {
        let d = json!({"role": "impl", "agent": agent, "branch": format!("issue-{n}")});
        append(&c.state, &Event::new(10, "sendmeter", Some(n), "spawn", d)).unwrap();
    }
    sh.lane("sendmeter-impl-14", "working", "/w/issue-14");

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
    for n in ["12", "13"] {
        let (code, out) = close(n, "abc1234");
        assert!(
            code == 0 && out.contains(&format!("closed sendmeter#{n}")),
            "{n}: {code} {out}"
        );
    }
    let events = read(&c.state).unwrap();
    let left: Vec<u64> = drover::queue::view(&events, &fold(&events), "sendmeter")
        .iter()
        .map(|i| i.issue)
        .collect();
    assert_eq!(left, [90, 14], "a closed queued issue leaves the queue");
    for (n, why) in [
        ("14", "has a live lane"),
        ("15", "has an open PR"),
        ("8", "is labeled keep"),
    ] {
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
    let closed = events
        .iter()
        .rfind(|e| e.is("closed", "sendmeter", 90))
        .expect("closed");
    let text = closed.str("why").unwrap();
    assert!(text.starts_with("read 6 issues\n- "), "{text}");
    for undo in [
        format!("undo: `gh issue reopen 5 {REPO}`"),
        "undo: `its earlier text is in the folded block`".into(),
        format!("undo: `gh issue edit 9 {REPO} --remove-label stale?`"),
    ] {
        assert!(text.contains(&undo), "missing {undo:?}:\n{text}");
    }
    assert!(
        !r.called("gh issue edit 21 ") && !r.called("gh issue edit 22 "),
        "open root, flagged already"
    );
    assert!(r.called("gh issue edit 20 "), "closed root #3 flags #20");
    assert!(r.called(&format!("gh issue close 90 {REPO} --comment {text}")));
    assert_eq!(fold(&events)[&("sendmeter".into(), 90)].state, "closed");
}

#[test]
fn the_docs_pass_files_one_docs_issue_with_each_finding_and_none_when_clean() {
    set_env("DROVER_LANE", LANE);
    let c = ctx(&tmpdir("triage-docs"), CONFIG);
    let root = c.worktrees.join("repo");
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("README.md"), "# sendmeter\n\n## Non-goals\n- no web UI\n").unwrap();
    std::fs::write(root.join("docs/use.md"), "Pass `--legacy` to send.\nOpen the web UI.\n").unwrap();
    let r = Fake::default().reply(
        "gh issue create",
        "https://github.com/jirathip-dev/sendmeter/issues/91\n",
    );
    let (findings, clean) = (c.worktrees.join("findings.md"), c.worktrees.join("clean.md"));
    std::fs::write(&clean, "\n").unwrap();
    std::fs::write(
        &findings,
        "docs/use.md:1: names `--legacy`, removed: crates/core/src/cli.rs has no such flag\n\
         - docs/use.md:2: a web UI contradicts README.md:3 non-goals\n",
    )
    .unwrap();
    let docs = |f: &std::path::Path, root: &std::path::Path| {
        let (f, root) = (f.to_str().unwrap(), root.to_str().unwrap());
        run(
            &c,
            &r,
            &["triage", "docs", "sendmeter", "--findings", f, "--root", root],
        )
    };
    let (code, out) = docs(&clean, &root);
    assert!(code == 0 && out.contains("no docs issue"), "{code} {out}");
    assert_eq!(r.count("gh issue create"), 0, "a clean pass files nothing");

    let (code, out) = docs(&findings, &root);
    assert!(code == 0 && out.contains("filed sendmeter#91"), "{code} {out}");
    let want = "gh issue create -R jirathip-dev/sendmeter --title docs: sendmeter stale agent-read text, week of \
                1970-01-12 --body Triage (sendmeter:90:triage) found these passages stale, contradicting or bloated. \
                Fix each in its file; never edit the README's top (intent) section.\n\n\
                - docs/use.md:1: names `--legacy`, removed: crates/core/src/cli.rs has no such flag\n\
                - docs/use.md:2: a web UI contradicts README.md:3 non-goals\n";
    assert!(r.called(want), "{:?}", r.calls.borrow());
    let (code, out) = docs(&findings, &root);
    assert!(code == 3 && out.contains("ALREADY_FILED"), "{code} {out}");
    assert_eq!(r.count("gh issue create"), 1, "one docs issue a run");
    let events = read(&c.state).unwrap();
    let ev = events.iter().rfind(|e| e.kind == "triage-docs").unwrap();
    assert_eq!(ev.str("undo"), Some("gh issue close 91 -R jirathip-dev/sendmeter"));

    let bare = ctx(&tmpdir("triage-docs-bare"), CONFIG);
    let (f, root) = (findings.to_str().unwrap(), bare.worktrees.to_str().unwrap());
    let (code, out) = run(
        &bare,
        &r,
        &["triage", "docs", "sendmeter", "--findings", f, "--root", root],
    );
    assert_eq!(code, 0, "{out}");
    let calls = r.calls.borrow();
    let body = calls.iter().rfind(|c| c.starts_with("gh issue create")).unwrap();
    let first = body.split("\n\n").nth(1).unwrap().lines().next().unwrap();
    assert!(first.starts_with("- README.md:1: no intent section"), "{body}");
}

fn hook(lane: &str, cmd: &str) -> String {
    let d = tmpdir("hook");
    let fleet = "\n[fleet.tangelo]\nrepo = \"o/tangelo\"\nlocal = \"/tmp/tangelo\"\nintegration = \"staging\"\n";
    std::fs::write(d.join("fleet.toml"), CONFIG.to_string() + fleet).unwrap();
    hook_in(&d, lane, "Bash", &json!({"command": cmd}))
}

#[test]
fn a_triage_lane_changes_issues_only_through_its_verbs_and_never_code() {
    let acts = [
        "drover triage close tangelo 5 --why done --evidence abc1234",
        "drover triage fold tangelo 7 --spec spec.md",
        "drover triage label tangelo 9 stale?",
        "drover triage docs tangelo --findings f.md",
    ];
    for cmd in acts {
        assert_eq!(hook("tangelo:90:triage", cmd), "", "{cmd}");
        for lane in ["tangelo:12:impl", "tangelo:12:rev"] {
            assert!(hook(lane, cmd).contains("REFUSED ORCHESTRATOR_ONLY"), "{lane}: {cmd}");
        }
    }
    for (cmd, why) in [
        ("gh issue close 5 -R o/tangelo", "REFUSED TRIAGE_VERBS"),
        ("gh issue edit 7 -R o/tangelo --body x", "REFUSED TRIAGE_VERBS"),
        ("gh issue comment 5 -b x", "never comment"),
        ("git commit -am x", "read-only"),
        ("git push origin issue-90", "read-only"),
        ("gh pr merge 3", "merging is gated"),
    ] {
        assert!(hook("tangelo:90:triage", cmd).contains(why), "{cmd}");
    }
    assert_eq!(hook("tangelo:12:impl", "gh issue edit 7 -R o/tangelo --body x"), "");
}
