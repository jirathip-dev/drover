mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;
use std::io::Write;
use std::process::{Command, Stdio};

const HEAD: &str = "1111111111111111111111111111111111111111";
const BODY: &str = "Adds a key.\n\n## For humans\nIt works.\n\n## Rollout\nIn sendmeter:\n```toml\n[fleet.sendmeter]\nmax_rev = 2\n```\n\n## Tests\nran\n";
const SNIPPET: &str = "In sendmeter:\n```toml\n[fleet.sendmeter]\nmax_rev = 2\n```";
const FAIL: &str =
    "FAIL\n\n- blocking [ac] src/x.rs:3: wrong\n  Proof: spec\n  > AC1: it works\n  src/x.rs:3 breaks it\n";

fn setup(name: &str, b: &FakeShed) -> drover::Ctx {
    setup_with(name, b, CONFIG)
}

fn setup_with(name: &str, b: &FakeShed, base: &str) -> drover::Ctx {
    let cfg = format!(
        "{base}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\n\n\
         [fleet.sendmeter.runtime]\nkind = \"shed-acp\"\nsocket = \"{}\"\n",
        b.sock.display()
    );
    let mut c = ctx(&tmpdir(name), &cfg);
    c.exe = env!("CARGO_BIN_EXE_drover").into();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    for t in ["t1", "t2"] {
        let (_, out) = run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Work"]);
        assert_eq!(out, format!("{t}\n"));
    }
    let spawn = json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"});
    append(&c.state, &Event::new(10, "sendmeter", Some(12), "spawn", spawn)).unwrap();
    let pass = json!({"verdict": "PASS", "sha": HEAD});
    append(&c.state, &Event::new(20, "sendmeter", Some(12), "verdict", pass)).unwrap();
    c
}

fn shed() -> Fake {
    let agents = json!([{"id": "claude-acp", "version": "0.101.0", "distribution": "npx", "cmd": ["echo"]}]);
    Fake::default()
        .reply("shed help run", "--meta <JSON>")
        .reply("shed agents --json", &agents.to_string())
        .reply("shed run", "")
        .reply("shed send", "")
}

fn merge(c: &drover::Ctx, b: &FakeShed, body: &str) -> Fake {
    let checks = json!([{"__typename": "CheckRun", "name": "build", "status": "COMPLETED", "conclusion": "SUCCESS"}]);
    let pr = json!({"number": 34, "state": "OPEN", "baseRefName": "staging", "headRefOid": HEAD, "statusCheckRollup": checks});
    let r = shed()
        .reply("gh pr view issue-12", &pr.to_string())
        .reply("gh pr merge 34", "")
        .reply(
            "gh pr view 34",
            &json!({"mergeCommit": {"oid": "m3rg3"}, "body": body}).to_string(),
        )
        .reply("gh issue", "");
    let (code, out) = run(c, &b.cli(&r), &["merge", "sendmeter", "12"]);
    assert_eq!(code, 0, "{out}");
    r
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    let all = read(&c.state).unwrap().to_vec();
    all.into_iter().filter(|e| e.kind == kind).collect()
}

fn rows(c: &drover::Ctx, t: &str) -> Vec<String> {
    let msgs = kinds(c, "thread-msg")
        .into_iter()
        .filter(|e| e.str("thread") == Some(t));
    msgs.map(|e| e.str("text").unwrap_or_default().to_string()).collect()
}

fn hook(c: &drover::Ctx, thread: &str, input: &serde_json::Value) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_drover"));
    for k in ["FLEET_LANE", "DROVER_OUTBOX", "SHED", "BERTH"] {
        cmd.env_remove(k);
    }
    let mut child = (cmd.args(["hook", "--role", "orchestrator"]))
        .env("DROVER_LANE", "sendmeter:-:orchestrator")
        .env("DROVER_THREAD", thread)
        .env("FLEET_CONFIG", &c.config)
        .env("FLEET_STATE", &c.state)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let o = child.wait_with_output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn filed_in(c: &drover::Ctx, thread: &str) {
    let input = json!({"hook_event_name": "PostToolUse", "tool_name": "Bash",
        "tool_input": {"command": "gh issue create -R jirathip-dev/sendmeter -t \"Swift key\" -F -"},
        "tool_response": {"stdout": "https://github.com/jirathip-dev/sendmeter/issues/12\n", "stderr": "", "interrupted": false}});
    hook(c, thread, &input);
    assert_eq!(kinds(c, "issue-filed").len(), 1);
}

fn cards(c: &drover::Ctx) -> Vec<(Event, drover::thread::Row)> {
    let all = read(&c.state).unwrap().to_vec();
    let rows = drover::thread::rows(&all);
    let card = |r: &&(&Event, drover::thread::Row)| r.1.kind == drover::thread::RowKind::ConfigCard;
    rows.iter()
        .filter(card)
        .map(|(e, r)| ((*e).clone(), r.clone()))
        .collect()
}

fn to(id: &str) -> drover::thread::Target {
    drover::thread::Target::Thread { id: id.into() }
}

#[test]
fn a_merged_rollout_is_a_config_card_in_the_filing_thread_whose_diff_is_its_lines() {
    let b = FakeShed::new();
    let c = setup("rollout-filed", &b);
    filed_in(&c, "t1");
    let (t1, t2) = (rows(&c, "t1").len(), rows(&c, "t2").len());
    let r = merge(&c, &b, BODY);
    let open = kinds(&c, "rollout-open");
    assert_eq!(open.len(), 1);
    assert_eq!((open[0].issue, open[0].str("to")), (Some(12), Some("t1")));
    assert_eq!(open[0].str("snippet"), Some(SNIPPET));
    let cards = cards(&c);
    assert_eq!(cards.len(), 1, "{cards:?}");
    let (e, row) = &cards[0];
    assert_eq!((row.to.clone(), row.open, row.issue), (to("t1"), true, None));
    assert_eq!(e.str("diff"), Some("  [fleet.sendmeter]\n+ max_rev = 2\n"));
    assert_eq!((e.str("id"), e.val("rollout").as_u64()), (Some("c1"), Some(12)));
    assert!(e.str("reason").is_none(), "{e:?}");
    assert_eq!(rows(&c, "t1").len(), t1, "no orchestrator is asked to draft it");
    assert_eq!(rows(&c, "t2").len(), t2, "the newer thread gets nothing");
    let sent = r.calls.borrow().join("\n");
    assert!(!sent.contains("orchestrator-t1"), "{sent}");
}

#[test]
fn with_no_filing_thread_the_card_goes_to_the_repos_newest() {
    let b = FakeShed::new();
    let c = setup("rollout-newest", &b);
    merge(&c, &b, BODY);
    assert_eq!(kinds(&c, "rollout-open")[0].str("to"), Some("t2"));
    assert_eq!(cards(&c)[0].1.to, to("t2"));
}

#[test]
fn no_rollout_section_or_none_posts_nothing() {
    for (i, body) in ["## For humans\nx\n", "## Rollout\nnone\n\n## Tests\nok\n"]
        .iter()
        .enumerate()
    {
        let b = FakeShed::new();
        let c = setup(&format!("rollout-none-{i}"), &b);
        let before = kinds(&c, "thread-msg").len();
        merge(&c, &b, body);
        assert_eq!(kinds(&c, "merged").len(), 1);
        assert!(kinds(&c, "rollout-open").is_empty(), "{body}");
        assert!(kinds(&c, "config-propose").is_empty(), "{body}");
        assert_eq!(kinds(&c, "thread-msg").len(), before, "{body}");
    }
}

#[test]
fn a_rollout_that_does_not_apply_cleanly_is_a_card_naming_why_with_no_diff() {
    let cases = [
        (
            "[fleet.sendmeter]\nmerge_label = \"merge-ready\"",
            "fleet.sendmeter.merge_label is already \"off\", not \"merge-ready\"",
        ),
        ("[fleet.nope]\nmax_rev = 2", "no fleet nope in fleet.toml"),
    ];
    for (i, (lines, why)) in cases.iter().enumerate() {
        let b = FakeShed::new();
        let c = setup(&format!("rollout-unclean-{i}"), &b);
        let before = std::fs::read_to_string(&c.config).unwrap();
        merge(
            &c,
            &b,
            &format!("## For humans\nx\n\n## Rollout\n```toml\n{lines}\n```\n"),
        );
        let cards = cards(&c);
        assert_eq!(cards.len(), 1, "{cards:?}");
        let (e, row) = &cards[0];
        assert_eq!((e.str("reason"), e.str("diff")), (Some(*why), None));
        assert!(row.text.contains(why), "{}", row.text);
        let (code, out) = run(&c, &b.cli(&shed()), &["config", "apply", "c1"]);
        assert!(
            code == 3 && out.contains("REFUSED BAD_VALUE") && out.contains(why),
            "{out}"
        );
        assert_eq!(std::fs::read_to_string(&c.config).unwrap(), before);
        let (text, _) = waiting(&c);
        assert!(
            text.contains(&format!("rollout not applied: {why}"))
                && text.contains("→ drover rollout dismiss sendmeter 12"),
            "{text}"
        );
    }
}

fn nudges(c: &drover::Ctx) -> Vec<drover::thread::Row> {
    let all = read(&c.state).unwrap().to_vec();
    let rows = drover::thread::rows(&all).into_iter().map(|r| r.1);
    rows.filter(|r| r.text.contains("drover job sendmeter lesson") || r.text.contains("gh pr create"))
        .collect()
}

fn attach(c: &drover::Ctx) {
    let (code, out) = run(c, &Fake::default(), &["attach", "sendmeter", "chat1"]);
    assert_eq!(code, 0, "{out}");
}

fn fails(c: &drover::Ctx, n: u64) {
    let rev = json!({"role": "rev", "stage": "rev", "check": true, "agent": "sendmeter-rev-12"});
    append(&c.state, &Event::new(30, "sendmeter", Some(12), "spawn", rev)).unwrap();
    let f = c.config.with_file_name("verdict.md");
    std::fs::write(&f, FAIL).unwrap();
    for i in 0..n {
        let sha = format!("{i}").repeat(40);
        let r = Fake::default()
            .reply(
                "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
                &format!("{sha}\n"),
            )
            .reply("gh api -X POST", "{}");
        let args = [
            "verdict",
            "sendmeter",
            "12",
            "FAIL",
            "--sha",
            &sha,
            "--file",
            f.to_str().unwrap(),
        ];
        let (code, out) = run(c, &r, &args);
        assert_eq!(code, 0, "{out}");
    }
}

#[test]
fn a_lessons_nudge_is_an_owner_card_with_no_attached_chat_and_a_row_to_one() {
    let b = FakeShed::new();
    let c = setup("nudge-lessons-card", &b);
    fails(&c, 1);
    assert!(nudges(&c).is_empty(), "one FAIL nudges nothing");
    fails(&c, 1);
    let n = nudges(&c);
    assert_eq!(n.len(), 1, "{n:?}");
    assert_eq!(
        n[0].to,
        drover::thread::Target::Desk {
            repo: "sendmeter".into()
        }
    );
    assert!(n[0].prompt.is_none() && n[0].push.is_some(), "{:?}", n[0]);
    assert!(n[0].text.contains("`drover job sendmeter lesson`"), "{}", n[0].text);

    let c = setup("nudge-lessons-chat", &b);
    attach(&c);
    fails(&c, 2);
    let n = nudges(&c);
    assert_eq!(n.len(), 1, "{n:?}");
    assert_eq!(
        (n[0].to.clone(), n[0].kind),
        (to("chat1"), drover::thread::RowKind::System)
    );
    let prompt = n[0].prompt.clone().unwrap_or_default();
    assert!(prompt.contains("`drover job sendmeter lesson`"), "{prompt}");
}

#[test]
fn a_promotion_nudge_is_an_owner_card_with_no_attached_chat_and_a_row_to_one() {
    let cmd = "`gh pr create -R jirathip-dev/sendmeter -B main -H staging";
    let cfg = CONFIG.replace("stall_minutes = 30", "stall_minutes = 30\npromote_after_merges = 1");
    let b = FakeShed::new();
    let c = setup_with("nudge-promote-card", &b, &cfg);
    merge(&c, &b, "## For humans\nx\n");
    let n = nudges(&c);
    assert_eq!(n.len(), 1, "{n:?}");
    assert_eq!(
        n[0].to,
        drover::thread::Target::Desk {
            repo: "sendmeter".into()
        }
    );
    assert!(n[0].prompt.is_none() && n[0].text.contains(cmd), "{:?}", n[0]);

    let c = setup_with("nudge-promote-chat", &b, &cfg);
    attach(&c);
    merge(&c, &b, "## For humans\nx\n");
    let n = nudges(&c);
    assert_eq!(n.len(), 1, "{n:?}");
    assert_eq!(n[0].to, to("chat1"));
    assert!(n[0].prompt.as_deref().is_some_and(|p| p.contains(cmd)), "{:?}", n[0]);
}

fn waiting(c: &drover::Ctx) -> (String, serde_json::Value) {
    let (code, text) = run(c, &Fake::default(), &["waiting", "sendmeter"]);
    assert_eq!(code, 0, "{text}");
    let (code, j) = run(c, &Fake::default(), &["waiting", "--json"]);
    assert_eq!(code, 0, "{j}");
    (text, serde_json::from_str(&j).unwrap())
}

#[test]
fn the_owners_apply_of_drovers_rollout_card_sets_its_lines_and_closes_the_rollout() {
    let b = FakeShed::new();
    let c = setup("rollout-apply", &b);
    filed_in(&c, "t1");
    let before = std::fs::read_to_string(&c.config).unwrap();
    merge(&c, &b, BODY);
    let (text, j) = waiting(&c);
    assert!(
        text.contains("sendmeter #12 owner: rollout not applied (card c1)")
            && text.contains("→ drover config apply c1"),
        "{text}"
    );
    let all = j.as_array().unwrap();
    let row = all.iter().find(|w| w["issue"] == 12).unwrap();
    assert_eq!(
        (&row["lane"], &row["who"]),
        (&json!("orchestrator-t1"), &json!("owner"))
    );
    assert!(row["since"].as_u64().is_some(), "{row}");

    let file = c.config.with_file_name("proposed.toml");
    std::fs::write(&file, before.replacen("stall_minutes = 30", "stall_minutes = 31", 1)).unwrap();
    let f = file.to_str().unwrap();
    let (code, out) = run(&c, &Fake::default(), &["config", "propose", "t1", f, "--rollout", "99"]);
    assert!(code == 3 && out.contains("REFUSED NO_SUCH_ROLLOUT"), "{out}");
    assert!(
        kinds(&c, "rollout-done").is_empty(),
        "posting the card alone closes nothing"
    );
    let (code, out) = run(&c, &b.cli(&shed()), &["config", "apply", "c1"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.trim_end().ends_with(", rollout #12 done"), "{out}");
    let done = kinds(&c, "rollout-done");
    assert_eq!(
        (done.len(), done[0].issue, done[0].str("config")),
        (1, Some(12), Some("c1"))
    );
    let after = std::fs::read_to_string(&c.config).unwrap();
    assert_eq!(
        drover::propose::diff(&before, &after),
        "  [fleet.sendmeter]\n+ max_rev = 2\n"
    );
    let (text, j) = waiting(&c);
    assert!(!text.contains("rollout"), "{text}");
    assert!(!j.as_array().unwrap().iter().any(|w| w["issue"] == 12), "{j}");
}

#[test]
fn the_owner_dismisses_an_open_rollout_with_by() {
    let b = FakeShed::new();
    let c = setup("rollout-dismiss", &b);
    merge(&c, &b, BODY);
    let dismiss = |by: &[&str]| {
        let args = [&["rollout", "dismiss", "sendmeter", "12"][..], by].concat();
        run(&c, &Fake::default(), &args)
    };
    assert_eq!(
        dismiss(&["--by", "alice"]),
        (0, "rollout #12 dismissed by alice\n".into())
    );
    let d = kinds(&c, "rollout-dismissed");
    assert_eq!((d.len(), d[0].issue, d[0].str("by")), (1, Some(12), Some("alice")));
    assert!(!waiting(&c).0.contains("rollout"));
    let (code, out) = dismiss(&[]);
    assert!(code == 3 && out.contains("REFUSED NO_SUCH_ROLLOUT"), "{out}");
    let cmd = format!("drover rollout {} sendmeter 12", "dismiss");
    let input = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": cmd}});
    assert!(
        hook(&c, "t1", &input).contains("REFUSED OWNER_ONLY"),
        "an orchestrator never dismisses"
    );
}
