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

fn card_text(c: &drover::Ctx, s: &Server, now: u64, key: &str) -> String {
    let all = rows(c, s, now);
    let last = all.iter().rfind(|r| r["key"] == key);
    last.unwrap_or_else(|| panic!("no row {key} in {all:?}"))["block"].to_string()
}

fn said(c: &drover::Ctx, t: &str) -> Vec<String> {
    let all = kinds(c, "thread-msg").into_iter();
    all.filter(|e| e.str("thread") == Some(t) && e.str("from") == Some("system"))
        .filter_map(|e| e.str("text").map(String::from))
        .collect()
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
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
    attach_chat(&c, "sendmeter", "t1");
    c
}

fn acp(adapter: &[&str]) -> Fake {
    let agents = json!([{"id": "claude-acp", "version": "0.101.0", "distribution": "npx", "cmd": adapter}]);
    Fake::default()
        .reply("shed help run", "--meta <JSON>")
        .reply("shed agents --json", &agents.to_string())
        .reply("shed run", "")
        .reply("shed send", "")
}

#[test]
fn parity_32_a_config_proposal_is_applied_only_by_the_owners_tap_with_a_backup_and_a_doctor_check() {
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

    let r = acp(&["echo"]);
    let cli = sh.cli(&r);
    let on = sh.on(&cli);
    let s = Server::new(&c, &on, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let open = card_text(&c, &s, T, "config-c1");
    assert!(
        open.contains("- model = \\\"opus\\\"") && open.contains("\"apply\""),
        "{open}"
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
        (st.val("applied").clone(), st.str("backup")),
        (json!(true), Some(backup.as_str()))
    );
    assert!(st.val("doctor").is_boolean());
    let applied = card_text(&c, &s, T + 20, "config-c1");
    assert!(
        applied.contains("applied by owner") && applied.contains(&backup) && !applied.contains("\"apply\""),
        "applied: {applied}"
    );
    assert!(
        said(&c, "t1")
            .iter()
            .any(|t| t.starts_with("config c1 applied by owner, backup kept, doctor ")),
        "the chat gets the settle line as a row: {:?}",
        said(&c, "t1")
    );
    assert!(
        kinds(&c, "orchestrator-start").is_empty(),
        "drover starts no chat session"
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
fn parity_33_a_secret_goes_only_to_the_keychain_from_an_owner_device() {
    use std::os::unix::fs::PermissionsExt;
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
    assert!(req(&s, "/thread", &form, "", T).starts_with("HTTP/1.1 401"));
    let file = c.config.with_file_name("secrets/API_KEY");
    assert!(!file.exists());

    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let card = card_text(&c, &s, T, "secret-t1-API_KEY");
    assert!(
        card.contains("the search API") && card.contains("secret-card"),
        "{card}"
    );
    let res = req(&s, "/thread", &form, &cookie, T + 10);
    assert_eq!(body(&res), "secret API_KEY set ✓ (by owner)\n", "{res}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), value);
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    let calls = f.calls.borrow().join("\n");
    let log = drover::events::jsonl(&c.state).unwrap();
    let shown = format!("{:?}", rows(&c, &s, T + 20));
    let cards = String::from_utf8_lossy(&s.respond(
        &format!("GET /screens/cards HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}\r\n"),
        T + 20,
    ))
    .into_owned();
    for (what, text) in [("argv", &calls), ("events", &log), ("rows", &shown), ("board", &cards)] {
        assert!(!text.contains(value), "the secret is in {what}: {text}");
    }
    assert!(said(&c, "t1").contains(&"secret API_KEY set ✓ (by owner)".to_string()));
    assert!(!f.inputs.borrow().iter().any(|i| i.1.contains(value)));
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

    let r1 = card_text(&c, &s, T + 60, "run-r1");
    for want in [
        "git log -1 main",
        "/srv/app",
        "read-only",
        "owner asked",
        &sha[..12],
        "on main · expires in 14m",
    ] {
        assert!(r1.contains(want), "{want}: {r1}");
    }

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
    let r1 = card_text(&c, &s, T + 140, "run-r1");
    assert!(
        r1.contains("**approved** by owner · exit 0") && r1.contains("commit 1a2b"),
        "{r1}"
    );
    let r2 = card_text(&c, &s, T + 140, "run-r2");
    assert!(r2.contains("**denied** by owner"), "{r2}");
    assert!(card_text(&c, &s, T + 140, "run-r3").contains("expires in"));
    assert!(card_text(&c, &s, T + 901, "run-r3").contains("**expired**"));
}
