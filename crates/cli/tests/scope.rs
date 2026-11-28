mod common;
use base64::Engine;
use common::*;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;
const LINE: &str = "the queue row loses its title at 390 px";
const BODY: &str =
    "## Problem\n\nThe row drops its title.\n\n## Wanted\n\nKeep it.\n\n## AC\n\n- a 390 px test\n\n## Notes\n\nnone";

fn req(s: &Server, method: &str, path: &str, body: &str, ck: &str) -> (u16, Value) {
    let r = format!(
        "{method} {path} HTTP/1.1\r\nHost: {ADDR}\r\n{ck}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&r, T)).into_owned();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    (
        head[9..12].parse().unwrap(),
        serde_json::from_str(body).unwrap_or(Value::Null),
    )
}

fn card(cards: &Value) -> Value {
    let all = cards.as_array().unwrap();
    assert_eq!(all.last().unwrap()["id"], "new-issue", "the last card: {cards}");
    all.last().unwrap().clone()
}

fn send(s: &Server, ck: &str, req_id: &str, text: &str, files: Option<(&str, &[u8])>) -> (u16, Value) {
    let mut v = json!({"text": text, "repo": "sendmeter", "req": req_id});
    if let Some((name, bytes)) = files {
        v["name"] = name.into();
        v["data"] = base64::engine::general_purpose::STANDARD.encode(bytes).into();
    }
    let a = json!({"screen": "cards", "block": "new-issue", "action": "new-issue", "values": v});
    req(s, "POST", "/action", &a.to_string(), ck)
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

fn assert_folded(folded: &Value) {
    assert_eq!(folded["kind"], "new-issue");
    assert_eq!(
        folded["fold"]["line"],
        "+ new issue · a line or a screenshot, for any repo"
    );
    let composer = folded["blocks"].as_array().unwrap().last().unwrap();
    assert_eq!(
        (&composer["action"], &composer["chips"][0]["all"]),
        (&json!("new-issue"), &json!(["sendmeter"]))
    );
    assert!(
        !folded.to_string().contains("A scope lane reads"),
        "no explanation line: {folded}"
    );
}

fn assert_brief(c: &drover::Ctx) {
    let brief = std::fs::read_to_string(c.worktrees.join("sendmeter/issue-0/.brief.md")).unwrap();
    assert!(
        brief.contains("# ROLE: SCOPE") && brief.contains(&format!("> {LINE}")),
        "{brief}"
    );
    assert!(brief.contains("drover draft sendmeter --title"), "{brief}");
    let shot = brief
        .lines()
        .find_map(|l| l.strip_prefix("- orders.png: `"))
        .and_then(|l| l.strip_suffix('`'))
        .unwrap_or_else(|| panic!("the screenshot is in the brief: {brief}"));
    assert_eq!(std::fs::read(shot).unwrap(), b"PNG bytes");
}

fn assert_waiting(open: &Value) {
    assert_eq!(open["fold"]["line"], "sendmeter: scoping, 1 waits");
    let rows = &open["blocks"][0]["rows"];
    assert_eq!(rows.as_array().map(Vec::len), Some(2), "{open}");
    assert_eq!(rows[1]["swipe"][0]["action"], "scope-drop");
    assert!(
        rows[1]["swipe"][0]["confirm"]["title"].is_string(),
        "[drop] has a confirm: {open}"
    );
    let drawn = drover_chat::text(open, 120, T).join("\n");
    for want in [
        "sendmeter: scoping, 1 waits",
        "sendmeter · a second line",
        "repo: sendmeter",
        "one line: the bug",
    ] {
        assert!(drawn.contains(want), "the board draws {want:?}:\n{drawn}");
    }
}

#[test]
fn new_issue_spawns_one_scope_lane_with_the_line_and_screenshot_in_its_brief_and_a_second_waits() {
    let c = ctx(&tmpdir("scope-new"), CONFIG);
    let sh = FakeShed::attached(&c);
    let r = Fake::default()
        .reply("git -C", "")
        .reply("which shed", "/opt/shed/bin/shed\n");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    assert_folded(&card(&req(&s, "GET", "/screens/cards", "", &ck).1));

    let (code, v) = send(&s, &ck, "new-1", LINE, Some(("orders.png", b"PNG bytes")));
    assert_eq!((code, &v["toast"]), (200, &json!("sendmeter: scoping")), "{v}");
    let cards = req(&s, "GET", "/screens/cards", "", &ck).1;
    let spawns = kinds(&c, "spawn");
    assert_eq!(spawns.len(), 1, "{spawns:?}");
    assert_eq!(
        (spawns[0].str("role"), spawns[0].str("agent"), spawns[0].issue),
        (Some("scope"), Some("sendmeter-scope-0"), Some(0))
    );
    assert!(sh.has("sendmeter-scope-0"), "{:?}", sh.methods());
    assert_brief(&c);
    assert_eq!(card(&cards)["fold"]["line"], "sendmeter: scoping");

    let (code, v) = send(&s, &ck, "new-2", "a second line", None);
    assert_eq!((code, &v["toast"]), (200, &json!("sendmeter: waits behind 1")), "{v}");
    let (_, again) = send(&s, &ck, "new-2", "a second line", None);
    assert_eq!(again["toast"], "sendmeter: already sent");
    let cards = req(&s, "GET", "/screens/cards", "", &ck).1;
    assert_eq!(kinds(&c, "spawn").len(), 1, "the second waits");
    assert_eq!(kinds(&c, "scope-request").len(), 2, "a resend logs nothing");
    assert_waiting(&card(&cards));

    let drop = json!({"screen": "cards", "block": "new-issue", "action": "scope-drop",
        "values": {"fleet": "sendmeter", "req": "new-1"}});
    let (code, v) = req(&s, "POST", "/action", &drop.to_string(), &ck);
    assert_eq!(code, 200, "{v}");
    req(&s, "GET", "/screens/cards", "", &ck);
    let killed = kinds(&c, "lane-killed");
    assert_eq!(killed.len(), 1, "the running scope's lane closes");
    let starts = kinds(&c, "scope-started");
    assert_eq!(
        starts.iter().map(|e| e.str("req")).collect::<Vec<_>>(),
        [Some("new-1"), Some("new-2")]
    );
    assert!(
        kinds(&c, "spawn").len() == 2 && !r.called("gh issue create"),
        "the second starts, nothing filed"
    );
}

fn running(c: &drover::Ctx, req: &str) {
    for (kind, d) in [
        ("scope-request", json!({"req": req, "text": LINE, "files": []})),
        ("scope-started", json!({"req": req})),
    ] {
        append(&c.state, &Event::new(T, "sendmeter", None, kind, d)).unwrap();
    }
    let sp = json!({"role": "scope", "agent": "sendmeter-scope-0", "pane": "sendmeter-scope-0", "branch": "issue-0"});
    append(&c.state, &Event::new(T, "sendmeter", Some(0), "spawn", sp)).unwrap();
}

fn draft(c: &drover::Ctx, r: &Fake, marks: &[&str]) -> String {
    let mut a = vec![
        "draft",
        "sendmeter",
        "--title",
        "fix: keep the row title at 390 px",
        "--body",
        BODY,
    ];
    for m in marks {
        a.extend(["--mark", m]);
    }
    let (code, out) = run(c, r, &a);
    assert_eq!(code, 0, "{out}");
    out.trim().to_string()
}

fn lane_asks(c: &drover::Ctx) {
    let asked = std::process::Command::new(env!("CARGO_BIN_EXE_drover"))
        .args([
            "ask",
            "sendmeter",
            "-",
            "Per key or per IP?",
            "--option",
            "key",
            "--option",
            "ip",
            "--recommend",
            "1",
        ])
        .env_remove("FLEET_LANE")
        .env("DROVER_LANE", "sendmeter:0:scope")
        .env("FLEET_CONFIG", &c.config)
        .env("FLEET_STATE", &c.state)
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&asked.stdout).into_owned();
    assert!(
        asked.status.success() && !said.contains("IN_SCOPE"),
        "a scope lane's question reaches the owner: {said}"
    );
    let q = kinds(c, "needs-decision").pop().unwrap();
    assert_eq!((q.str("agent"), q.issue), (Some("sendmeter-scope-0"), None), "{q:?}");
}

fn drops_file_nothing(c: &drover::Ctx, r: &Fake) {
    running(c, "new-2");
    let id = draft(c, r, &[]);
    let (code, out) = run(c, r, &["answer", "sendmeter", &id, "2"]);
    assert_eq!(code, 0, "{out}");
    let creates = r
        .calls
        .borrow()
        .iter()
        .filter(|c| c.starts_with("gh issue create"))
        .count();
    assert_eq!(creates, 1, "drop files nothing");
    assert_eq!(kinds(c, "queue").len(), 1, "drop queues nothing");
    assert_eq!(kinds(c, "scope-done").pop().unwrap().str("outcome"), Some("dropped"));
}

#[test]
fn approving_the_draft_files_exactly_its_body_and_queues_it_and_dropping_files_nothing() {
    let c = ctx(&tmpdir("scope-draft"), CONFIG);
    let r = Fake::default()
        .reply(
            "gh issue create",
            "https://github.com/jirathip-dev/sendmeter/issues/77\n",
        )
        .reply(
            "gh issue view 77",
            r#"{"title": "fix: keep the row title at 390 px", "body": "x"}"#,
        );
    let (code, out) = run(&c, &r, &["draft", "sendmeter", "--title", "t", "--body", "b"]);
    assert!(code == 3 && out.contains("NO_SCOPE"), "{code} {out}");
    running(&c, "new-1");
    lane_asks(&c);
    let id = draft(&c, &r, &["repro"]);
    let ask = kinds(&c, "needs-decision").pop().unwrap();
    assert_eq!(
        (ask.str("id"), ask.str("agent")),
        (Some(id.as_str()), Some("sendmeter-scope-0"))
    );
    assert!(ask.str("question").unwrap().contains(BODY), "{ask:?}");

    let (code, out) = run(&c, &r, &["answer", "sendmeter", &id, "say what the row shows"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        kinds(&c, "scope-done").is_empty() && !r.called("gh issue create"),
        "a revise note files nothing"
    );

    let id = draft(&c, &r, &["repro"]);
    let (code, out) = run(&c, &r, &["answer", "sendmeter", &id, "approve"]);
    assert_eq!(code, 0, "{out}");
    let want =
        format!("gh issue create -R jirathip-dev/sendmeter --title fix: keep the row title at 390 px --body {BODY}");
    assert!(r.called(&want), "{:?}", r.calls.borrow());
    let q = kinds(&c, "queue").pop().expect("queued");
    assert_eq!(
        (q.val("op"), q.val("issues"), q.val("repro")),
        (&json!("add"), &json!([77]), &json!(true))
    );
    let done = kinds(&c, "scope-done");
    assert_eq!(
        (done.len(), done[0].str("outcome"), done[0].val("filed")),
        (1, Some("filed"), &json!(77))
    );
    assert_eq!(kinds(&c, "lane-killed").len(), 1, "the scope lane closes");

    drops_file_nothing(&c, &r);
}

fn hook(lane: &str, tool: &str, input: &Value) -> String {
    let d = tmpdir("scope-hook");
    std::fs::write(d.join("fleet.toml"), CONFIG).unwrap();
    hook_in(&d, lane, tool, input)
}

#[test]
fn a_scope_lanes_hook_refuses_file_edits_spawn_and_merge_and_lets_it_ask_and_draft() {
    let (lane, edit) = (
        "sendmeter:0:scope",
        json!({"file_path": "/w/src/main.rs", "old_string": "a", "new_string": "b"}),
    );
    assert!(hook(lane, "Edit", &edit).contains("read-only"), "an edit");
    assert_eq!(hook("sendmeter:12:impl", "Edit", &edit), "");
    let bash = |cmd: &str| hook(lane, "Bash", &json!({"command": cmd}));
    for (cmd, why) in [
        ("drover spawn sendmeter 12 impl", "REFUSED SCOPE_ONLY"),
        ("drover merge sendmeter 12", "REFUSED SCOPE_ONLY"),
        ("gh pr merge 3", "merging is gated"),
        ("git commit -am x", "read-only"),
    ] {
        assert!(bash(cmd).contains(why), "{cmd}: {}", bash(cmd));
    }
    for cmd in [
        "drover ask sendmeter - \"per key or per IP?\" --option key --option ip --recommend 1",
        "drover draft sendmeter --title t --body b --mark repro",
    ] {
        assert_eq!(bash(cmd), "", "{cmd}");
    }
}
