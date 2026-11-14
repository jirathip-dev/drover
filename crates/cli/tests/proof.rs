mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::json;

const RUN: &str =
    "- blocking [code] src/x.rs:9: total is off by one\n  Proof: run\n  ```\n  cargo test total\n  ```\n  \
```\n  left: 4, right: 5 (AC 2 wants 5)\n  ```\n";
const SPEC: &str = "- blocking [code] src/x.rs:9: no refusal\n  Proof: spec\n  > AC 3: an empty file is refused\n";
const BODY: &str = "- blocking [pr-body] PR body: no `## For humans`\n  Proof: spec\n  \
> The PR body has a `## For humans` section\n  PR#34:1 has none\n";

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
    let body = format!("FAIL\n\n{RUN}- blocking [code] src/y.rs:2: leaks the token\n");
    let (code, out, c) = verdict("FAIL", &body);
    assert_eq!((code, verdicts(&c)), (3, 0), "{out}");
    assert!(out.contains("REFUSED NO_PROOF"), "{out}");
    assert!(
        out.contains("`- blocking [code] src/y.rs:2: leaks the token` (no Proof block)"),
        "{out}"
    );
    assert!(!out.contains("src/x.rs:9"), "the proven finding is not named: {out}");
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
        let (code, out, c) = verdict("FAIL", &format!("FAIL\n- blocking [code] src/x.rs:9: wrong\n{proof}"));
        assert_eq!((code, verdicts(&c)), (3, 0), "{proof}: {out}");
        assert!(
            out.contains(&format!(
                "REFUSED NO_PROOF: `- blocking [code] src/x.rs:9: wrong` ({why})"
            )),
            "{out}"
        );
    }
    let (code, out, _) = verdict("FAIL", "FAIL\n- blocking [code] wrong\nProof: spec\n> AC 3\n");
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
    let body = "FAIL\n- blocking [ui] web/a.ts:4: chip overflows\n  Proof: run\n  verify/ui-390/board.png\n";
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
    let notes = "- non-blocking [code] src/z.rs:1: a nit, no proof\n- blocking (pasted): see below\n";
    let (code, out, c) = verdict("PASS", &format!("PASS\n{notes}"));
    assert_eq!((code, verdicts(&c)), (0, 1), "PASS is not parsed: {out}");
    let body = format!("FAIL\n{RUN}- non-blocking [code] src/z.rs:1: a nit, no proof\n");
    let (code, out, c) = verdict("FAIL", &body);
    assert_eq!((code, verdicts(&c)), (0, 1), "{out}");
}

#[test]
fn proof_output_and_quotes_are_never_findings() {
    let body = "FAIL\n- blocking [code] src/x.rs:9: wrong\n  Proof: run\n  ```\n  drover verdict\n  ```\n  \
```\n  - blocking [x] pasted output\n  ```\n";
    let (code, out, _) = verdict("FAIL", body);
    assert_eq!(code, 0, "{out}");
    let quoted = "FAIL (form-only)\n\n- blocking [pr-body] PR body stale\n  Proof: spec\n  \
> missing or inaccurate is a `blocking` finding\n  PR#34:2\n";
    let (code, out, _) = verdict("FAIL", quoted);
    assert!(code == 0 && out.contains("recorded FAIL (form-only)"), "{out}");
}
