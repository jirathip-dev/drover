mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::json;

const RUN: &str = "- blocking [ac] src/x.rs:9: total is off by one\n  Proof: run\n  ```\n  cargo test total\n  ```\n  \
```\n  left: 4, right: 5 (AC 2 wants 5)\n  ```\n";
const SPEC: &str = "- blocking [ac] src/x.rs:9: no refusal\n  Proof: spec\n  > AC 3: an empty file is refused\n";
const BODY: &str = "- blocking [trailer] abc1234: an AI co-author trailer\n  Proof: spec\n  \
> an AI co-author trailer in a commit made by this round's lane\n  abc1234:5\n";

fn verdict(v: &str, body: &str) -> (i32, String, drover::Ctx) {
    let d = tmpdir("proof");
    let c = ctx(&d, CONFIG);
    for (ts, role) in [(1, "impl"), (2, "rev")] {
        let e = json!({"role": role, "agent": format!("sendmeter-{role}-12"), "branch": "issue-12"});
        append(&c.state, &Event::new(ts, "sendmeter", Some(12), "spawn", e)).unwrap();
    }
    let (sha, file) = ("a".repeat(40), d.join("verdict.md"));
    std::fs::write(&file, body).unwrap();
    let r = Fake::default().reply("gh api", &sha);
    let args = [
        "verdict",
        "sendmeter",
        "12",
        v,
        "--sha",
        &sha,
        "--file",
        &file.to_string_lossy(),
    ];
    let (code, out) = run(&c, &r, &args);
    (code, out, c)
}

fn verdicts(c: &drover::Ctx) -> usize {
    read(&c.state).unwrap().iter().filter(|e| e.kind == "verdict").count()
}

#[test]
fn a_run_or_spec_proof_is_recorded() {
    for b in [RUN, SPEC] {
        let (code, out, c) = verdict("FAIL", &format!("FAIL\n\n{b}"));
        assert_eq!((code, verdicts(&c)), (0, 1), "{out}");
        assert!(out.contains("recorded FAIL"), "{out}");
    }
}

#[test]
fn a_blocker_without_proof_is_refused_by_name() {
    let body = format!("FAIL\n\n{RUN}- blocking [ac] src/y.rs:2: leaks the token\n");
    let (code, out, c) = verdict("FAIL", &body);
    assert_eq!((code, verdicts(&c)), (3, 0), "{out}");
    assert!(out.contains("REFUSED NO_PROOF"), "{out}");
    assert!(
        out.contains("`- blocking [ac] src/y.rs:2: leaks the token` (no Proof block)"),
        "{out}"
    );
    assert!(!out.contains("src/x.rs:9"), "the proven finding is not named: {out}");
    let refused: Vec<Event> = read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "verdict-refused")
        .cloned()
        .collect();
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!(
        (refused[0].str("code"), refused[0].str("head")),
        (Some("NO_PROOF"), Some(&*"a".repeat(40)))
    );
    assert!(refused[0].passive, "the fold ignores it");
}

#[test]
fn malformed_proofs_are_refused() {
    let cases = [
        ("Proof: run\n```\ncargo test\n```\n", "run proof has no output"),
        ("Proof: run\nleft: 4\n", "run proof has no fenced command"),
        ("Proof: spec\nsee AC 3\n", "spec proof has no `>` quoted line"),
        ("Proof: run\n", "run proof has no fenced command"),
        ("Proof: hunch\n> AC 3\n", "Proof is neither `run` nor `spec`"),
    ];
    let trailer = "\nReviewed head: aaaa\nTests checked against anti-patterns: yes\n";
    for (proof, why) in cases
        .iter()
        .flat_map(|c| [(c.0.to_string(), c.1), (format!("{}{trailer}", c.0), c.1)])
    {
        let (code, out, c) = verdict("FAIL", &format!("FAIL\n- blocking [ac] src/x.rs:9: wrong\n{proof}"));
        assert_eq!((code, verdicts(&c)), (3, 0), "{proof}: {out}");
        assert!(
            out.contains(&format!(
                "REFUSED NO_PROOF: `- blocking [ac] src/x.rs:9: wrong` ({why})"
            )),
            "{out}"
        );
    }
    let (code, out, _) = verdict("FAIL", "FAIL\n- blocking [ac] wrong\nProof: spec\n> AC 3\n");
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("spec proof has no file:line"), "{out}");
}

#[test]
fn prose_after_the_findings_is_not_a_finding() {
    let body = format!("FAIL\n{RUN}\nNo other blocking findings.\nReviewed head: aaaa\n");
    let (code, out, c) = verdict("FAIL", &body);
    assert_eq!((code, verdicts(&c)), (0, 1), "{out}");
}

#[test]
fn a_screenshot_path_is_a_run_proof() {
    let body = "FAIL\n- blocking [ac] web/a.ts:4: chip overflows\n  Proof: run\n  verify/ui-390/board.png\n";
    let (code, out, c) = verdict("FAIL", body);
    assert_eq!((code, verdicts(&c)), (0, 1), "{out}");
}

#[test]
fn a_form_only_fail_with_a_spec_proof_is_recorded_and_not_counted() {
    let (code, out, c) = verdict("FAIL", &format!("FAIL (form-only)\n\n{BODY}"));
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("recorded FAIL (form-only)") && out.contains("not counted"),
        "{out}"
    );
    let s = fold(&read(&c.state).unwrap())
        .remove(&("sendmeter".to_string(), 12))
        .unwrap();
    assert_eq!((s.form, s.stage_rounds.values().sum::<u32>()), (1, 0));
}

#[test]
fn pass_and_notes_are_free_form() {
    let notes = "- non-blocking [ac] src/z.rs:1: a nit, no proof\n- blocking (pasted): see below\n";
    let (code, out, c) = verdict("PASS", &format!("PASS\n{notes}"));
    assert_eq!((code, verdicts(&c)), (0, 1), "PASS is not parsed: {out}");
    let body = format!("FAIL\n{RUN}- non-blocking [ac] src/z.rs:1: a nit, no proof\n");
    let (code, out, c) = verdict("FAIL", &body);
    assert_eq!((code, verdicts(&c)), (0, 1), "{out}");
}

#[test]
fn proof_output_and_quotes_are_never_findings() {
    let body = "FAIL\n- blocking [ac] src/x.rs:9: wrong\n  Proof: run\n  ```\n  drover verdict\n  ```\n  \
```\n  - blocking [x] pasted output\n  ```\n";
    let (code, out, _) = verdict("FAIL", body);
    assert_eq!(code, 0, "{out}");
    let quoted = "FAIL (form-only)\n\n- blocking [trailer] abc1234 has one\n  Proof: spec\n  \
> an AI co-author trailer in a commit is a `blocking` finding\n  abc1234:5\n";
    let (code, out, _) = verdict("FAIL", quoted);
    assert!(code == 0 && out.contains("recorded FAIL (form-only)"), "{out}");
}

#[test]
fn a_blocker_outside_the_ac_is_refused_and_follow_ups_are_recorded() {
    let body =
        format!("FAIL\n{RUN}- blocking [style] src/y.rs:2: long fn\n  Proof: spec\n  > keep fns short\n  src/y.rs:2\n");
    let (code, out, c) = verdict("FAIL", &body);
    assert_eq!((code, verdicts(&c)), (3, 0), "{out}");
    assert!(
        out.contains("REFUSED OUT_OF_SCOPE: `- blocking [style] src/y.rs:2: long fn`"),
        "{out}"
    );
    assert!(!out.contains("src/x.rs:9"), "the AC finding is not named: {out}");
    let body =
        format!("FAIL\n{RUN}- follow-up [style] src/y.rs:2: long fn\n* follow-up [tests] src/z.rs:7: no edge case\n");
    let (code, out, c) = verdict("FAIL", &body);
    assert_eq!((code, verdicts(&c)), (0, 1), "{out}");
    let all = read(&c.state).unwrap();
    let v = all.iter().rfind(|e| e.kind == "verdict").unwrap();
    assert_eq!(
        v.data["follow_ups"],
        json!(["[style] src/y.rs:2: long fn", "[tests] src/z.rs:7: no edge case"])
    );
}

fn round(c: &drover::Ctx, d: &std::path::Path, k: u64, body: &str) -> String {
    let sha = format!("{k}").repeat(40);
    for (ts, role) in [(k * 10 + 1, "impl"), (k * 10 + 2, "rev")] {
        let e = json!({"role": role, "agent": format!("sendmeter-{role}-12"), "branch": "issue-12"});
        append(&c.state, &Event::new(ts, "sendmeter", Some(12), "spawn", e)).unwrap();
    }
    let file = d.join(format!("verdict-{k}.md"));
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

fn cards(c: &drover::Ctx) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "needs-human")
        .cloned()
        .collect()
}

#[test]
fn the_same_blocker_in_three_fails_is_one_stuck_card_naming_it() {
    let d = tmpdir("stuck-same");
    let c = ctx(&d, CONFIG);
    for k in 1..=3 {
        let line = 9 + k;
        let other =
            format!("- blocking [ac] src/k{k}.rs:1: new each round\n  Proof: spec\n  > AC 1\n  src/k{k}.rs:1\n");
        let same = format!(
            "- blocking [ac] `src/x.rs:{line}`: total is off by one\n  Proof: spec\n  > AC 2\n  src/x.rs:{line}\n"
        );
        let out = round(&c, &d, k, &format!("FAIL\n\n{other}{same}"));
        assert_eq!(cards(&c).len(), usize::from(k == 3), "round {k}: {out}");
    }
    let card = &cards(&c)[0];
    assert_eq!(card.str("code"), Some("STUCK"));
    assert_eq!(
        card.str("reason"),
        Some("stuck: the same blocking finding `[ac] src/x.rs` in 3 FAILs in a row (2 fix rounds)")
    );
    let s = fold(&read(&c.state).unwrap())
        .remove(&("sendmeter".to_string(), 12))
        .unwrap();
    assert_eq!(s.state, "needs-human");
}

#[test]
fn findings_that_change_each_round_never_make_a_card() {
    let d = tmpdir("stuck-moving");
    let c = ctx(&d, CONFIG);
    for k in 1..=5 {
        let cat = if k % 2 == 0 { "ac" } else { "rules" };
        let f = format!("- blocking [{cat}] src/x.rs:9: a new miss\n  Proof: spec\n  > AC {k}\n  src/x.rs:9\n");
        let g = format!("- blocking [ac] src/r{k}.rs:3: and another\n  Proof: spec\n  > AC {k}\n  src/r{k}.rs:3\n");
        round(&c, &d, k, &format!("FAIL\n\n{f}{g}"));
    }
    assert!(cards(&c).is_empty(), "{:?}", cards(&c));
    let s = fold(&read(&c.state).unwrap())
        .remove(&("sendmeter".to_string(), 12))
        .unwrap();
    assert_eq!(s.state, "fix(5)", "no round limit");
}

fn ci_red(c: &drover::Ctx, d: &std::path::Path, k: u64, check: &str) -> String {
    let sha = format!("{k}").repeat(40);
    for (ts, role) in [(k * 10 + 1, "impl"), (k * 10 + 2, "rev")] {
        let e = json!({"role": role, "agent": format!("sendmeter-{role}-12"), "branch": "issue-12"});
        append(&c.state, &Event::new(ts, "sendmeter", Some(12), "spawn", e)).unwrap();
    }
    let pass = json!({"verdict": "PASS", "sha": sha, "stage": "rev", "loop": "rev", "body": "PASS\n"});
    append(
        &c.state,
        &Event::new(k * 10 + 3, "sendmeter", Some(12), "verdict", pass),
    )
    .unwrap();
    let url = format!("https://github.com/jirathip-dev/sendmeter/actions/runs/{k}00/job/1");
    let rollup = json!([{"__typename": "CheckRun", "name": check, "status": "COMPLETED", "conclusion": "FAILURE", "detailsUrl": url}]);
    let pr = json!({"headRefOid": sha, "statusCheckRollup": rollup});
    let log = Fake::default().reply("gh run view", "test total ... FAILED");
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let body = drover::gh::findings(&log, cfg.fleet("sendmeter").unwrap(), &pr, &sha, &[]);
    let file = d.join(format!("ci-{k}.md"));
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

#[test]
fn ci_red_at_the_same_check_for_two_fix_rounds_is_one_card_naming_it() {
    let d = tmpdir("stuck-ci");
    let c = ctx(&d, CONFIG);
    for k in 1..=3 {
        let out = ci_red(&c, &d, k, "rust / clippy");
        assert_eq!(cards(&c).len(), usize::from(k == 3), "round {k}: {out}");
    }
    assert_eq!(
        cards(&c)[0].str("reason"),
        Some("stuck: CI red at the same check `rust / clippy` for 2 fix rounds")
    );
    let d = tmpdir("stuck-ci-moving");
    let c = ctx(&d, CONFIG);
    for (k, check) in [(1, "build"), (2, "rust / clippy"), (3, "build"), (4, "test")] {
        ci_red(&c, &d, k, check);
    }
    assert!(cards(&c).is_empty(), "{:?}", cards(&c));
}
