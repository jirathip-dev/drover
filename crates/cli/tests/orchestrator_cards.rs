mod common;
use common::*;
use drover::events::{read, Event};
use drover::serve::Server;
use serde_json::json;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn req(s: &Server, path: &str, body: &str, cookie: &str, now: u64) -> String {
    let r = format!(
        "POST {path} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&r, now)).into_owned()
}

fn body(res: &str) -> &str {
    &res[res.find("\r\n\r\n").unwrap() + 4..]
}

fn chat(s: &Server, cookie: &str, now: u64) -> String {
    let j = api(s, ADDR, cookie, now, "chat");
    board_js(&[(&j, "chat.tab(d)")]).remove(0)
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

fn setup(name: &str, role_extra: &str, extra: &str) -> drover::Ctx {
    let d = tmpdir(name);
    let cfg = format!("{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\n{role_extra}\n{extra}");
    let c = ctx(&d, &cfg);
    let profile = c.config.with_file_name("claude").join("fleet-orchestrator");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Grill"]).1,
        "t1\n"
    );
    c
}

fn acp_setup(name: &str, role_extra: &str, extra: &str, b: &FakeShed) -> drover::Ctx {
    let d = tmpdir(name);
    let cfg = format!(
        "{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\n{role_extra}\n\n\
         [fleet.sendmeter.runtime]\nkind = \"shed-acp\"\nsocket = \"{}\"\n\n{extra}",
        b.sock.display()
    );
    let mut c = ctx(&d, &cfg);
    c.exe = env!("CARGO_BIN_EXE_drover").into();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Grill"]).1,
        "t1\n"
    );
    c
}

fn acp(adapter: &[&str]) -> Fake {
    let agents = json!([{"id": "claude-acp", "version": "0.101.0", "distribution": "npx", "cmd": adapter}]);
    Fake::default()
        .reply("shed agents --json", &agents.to_string())
        .reply("shed run", "")
        .reply("shed send", "")
}

fn herdr(agents: &str) -> Fake {
    Fake::default()
        .reply("herdr agent list", agents)
        .reply(
            "herdr workspace create",
            r#"{"result":{"root_pane":{"pane_id":"w9:p1"}}}"#,
        )
        .reply("herdr workspace close", "")
        .reply("herdr pane run", "")
        .reply("herdr pane send-keys", "")
        .reply("herdr agent start", "")
        .reply("herdr agent wait", "")
        .reply("herdr agent prompt", "")
}

fn live() -> String {
    agents(&[("orchestrator-t1", "idle", "/x", "w9:p1")])
}

#[test]
fn a_config_proposal_is_applied_only_by_the_owners_tap_with_a_backup_and_a_doctor_check() {
    let sh = FakeShed::new();
    let c = acp_setup("cards-config", "", "", &sh);
    let old = std::fs::read_to_string(&c.config).unwrap();
    let new = old.replace("model = \"opus\"", "model = \"sonnet\"");
    let file = c.config.with_file_name("proposed.toml");
    std::fs::write(&file, &new).unwrap();
    let bad = c.config.with_file_name("bad.toml");
    std::fs::write(&bad, "[limits]\neffort = \"huge\"\n").unwrap();
    assert_ne!(
        run(
            &c,
            &Fake::default(),
            &["config", "propose", "t1", bad.to_str().unwrap()]
        )
        .0,
        0,
        "validated"
    );
    let (code, out) = run(
        &c,
        &Fake::default(),
        &["config", "propose", "t1", file.to_str().unwrap()],
    );
    assert_eq!((code, out.as_str()), (0, "c1\n"));
    assert_eq!(
        kinds(&c, "config-propose")[0].str("diff"),
        Some("  [fleet.sendmeter.role.impl]\n- model = \"opus\"\n+ model = \"sonnet\"\n")
    );

    assert_eq!(
        run(
            &c,
            &sh.cli(&acp(&["echo"])),
            &["thread", "say", "t1", "use sonnet for impl"]
        )
        .0,
        0
    );
    let r = acp(&["echo"]);
    let cli = sh.cli(&r);
    let on = sh.on(&cli);
    let s = Server::new(&c, &on, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let page = chat(&s, &cookie, T);
    assert!(page.contains("<pre class=df>  [fleet.sendmeter.role.impl]\n- model = &quot;opus&quot;\n+ model = &quot;sonnet&quot;\n</pre>"), "{page}");
    assert!(
        page.contains("<form class=chc data-id=\"c1\"><button name=act value=apply>apply</button>"),
        "{page}"
    );

    let res = req(&s, "/thread", "op=config&id=c1&act=apply", "", T);
    assert!(res.starts_with("HTTP/1.1 401"), "{res}");
    assert_eq!(std::fs::read_to_string(&c.config).unwrap(), old);
    assert!(kinds(&c, "config-settled").is_empty());

    let res = req(&s, "/thread", "op=config&id=c1&act=apply", &cookie, T + 10);
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(
        body(&res).starts_with("config c1 applied by owner, backup kept, doctor "),
        "{res}"
    );
    assert_eq!(std::fs::read_to_string(&c.config).unwrap(), new);
    let backup = format!("{}.bak-{}", c.config.display(), T + 10);
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), old);
    assert!(r.called("gh auth status"), "doctor ran: {:?}", r.calls.borrow());
    let st = &kinds(&c, "config-settled")[0];
    assert_eq!(
        (st.data["applied"].clone(), st.str("backup")),
        (json!(true), Some(backup.as_str()))
    );
    assert!(st.data["doctor"].is_boolean());
    let page = chat(&s, &cookie, T + 20);
    assert!(
        !page.contains("class=chc")
            && page.contains(" class=\"qs note\">config c1 applied by owner, backup kept, doctor ")
    );
    assert!(
        r.called("shed send orchestrator-t1 [t1] system: config c1 applied by owner"),
        "the orchestrator learns it: {:?}",
        r.calls.borrow()
    );
    assert!(body(&req(&s, "/thread", "op=config&id=c1&act=discard", &cookie, T + 30)).contains("ALREADY_SETTLED"));

    std::fs::write(&file, new.replace("review_rounds = 3", "review_rounds = 4")).unwrap();
    assert_eq!(
        run(
            &c,
            &Fake::default(),
            &["config", "propose", "t1", file.to_str().unwrap()]
        )
        .1,
        "c2\n"
    );
    std::fs::write(&c.config, new.clone() + "\n# hand edit\n").unwrap();
    assert!(body(&req(&s, "/thread", "op=config&id=c2&act=apply", &cookie, T + 40)).contains("STALE"));
    assert_eq!(
        body(&req(&s, "/thread", "op=config&id=c2&act=discard", &cookie, T + 50)),
        "config c2 discarded by owner\n"
    );
    assert!(std::fs::read_to_string(&c.config).unwrap().ends_with("# hand edit\n"));
}

#[test]
fn a_signed_out_orchestrator_with_no_account_gets_a_sign_in_card_for_the_harness_login() {
    let signed_out = "Invalid API key · Please run /login";
    let page = |c: &drover::Ctx| {
        let r = herdr(NO_AGENTS);
        let s = Server::new(c, &r, Some(ADDR.into()));
        chat(&s, &pair(&s, c, "Mozilla/5.0 (iPhone)", T), T + 120)
    };
    let c = setup("cards-signin-plain", "", "");
    assert_eq!(run(&c, &herdr(NO_AGENTS), &["thread", "say", "t1", "scope #12"]).0, 0);
    let later = drover::Ctx {
        now: T + 60,
        ..c.clone()
    };
    let r = herdr(&live()).reply("herdr pane read w9:p1", signed_out);
    assert_eq!(run(&later, &r, &["check"]).0, 0);
    assert!(r.called("herdr workspace close w9"));
    assert_eq!(kinds(&c, "orchestrator-start").len(), 1, "nothing to resume on");
    assert!(kinds(&c, "agent-unavailable").is_empty(), "the harness is not rested");
    let lines: Vec<_> = kinds(&c, "thread-msg")
        .into_iter()
        .filter(|e| e.str("signin").is_some())
        .collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].str("signin"), Some("claude"));
    assert!(page(&c).contains("claude is signed out: sign in <button type=button data-su=\"claude\""));
    run(
        &later,
        &herdr(&live()).reply("herdr pane read w9:p1", signed_out),
        &["check"],
    );
    assert_eq!(
        kinds(&c, "thread-msg")
            .iter()
            .filter(|e| e.str("signin").is_some())
            .count(),
        1,
        "once"
    );
}

#[test]
fn a_secret_goes_only_to_the_keychain_from_an_owner_device_and_a_secret_reference_injects_at_spawn() {
    let value = "hunter2-SECRET-value";
    let sh = FakeShed::new();
    let c = acp_setup(
        "cards-secret",
        "",
        "[fleet.sendmeter.env]\nAPI_KEY = \"secret:API_KEY\"\n",
        &sh,
    );
    let bad = format!("{CONFIG}\n[fleet.sendmeter.env]\nX = \"secret:x'; rm -rf ~\"\n");
    assert!(
        drover::config::Config::parse(&bad).is_err(),
        "a secret: reference is a name"
    );
    let (code, out) = run(
        &c,
        &Fake::default(),
        &["secret", "request", "t1", "API_KEY", "--why", "the search API"],
    );
    assert_eq!(code, 0, "{out}");
    let f = acp(&["printenv", "API_KEY"]);
    let cli = sh.cli(&f);
    let r = sh.on(&cli);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let form = format!("op=secret&t=t1&name=API_KEY&act=save&value={value}");
    let guest = chat(&s, "", T);
    assert!(
        guest.contains("only a paired owner device can enter it") && !guest.contains("class=chs"),
        "{guest}"
    );
    assert!(req(&s, "/thread", &form, "", T).starts_with("HTTP/1.1 401"));
    let file = c.config.with_file_name("secrets/API_KEY");
    assert!(!file.exists());

    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    assert!(chat(&s, &cookie, T).contains("<form class=chs data-t=\"t1\" data-name=\"API_KEY\"><input type=password"));
    let res = req(&s, "/thread", &form, &cookie, T + 10);
    assert_eq!(body(&res), "secret API_KEY set ✓ (by owner)\n", "{res}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), value);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    let calls = f.calls.borrow().join("\n");
    let log = drover::events::jsonl(&c.state).unwrap();
    let (_, show) = run(&c, &Fake::default(), &["thread", "show", "t1", "--json"]);
    let page = chat(&s, &cookie, T + 20);
    for (what, text) in [("argv", &calls), ("events", &log), ("chat", &show), ("board", &page)] {
        assert!(!text.contains(value), "the secret is in {what}: {text}");
    }
    assert!(show.contains("secret API_KEY set ✓ (by owner)") && !page.contains("class=chs"));
    assert_eq!(*cli.out.borrow(), [format!("{value}\n")], "{calls}");
    assert!(calls.contains("&& . '") && !f.inputs.borrow().iter().any(|i| i.1.contains(value)));
    assert!(
        body(&req(&s, "/thread", &form, &cookie, T + 30)).contains("NO_SUCH_REQUEST"),
        "one value per request"
    );
    assert_eq!(run(&c, &Fake::default(), &["secret", "ls"]).1, "sendmeter API_KEY\n");
}

#[test]
fn a_run_card_shows_as_its_own_row_with_approve_and_deny_and_then_its_outcome() {
    let sh = FakeShed::new();
    let c = acp_setup("cards-run", "", "", &sh);
    let card = |cmd: &str| {
        let args = [
            "card",
            "post",
            "t1",
            "--cmd",
            cmd,
            "--why",
            "owner asked",
            "--blast",
            "read-only",
            "--dir",
            "/srv/app",
        ];
        run(&drover::Ctx { now: T, ..c.clone() }, &Fake::default(), &args).1
    };
    let cmd = |n: &str| {
        let d = json!({"thread": "t1", "cmd": format!("drover brief sendmeter {n}")});
        drover::events::append(&c.state, &Event::new(T, "sendmeter", None, "thread-cmd", d)).unwrap();
    };
    cmd("1");
    assert_eq!(card("git log -1 main"), "r1\n");
    cmd("2");
    assert_eq!(card("git status"), "r2\n");
    assert_eq!(card("uptime"), "r3\n");
    let sha = kinds(&c, "run-card")[0].str("sha").unwrap().to_string();
    let r = acp(&["echo"])
        .reply("sh -c nohup", "4242\n")
        .reply("sh -c", "commit 1a2b\n");
    let cli = sh.cli(&r);
    let on = sh.on(&cli);
    let s = Server::new(&c, &on, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);

    let j: serde_json::Value = serde_json::from_str(&api(&s, ADDR, &cookie, T + 60, "thread/t1")).unwrap();
    let msgs = j["msgs"].as_array().unwrap();
    let r1 = msgs.iter().find(|m| m["kind"] == "run" && m["run"]["id"] == "r1");
    let r1 = r1.unwrap_or_else(|| panic!("no run card r1 in {j}"));
    assert_eq!(
        (
            &r1["run"]["sha"],
            &r1["run"]["cmd"],
            &r1["run"]["dir"],
            r1["open"].clone()
        ),
        (&json!(sha), &json!("git log -1 main"), &json!("/srv/app"), json!(true))
    );

    let page = chat(&s, &cookie, T + 60);
    let row = format!("<form class=chr data-id=\"r1\" data-sha=\"{}\"><button name=act value=approve>approve</button><button name=act value=deny>deny</button></form>", &sha[..12]);
    for want in [
        row.as_str(),
        "<pre class=rc>git log -1 main</pre>",
        "why: owner asked",
        "touches: read-only",
        "runs on: main host, /srv/app",
        "expires in 14m",
    ] {
        assert!(page.contains(want), "{want} not in {page}");
    }
    let folds: Vec<&str> = page
        .split("<details")
        .filter(|d| d.contains("class=\"blk fold\""))
        .collect();
    assert!(folds.iter().all(|f| !f.contains("run card")), "{page}");

    let res = req(
        &s,
        "/thread",
        &format!("op=card&id=r1&act=approve&sha={}", &sha[..12]),
        &cookie,
        T + 120,
    );
    assert!(
        res.starts_with("HTTP/1.1 200") && body(&res).contains("run r1 approved by owner, running in /srv/app…"),
        "{res}"
    );
    let ran = run(
        &drover::Ctx {
            now: T + 125,
            ..c.clone()
        },
        &r,
        &["card", "run", "r1"],
    );
    assert_eq!(ran.0, 0, "{ran:?}");
    let res = req(&s, "/thread", "op=card&id=r2&act=deny", &cookie, T + 130);
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    let page = chat(&s, &cookie, T + 140);
    for want in [
        "✓ approved by owner · exit 0",
        "commit 1a2b",
        "✗ denied by owner · nothing ran",
    ] {
        assert!(page.contains(want), "{want} not in {page}");
    }
    assert!(
        !page.contains("data-id=\"r1\"") && !page.contains("data-id=\"r2\""),
        "{page}"
    );
    assert!(page.contains("data-id=\"r3\""), "{page}");
    let page = chat(&s, &cookie, T + 901);
    assert!(
        page.contains("expired · nothing ran") && !page.contains("data-id=\"r3\""),
        "{page}"
    );
}

fn card_between_replies(c: &drover::Ctx, sh: &FakeShed) -> (String, Fake, String) {
    let at = |n: u64| drover::Ctx {
        now: T + n,
        ..c.clone()
    };
    let say = |n: u64, text: &str| run(&at(n), &Fake::default(), &["thread", "reply", "t1", text]);
    say(0, "before the card");
    let args = [
        "card",
        "post",
        "t1",
        "--cmd",
        "uptime",
        "--why",
        "owner asked",
        "--blast",
        "read-only",
        "--dir",
        "/srv/app",
    ];
    assert_eq!(run(&at(10), &Fake::default(), &args).1, "r1\n");
    say(20, "after the card");
    let sha = kinds(c, "run-card")[0].str("sha").unwrap().to_string();
    let r = acp(&["echo"])
        .reply("sh -c nohup", "4242\n")
        .reply("sh -c", "load 0.42\n");
    let cookie = pair(
        &Server::new(c, &sh.on(&r), Some(ADDR.into())),
        c,
        "Mozilla/5.0 (iPhone)",
        T,
    );
    (sha, r, cookie)
}

fn t1(page: &str) -> &str {
    &page[page.find("<div class=cvp data-t=\"t1\">").unwrap()..]
}
fn pos(page: &str, w: &str) -> usize {
    page.find(w).unwrap_or_else(|| panic!("{w} not in {page}"))
}

#[test]
fn a_run_card_between_two_replies_renders_in_order_in_the_chat_tab() {
    let sh = FakeShed::new();
    let c = acp_setup("cards-run-order", "", "", &sh);
    let (sha, r, cookie) = card_between_replies(&c, &sh);
    let cli = sh.cli(&r);
    let on = sh.on(&cli);
    let s = Server::new(&c, &on, Some(ADDR.into()));
    let j: serde_json::Value = serde_json::from_str(&api(&s, ADDR, &cookie, T + 60, "chat")).unwrap();
    let t = j["threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "t1")
        .unwrap();
    let msgs: Vec<(&str, &str)> = t["page"]["msgs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| ["msg", "run"].contains(&m["kind"].as_str().unwrap()))
        .map(|m| (m["kind"].as_str().unwrap(), m["text"].as_str().unwrap()))
        .collect();
    assert_eq!(
        msgs,
        [("msg", "before the card"), ("run", "uptime"), ("msg", "after the card")],
        "{t}"
    );

    let all = chat(&s, &cookie, T + 60);
    let page = t1(&all);
    let form = format!("<form class=chr data-id=\"r1\" data-sha=\"{}\"><button name=act value=approve>approve</button><button name=act value=deny>deny</button></form>", &sha[..12]);
    let order = [
        "before the card",
        "<pre class=rc>uptime</pre>",
        "why: owner asked",
        "touches: read-only",
        &form,
        "after the card",
    ];
    let at: Vec<usize> = order.iter().map(|w| pos(page, w)).collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "{order:?} out of order in {page}");
}

#[test]
fn a_settled_run_cards_result_renders_in_the_chat_tab() {
    let sh = FakeShed::new();
    let c = acp_setup("cards-run-result", "", "", &sh);
    let (sha, r, cookie) = card_between_replies(&c, &sh);
    let cli = sh.cli(&r);
    let out = run(
        &drover::Ctx {
            now: T + 120,
            ..c.clone()
        },
        &cli,
        &["card", "approve", "r1", "--sha", &sha[..12]],
    );
    assert!(
        out.1.contains("run r1 approved by owner, running in /srv/app…"),
        "{out:?}"
    );
    let ran = run(
        &drover::Ctx {
            now: T + 125,
            ..c.clone()
        },
        &cli,
        &["card", "run", "r1"],
    );
    assert_eq!(ran.0, 0, "{ran:?}");
    let on = sh.on(&cli);
    let s = Server::new(&c, &on, Some(ADDR.into()));
    let all = chat(&s, &cookie, T + 130);
    let page = t1(&all);
    let order = [
        "<pre class=rc>uptime</pre>",
        "✓ approved by owner · exit 0",
        "load 0.42",
        "after the card",
        "run r1 approved by owner, ran in /srv/app: exit 0",
    ];
    let at: Vec<usize> = order.iter().map(|w| pos(page, w)).collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "{order:?} out of order in {page}");
    assert!(!page.contains("data-id=\"r1\""), "{page}");
}
