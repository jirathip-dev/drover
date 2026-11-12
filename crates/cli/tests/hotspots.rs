mod common;
use common::*;
use herdsman::events::{append, read, Event};
use herdsman::runner::{Out, Real, Runner};
use serde_json::{json, Value};
use std::path::Path;

const NOW: u64 = 1_700_000_000;
const HOUR: u64 = 3600;
const SECOND: &str = "\n[fleet.mirror]\nrepo = \"jirathip-dev/sendmeter\"\nlocal = \"/tmp/does-not-matter/mirror\"\nintegration = \"staging\"\nhotspots = \"off\"\n[fleet.mirror.role.impl]\nharness = \"hermes\"\n";

struct RealGit(Fake);
impl Runner for RealGit {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        if prog == "git" {
            return Real.run(prog, args);
        }
        self.0.run(prog, args)
    }
    fn sleep(&self, _: u64) {}
}

fn git(dir: &Path, at: u64, args: &[&str]) -> String {
    let date = format!("@{at} +0000");
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn pr(dir: &Path, at: u64, k: usize, files: &[&str]) -> String {
    git(dir, at, &["checkout", "-qb", &format!("pr{k}"), "staging"]);
    for f in files {
        std::fs::create_dir_all(dir.join(f).parent().unwrap()).unwrap();
        std::fs::write(dir.join(f), format!("{k}\n")).unwrap();
    }
    git(dir, at, &["add", "."]);
    git(dir, at, &["commit", "-qm", &format!("pr {k}")]);
    git(dir, at, &["checkout", "-q", "staging"]);
    git(
        dir,
        at,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            &format!("merge pr {k}"),
            &format!("pr{k}"),
        ],
    );
    git(dir, at, &["rev-parse", "HEAD"])
}

fn ev(c: &herdsman::Ctx, fleet: &str, ts: u64, issue: Option<u64>, kind: &str, d: Value) {
    append(&c.state, &Event::new(ts, fleet, issue, kind, d)).unwrap();
}

fn fixture(name: &str) -> herdsman::Ctx {
    let d = tmpdir(name);
    let local = d.join("local");
    std::fs::create_dir_all(&local).unwrap();
    git(&local, NOW, &["init", "-q", "-b", "staging"]);
    git(&local, NOW, &["config", "user.email", "t@example.com"]);
    git(&local, NOW, &["config", "user.name", "t"]);
    std::fs::write(local.join("a.rs"), "\n").unwrap();
    git(&local, NOW - 30 * 86400, &["add", "."]);
    git(&local, NOW - 30 * 86400, &["commit", "-qm", "base"]);
    pr(&local, NOW - 20 * 86400, 0, &["b.rs"]);
    let mut shas = vec![];
    for k in 1..=5 {
        let files: &[&str] = if k < 5 {
            &["a.rs", "b.rs"]
        } else {
            &["a.rs", "web/app.ts"]
        };
        shas.push(pr(&local, NOW - (10 - k as u64) * 86400, k, files));
    }
    git(&local, NOW, &["update-ref", "refs/remotes/origin/staging", "HEAD"]);
    let cfg = format!("{CONFIG}{SECOND}").replace("/tmp/does-not-matter/sendmeter", local.to_str().unwrap());
    let c = ctx(&d, &cfg);
    let c = herdsman::Ctx { now: NOW, ..c };
    ev(
        &c,
        "sendmeter",
        NOW - 20 * 86400,
        Some(9),
        "conflict-predicted",
        json!({"other": 0, "files": ["a.rs"]}),
    );
    ev(
        &c,
        "sendmeter",
        NOW - HOUR,
        Some(12),
        "conflict-predicted",
        json!({"other": 13, "files": ["a.rs", "b.rs"]}),
    );
    ev(
        &c,
        "mirror",
        NOW - HOUR,
        Some(30),
        "conflict-predicted",
        json!({"other": 0, "files": ["a.rs"]}),
    );
    ev(
        &c,
        "sendmeter",
        NOW - HOUR,
        Some(14),
        "conflict-predicted",
        json!({"other": 0, "files": ["b.rs"]}),
    );
    for _ in 0..2 {
        ev(
            &c,
            "sendmeter",
            NOW - 2 * 86400,
            Some(12),
            "spawn",
            json!({"role": "impl", "update": true}),
        );
    }
    ev(
        &c,
        "sendmeter",
        NOW - 2 * 86400,
        Some(12),
        "spawn",
        json!({"role": "impl"}),
    );
    ev(
        &c,
        "sendmeter",
        NOW - 86400,
        Some(12),
        "merged",
        json!({"merge_sha": shas[2]}),
    );
    c
}

fn gh(open: &str) -> Fake {
    Fake::default()
        .reply("gh issue list", open)
        .reply(
            "gh issue create",
            "https://github.com/jirathip-dev/sendmeter/issues/77\n",
        )
        .reply("gh issue edit", "")
}

#[test]
fn reports_only_files_five_merged_prs_touched_with_their_conflicts_and_updates() {
    let c = fixture("hotspots-report");
    let r = RealGit(gh("[]"));
    let (code, out) = run(&c, &r, &["hotspots", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("| `a.rs` | 5 | 2 | 2 |"), "{out}");
    assert!(!out.contains("b.rs") && !out.contains("app.ts"), "{out}");
    let stored: Value =
        serde_json::from_str(&std::fs::read_to_string(herdsman::hotspots::path(&c, "sendmeter")).unwrap()).unwrap();
    assert_eq!(
        stored,
        json!([{"file": "a.rs", "prs": 5, "conflicts": 2, "updates": 2}])
    );
}

#[test]
fn a_second_run_in_the_week_updates_the_open_issue_queued_architect() {
    let c = fixture("hotspots-week");
    let title = "hotspots: sendmeter week of 2023-11-13";
    let r = RealGit(gh("[]"));
    let (code, out) = run(&c, &r, &["hotspots", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&format!("filed sendmeter#77: {title}")), "{out}");
    let open =
        json!([{"number": 70, "title": "hotspots: sendmeter week of 2023-11-06"}, {"number": 77, "title": title}]);
    let r = RealGit(gh(&open.to_string()));
    let later = herdsman::Ctx {
        now: NOW + 86400,
        ..c.clone()
    };
    let (code, out) = run(&later, &r, &["hotspots", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(!r.0.called("gh issue create"), "{out}");
    assert!(
        r.0.called("gh issue edit 77 -R jirathip-dev/sendmeter --body Hot files in sendmeter"),
        "{out}"
    );
    let q = herdsman::queue::replay(&read(&c.state).unwrap(), "sendmeter");
    assert_eq!(
        q.iter().map(|i| (i.issue, i.architect)).collect::<Vec<_>>(),
        [(77, true)]
    );
}

#[test]
fn tick_skips_a_fleet_with_hotspots_off() {
    let c = fixture("hotspots-off");
    let r = RealGit(
        gh("[]")
            .reply("herdr status", "server:\n  status: running\n")
            .reply("herdr agent list", NO_AGENTS)
            .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n"),
    );
    let (code, out) = run(&c, &r, &["tick"]);
    assert_eq!(code, 0, "{out}");
    assert!(herdsman::hotspots::path(&c, "sendmeter").exists(), "{out}");
    assert!(!herdsman::hotspots::path(&c, "mirror").exists(), "{out}");
    assert!(
        r.0.called("gh issue create -R jirathip-dev/sendmeter --title hotspots: sendmeter"),
        "{out}"
    );
}
