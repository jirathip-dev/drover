//! #228: a `FAIL (form-only)` verdict whose blockers are all form categories starts a fix round that doesn't count
//! toward `review_rounds`, at most `form_rounds` times per issue.
mod common;
use common::*;
use drover::config::Config;
use drover::events::{append, fold, read, Event, IssueState};
use serde_json::json;

// #418: each blocker carries a Proof block, spec (a quoted rule + file:line) or run (a fenced command + its output).
const FORM: &str = "FAIL (form-only)\n\n- blocking [pr-body] PR body: no `## For humans`\n  Proof: spec\n  \
> The PR body has a `## For humans` section\n  PR#34:1 has none\n- blocking [format] src/x.rs:3: cargo fmt\n  \
Proof: run\n  ```\n  cargo fmt --check\n  ```\n  ```\n  Diff in src/x.rs:3\n  ```\n- non-blocking src/y.rs:1: a nit\n";
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
    assert_eq!(ev.last().unwrap().data["form_only"], true);
    // The next review is still round 1 of 3, and a plain FAIL there counts.
    let out = verdict(
        &c,
        &d,
        'b',
        &format!("FAIL\n\n- blocking [code] src/x.rs:9: wrong\n{SPEC}"),
    );
    assert!(out.contains("(round 1/3)"), "{out}");
    assert_eq!(state(&c).state, "fix(2)");
}

#[test]
fn one_substantive_or_untagged_blocker_counts() {
    let (c, d) = fixture("form-mixed", "");
    let out = verdict(
        &c,
        &d,
        'a',
        &format!("{FORM}- blocking [code] src/x.rs:9: wrong result\n{SPEC}"),
    );
    assert!(
        out.contains("form-only-rejected") && out.contains("(round 1/3)"),
        "{out}"
    );
    assert_eq!(
        (state(&c).state.as_str(), logged(&c, "form-only-rejected")),
        ("fix(1)", 1)
    );
    let out = verdict(&c, &d, 'b', &format!("{FORM}- blocking src/x.rs:9: no tag\n{SPEC}"));
    assert!(out.contains("(round 2/3)"), "{out}");
    let s = state(&c);
    assert_eq!((s.form, logged(&c, "form-only-rejected")), (0, 2));
    assert_eq!(read(&c.state).unwrap().last().unwrap().data["form_only"], false);
}

#[test]
fn past_form_rounds_they_count_and_needs_you_says_how_many_were_form() {
    let (c, d) = fixture("form-bound", "form_rounds = 1\n");
    assert!(verdict(&c, &d, 'a', FORM).contains("not counted · form 1/1"));
    let out = verdict(&c, &d, 'b', FORM);
    assert!(
        out.contains("form-rounds-exhausted") && out.contains("(round 1/3)"),
        "{out}"
    );
    assert_eq!(logged(&c, "form-rounds-exhausted"), 1);
    verdict(&c, &d, 'c', FORM);
    let out = verdict(&c, &d, 'd', FORM);
    assert!(out.contains("(round 3/3)") && out.contains("needs-human"), "{out}");
    let ev = read(&c.state).unwrap();
    let nh = ev.iter().rfind(|e| e.kind == "needs-human").unwrap();
    assert_eq!(
        nh.str("reason"),
        Some("review rounds ran out (3/3, 1 form-only not counted)")
    );
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
    let out = verdict(&c, &d, 'a', FORM);
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
        "FAIL (form-only)\n- **blocking** [format] a.rs:1\n- non-blocking [code] b.rs:2\n"
    ));
    assert!(!ok("FAIL (form-only)\n- non-blocking [format] a.rs:1\n"), "no blocker");
    assert!(!ok("FAIL (form-only)\n- blocking [code] a.rs:1\n"));
    // #360: the verdict's anti-patterns line is no finding.
    let line = "Tests checked against anti-patterns: yes\n";
    assert!(ok(&format!("FAIL (form-only)\n{line}- blocking [format] a.rs:1\n")));
}

/// #360: a verdict.md carrying `Tests checked against anti-patterns: yes` records as usual.
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
fn the_card_says_not_counted_in_amber() {
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
    let page = queue_tab(&s, "127.0.0.1:7777", c.now); // #268: the board's Queue tab
    assert!(
        page.contains("rev <span class=a>FAIL (form-only)</span> r1 · <span class=a>not counted · form 1/3</span>"),
        "{page}"
    );
}

struct OnShed(Fake, FakeShed);

impl OnShed {
    fn new(f: Fake) -> OnShed {
        OnShed(f, FakeShed::new())
    }
}

impl std::ops::Deref for OnShed {
    type Target = Fake;
    fn deref(&self) -> &Fake {
        &self.0
    }
}

impl drover::runner::Runner for OnShed {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        self.0.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<drover::runner::Out> {
        self.0.run_in(prog, args, input)
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
    fn shed(&self) -> Option<drover::shed::Shed> {
        Some(self.1.shed())
    }
}

/// #249: a `## Blocking` heading is no finding; tagged items under it are form-only, an untagged one is not.
#[test]
fn a_blocking_heading_is_ignored() {
    let cats = ["pr-body".to_string(), "docs-fresh".to_string()];
    let ok = |b: &str| drover::gate::form_only(b, &cats);
    let head = "FAIL (form-only)\n\n## Blocking\n- blocking [pr-body] body lags the head\n";
    assert!(ok(&format!("{head}- blocking [docs-fresh] README\n")));
    assert!(!ok(&format!("{head}- blocking README is stale\n")));
}
