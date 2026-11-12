//! #141: each `driven` result as the Host tab's FLEETS row says it: (status, word, plain words + what clears it).
use drover::events::Event;
use drover::serve::in_words;
use serde_json::json;

fn said(code: Option<&str>, step: &str, issue: u64, result: &str, serial: &str) -> (String, String, String) {
    let d = json!({"step": step, "code": code, "result": result, "cmd": "drover …"});
    let (t, w, x) = in_words(&Event::new(1, "synergy", Some(issue), "driven", d), serial);
    (t.into(), w.into(), x)
}

/// (code, step, issue, raw result, serial label, [status, word, plain words]).
type Case<'a> = (Option<&'a str>, &'a str, u64, &'a str, &'a str, [&'a str; 3]);

#[test]
fn every_refusal_and_outcome_has_its_plain_words() {
    let busy =
        "REFUSED SERIAL_BUSY: serial #2755 is unmerged (impl), #2782 is unmerged (impl), #2891 is unmerged (impl); \
                retry after they merge";
    let exit4 = r#"exit 4: "drover-impl-138" ["claude" "fleet-impl"] @ "~/.herdr/worktrees/drover/issue-138""#;
    let golden: &[Case] = &[
        (
            Some("SERIAL_BUSY"),
            "start",
            2900,
            busy,
            "database",
            [
                "a",
                "waiting",
                "3 serial (database) slots full (#2755, #2782, #2891 must merge first)",
            ],
        ),
        (
            Some("SERIAL_BUSY"),
            "start",
            2900,
            "REFUSED SERIAL_BUSY: serial #2755 is unmerged (impl); retry after they merge",
            "serial",
            ["a", "waiting", "1 serial slot full (#2755 must merge first)"],
        ),
        (
            Some("HOST_BUSY"),
            "start",
            12,
            "REFUSED HOST_BUSY: load/cpu 4.24 > 2.5; retry later or pass --force",
            "serial",
            [
                "a",
                "held",
                "the host is busy: load/cpu 4.24 > 2.5 · starts again when it drops",
            ],
        ),
        (
            Some("DISK_LOW"),
            "start",
            12,
            "REFUSED DISK_LOW: 8.0 GB free < 10; `drover clean --yes`",
            "serial",
            ["a", "held", "8.0 GB free < 10 GB floor · `drover clean --yes`"],
        ),
        (
            Some("ALL_CAPPED"),
            "start",
            12,
            r#"REFUSED ALL_CAPPED: every account ["A", "B"] is capped; the first resets in 3h"#,
            "serial",
            [
                "a",
                "waiting",
                r#"every account ["A", "B"] is capped · the first resets in 3h"#,
            ],
        ),
        (
            Some("CAP"),
            "start",
            12,
            "REFUSED CAP: 2 impl lane(s) open in synergy, max_impl = 2",
            "serial",
            [
                "a",
                "waiting",
                "2 impl lane(s) open in synergy, max_impl = 2 · starts when a lane finishes",
            ],
        ),
        (
            Some("ROUND_LIMIT"),
            "merge",
            12,
            "REFUSED ROUND_LIMIT: review rounds ran out (4/3)",
            "serial",
            [
                "r",
                "needs you",
                "#12: review rounds ran out (4/3) · your call on its card",
            ],
        ),
        (
            Some("UPDATE_LIMIT"),
            "conflict",
            12,
            "REFUSED UPDATE_LIMIT: 3 update rounds passed",
            "serial",
            ["r", "needs you", "#12: 3 update rounds passed · your call on its card"],
        ),
        (
            Some("WORKTREE_DIRTY"),
            "start",
            12,
            "REFUSED WORKTREE_DIRTY: /w/issue-12 is no fleet lane and has 2 dirty files",
            "serial",
            [
                "r",
                "needs you",
                "#12: /w/issue-12 is no fleet lane and has 2 dirty files · `drover kill` it, or spawn `--supersede`",
            ],
        ),
        (
            Some("CI_NOT_GREEN"),
            "merge",
            12,
            "REFUSED CI_NOT_GREEN: ui is pending at abc",
            "serial",
            ["a", "waiting", "#12's CI is not green yet"],
        ),
        (
            Some("NO_VERDICT"),
            "merge",
            12,
            "REFUSED NO_VERDICT: no verdict recorded",
            "serial",
            ["a", "waiting", "#12 has no PASS for its head · a new round"],
        ),
        (
            Some("STALE_VERDICT"),
            "merge",
            12,
            "REFUSED STALE_VERDICT: no verdict for head abc",
            "serial",
            ["a", "waiting", "#12 has no PASS for its head · a new round"],
        ),
        (
            Some("VERDICT_FAIL"),
            "merge",
            12,
            "REFUSED VERDICT_FAIL: verdict for abc is FAIL",
            "serial",
            ["a", "waiting", "#12 has no PASS for its head · a new round"],
        ),
        (
            Some("PAUSED"),
            "start",
            12,
            "REFUSED PAUSED: synergy is paused; `drover resume synergy` first",
            "serial",
            ["m", "paused", "`drover resume synergy` first"],
        ),
        (
            Some("EXIT"),
            "start",
            138,
            exit4,
            "serial",
            ["g", "started", "#138 · prompt not confirmed yet"],
        ),
        (
            Some("EXIT"),
            "merge",
            12,
            "exit 1: gh: not found",
            "serial",
            ["r", "failed", "exit 1: gh: not found"],
        ),
        (
            Some("ERROR"),
            "start",
            12,
            "error: herdr: gone",
            "serial",
            ["r", "failed", "error: herdr: gone"],
        ),
        (
            None,
            "start",
            138,
            r#""drover-impl-138" ["claude" "fleet-impl"]"#,
            "serial",
            ["g", "started", "#138"],
        ),
        (None, "respawn", 138, "x", "serial", ["g", "started", "#138"]),
        (None, "review", 129, "x", "serial", ["g", "started", "review #129"]),
        (None, "fix", 129, "x", "serial", ["g", "started", "fix #129"]),
        (None, "conflict", 129, "x", "serial", ["g", "started", "update #129"]),
        (None, "merge", 129, "merged #129", "serial", ["g", "merged", "#129"]),
        (
            None,
            "clean",
            129,
            "removed 2 lanes",
            "serial",
            ["g", "cleaned", "merged lanes"],
        ),
        (
            None,
            "ci-red",
            129,
            "verdict FAIL",
            "serial",
            ["a", "sent back", "#129: red CI, a fix round next"],
        ),
        (
            Some("NO_HOST"),
            "start",
            12,
            "REFUSED NO_HOST: no host with docker (cloud host stopped or over budget)",
            "serial",
            [
                "a",
                "waiting",
                "no host with docker (cloud host stopped or over budget)",
            ],
        ),
    ];
    for (code, step, issue, result, serial, want) in golden {
        let got = said(*code, step, *issue, result, serial);
        assert_eq!(
            (got.0.as_str(), got.1.as_str(), got.2.as_str()),
            (want[0], want[1], want[2]),
            "{code:?} {step}"
        );
    }
}
