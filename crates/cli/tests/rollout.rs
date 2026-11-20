mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;
use std::io::Write;
use std::process::{Command, Stdio};

const HEAD: &str = "1111111111111111111111111111111111111111";
const BODY: &str = "Adds a key.\n\n## For humans\nIt works.\n\n## Rollout\nIn every Swift fleet:\n```toml\n[fleet.sendmeter]\nswift = \"6.1\"\n```\n\n## Tests\nran\n";
const SNIPPET: &str = "In every Swift fleet:\n```toml\n[fleet.sendmeter]\nswift = \"6.1\"\n```";

fn setup(name: &str, b: &FakeShed) -> drover::Ctx {
    let cfg = format!(
        "{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\naccount = \"C\"\n\n\
         [fleet.sendmeter.runtime]\nkind = \"shed-acp\"\nsocket = \"{}\"\n\n[accounts]\nC = \"C\"\n",
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

fn filed_in(c: &drover::Ctx, thread: &str) {
    let input = json!({"hook_event_name": "PostToolUse", "tool_name": "Bash",
        "tool_input": {"command": "gh issue create -R jirathip-dev/sendmeter -t \"Swift key\" -F -"},
        "tool_response": {"stdout": "https://github.com/jirathip-dev/sendmeter/issues/12\n", "stderr": "", "interrupted": false}});
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
    assert_eq!(kinds(c, "issue-filed").len(), 1);
}

#[test]
fn a_merged_rollout_is_one_row_in_the_filing_thread_waking_its_orchestrator() {
    let b = FakeShed::new();
    let c = setup("rollout-filed", &b);
    filed_in(&c, "t1");
    let (t1, t2) = (rows(&c, "t1").len(), rows(&c, "t2").len());
    let r = merge(&c, &b, BODY);
    let open = kinds(&c, "rollout-open");
    assert_eq!(open.len(), 1);
    assert_eq!((open[0].issue, open[0].str("to")), (Some(12), Some("t1")));
    assert_eq!(open[0].str("snippet"), Some(SNIPPET));
    let new: Vec<String> = rows(&c, "t1").into_iter().skip(t1).collect();
    assert_eq!(new.len(), 1, "{new:?}");
    assert!(
        new[0].starts_with("#12 merged with a rollout, not applied yet:\n"),
        "{}",
        new[0]
    );
    assert!(new[0].contains(SNIPPET) && new[0].contains("drover config propose t1 <file> --rollout 12"));
    assert_eq!(rows(&c, "t2").len(), t2, "the newer thread gets nothing");
    let sent = r.calls.borrow().join("\n");
    assert!(
        sent.contains("orchestrator-t1") && sent.contains("swift = \"6.1\""),
        "{sent}"
    );
}

#[test]
fn with_no_filing_thread_the_row_goes_to_the_repos_newest() {
    let b = FakeShed::new();
    let c = setup("rollout-newest", &b);
    let t2 = rows(&c, "t2").len();
    merge(&c, &b, BODY);
    assert_eq!(kinds(&c, "rollout-open")[0].str("to"), Some("t2"));
    assert_eq!(rows(&c, "t2").len(), t2 + 1);
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
        assert_eq!(kinds(&c, "thread-msg").len(), before, "{body}");
    }
}

fn waiting(c: &drover::Ctx) -> (String, serde_json::Value) {
    let (code, text) = run(c, &Fake::default(), &["waiting", "sendmeter"]);
    assert_eq!(code, 0, "{text}");
    let (code, j) = run(c, &Fake::default(), &["waiting", "--json"]);
    assert_eq!(code, 0, "{j}");
    (text, serde_json::from_str(&j).unwrap())
}

#[test]
fn a_config_card_naming_the_rollout_closes_it_on_apply() {
    let b = FakeShed::new();
    let c = setup("rollout-apply", &b);
    filed_in(&c, "t1");
    merge(&c, &b, BODY);
    let (text, j) = waiting(&c);
    assert!(
        text.contains("sendmeter #12 owner: rollout not applied (thread t1)")
            && text.contains("→ drover config propose t1 <file> --rollout 12"),
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
    let old = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(&file, old.replacen("review_rounds = 3", "review_rounds = 4", 1)).unwrap();
    let f = file.to_str().unwrap();
    let propose = |n: &str| run(&c, &Fake::default(), &["config", "propose", "t1", f, "--rollout", n]);
    let (code, out) = propose("99");
    assert!(code == 3 && out.contains("REFUSED NO_SUCH_ROLLOUT"), "{out}");
    assert_eq!(propose("12").1, "c1\n");
    assert!(kinds(&c, "rollout-done").is_empty(), "proposing alone closes nothing");
    let (code, out) = run(&c, &b.cli(&shed()), &["config", "apply", "c1"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.trim_end().ends_with(", rollout #12 done"), "{out}");
    let done = kinds(&c, "rollout-done");
    assert_eq!(
        (done.len(), done[0].issue, done[0].str("config")),
        (1, Some(12), Some("c1"))
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
}
