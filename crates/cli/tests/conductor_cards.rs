//! #387: conductor chat step 3: `config propose` cards only the owner applies, the conductor's account switch with
//! its session, and secrets that never leave the keychain.
mod common;
use common::*;
use herdsman::events::{read, Event};
use herdsman::serve::Server;
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

/// The [chat] tab as the board draws it (#269) from API v1's `chat`.
fn chat(s: &Server, cookie: &str, now: u64) -> String {
    let j = api(s, ADDR, cookie, now, "chat");
    board_js(&[(&j, "chat.tab(d)")]).remove(0)
}

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

/// A sendmeter conductor (`role_extra` added to its role, `extra` to fleet.toml) and thread t1.
fn setup(name: &str, role_extra: &str, extra: &str) -> herdsman::Ctx {
    let d = tmpdir(name);
    let cfg = format!("{CONFIG}\n[fleet.sendmeter.role.conductor]\nharness = \"claude\"\n{role_extra}\n{extra}");
    let c = ctx(&d, &cfg);
    let profile = c.config.with_file_name("claude").join("fleet-conductor");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Grill"]).1,
        "t1\n"
    );
    c
}

/// herdr with `agents` live, ready to start a conductor or type into one.
fn herdr(agents: &str) -> Fake {
    Fake::default()
        .stored("herdsman/all/CLAUDE_CODE_OAUTH_TOKEN_A", "a")
        .stored("herdsman/all/CLAUDE_CODE_OAUTH_TOKEN_B", "b")
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
    agents(&[("conductor-t1", "idle", "/x", "w9:p1")])
}

#[test]
fn a_config_proposal_is_applied_only_by_the_owners_tap_with_a_backup_and_a_doctor_check() {
    let c = setup("cards-config", "", "");
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
        run(&c, &herdr(NO_AGENTS), &["thread", "say", "t1", "use sonnet for impl"]).0,
        0
    );
    let r = herdr(&live());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let page = chat(&s, &cookie, T);
    assert!(page.contains("<pre class=df>  [fleet.sendmeter.role.impl]\n- model = &quot;opus&quot;\n+ model = &quot;sonnet&quot;\n</pre>"), "{page}");
    assert!(
        page.contains("<form class=chc data-id=\"c1\"><button name=act value=apply>apply</button>"),
        "{page}"
    );

    // Not a paired device: refused, nothing written.
    let res = req(&s, "/thread", "op=config&id=c1&act=apply", "", T);
    assert!(res.starts_with("HTTP/1.1 401"), "{res}");
    assert_eq!(std::fs::read_to_string(&c.config).unwrap(), old);
    assert!(kinds(&c, "config-settled").is_empty());

    // The owner's tap: backup, the new file, doctor, an event, the folded line.
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
        r.called("herdr agent prompt conductor-t1 [t1] system: config c1 applied by owner"),
        "the conductor learns it"
    );
    assert!(body(&req(&s, "/thread", "op=config&id=c1&act=discard", &cookie, T + 30)).contains("ALREADY_SETTLED"));

    // A proposal over a fleet.toml that moved since is refused; a discard writes nothing.
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
fn a_usage_limit_resumes_the_conductor_on_the_next_account_with_the_same_session() {
    let accounts = "[accounts]\nA = \"CLAUDE_CODE_OAUTH_TOKEN_A\"\nB = \"CLAUDE_CODE_OAUTH_TOKEN_B\"\n";
    let c = setup("cards-switch", "fallback = [\"claude:A\", \"claude:B\"]", accounts);
    assert_eq!(run(&c, &herdr(NO_AGENTS), &["thread", "say", "t1", "scope #12"]).0, 0);
    run(&c, &Fake::default(), &["thread", "reply", "t1", "Small or full?"]);
    let first = kinds(&c, "conductor-start")[0].clone();
    assert_eq!(first.str("account"), Some("A"));
    let session = first.str("session").unwrap().to_string();

    let banner = "You've hit your limit · resets 6:40pm";
    let r = herdr(&live()).reply("herdr pane read w9:p1", banner);
    let sh = Shell(&r, "CLAUDE_CODE_OAUTH_TOKEN", Shared::default());
    let (code, out) = run(
        &herdsman::Ctx {
            now: T + 60,
            ..c.clone()
        },
        &sh,
        &["check"],
    );
    assert_eq!(code, 0, "{out}");
    assert_eq!(kinds(&c, "account-capped")[0].str("account"), Some("A"));
    assert!(r.called("herdr workspace close w9"));
    let next = &kinds(&c, "conductor-start")[1];
    assert_eq!(
        (next.str("account"), next.str("session")),
        (Some("B"), Some(session.as_str()))
    );
    assert_eq!(next.data["resumed"], json!(true));
    let calls = r.calls.borrow();
    let start = calls
        .iter()
        .find(|c| c.starts_with("herdr agent start conductor-t1"))
        .unwrap();
    assert!(
        start.contains(&format!("--resume {session}")) && !start.contains("--session-id"),
        "{start}"
    );
    assert_eq!(*sh.2.borrow(), ["b"], "on B: {calls:?}");
    let (_, show) = run(&c, &Fake::default(), &["thread", "show", "t1"]);
    assert!(
        show.contains(
            "owner: scope #12\nconductor: Small or full?\nsystem: switched to claude:B (claude:A resting until "
        ),
        "{show}"
    );
}

#[test]
fn a_signed_out_conductor_gets_a_sign_in_card_for_its_own_login() {
    let signed_out = "Invalid API key · Please run /login";
    let page = |c: &herdsman::Ctx| {
        let r = herdr(NO_AGENTS);
        let s = Server::new(c, &r, Some(ADDR.into()));
        chat(&s, &pair(&s, c, "Mozilla/5.0 (iPhone)", T), T + 120)
    };
    // The default conductor (no account, no fallback): nothing to switch to, still a [sign in] for `claude`.
    let c = setup("cards-signin-plain", "", "");
    assert_eq!(run(&c, &herdr(NO_AGENTS), &["thread", "say", "t1", "scope #12"]).0, 0);
    let later = herdsman::Ctx {
        now: T + 60,
        ..c.clone()
    };
    let r = herdr(&live()).reply("herdr pane read w9:p1", signed_out);
    assert_eq!(run(&later, &r, &["check"]).0, 0);
    assert!(r.called("herdr workspace close w9"));
    assert_eq!(kinds(&c, "conductor-start").len(), 1, "no account to resume on");
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

    // On account A with a `claude A` login configured: the card opens that login, and B carries on.
    let extra = "[accounts]\nA = \"CLAUDE_CODE_OAUTH_TOKEN_A\"\nB = \"CLAUDE_CODE_OAUTH_TOKEN_B\"\n\n[[setup]]\nname = \"claude A\"\ncmd = \"claude /login\"\nenv = {CLAUDE_CONFIG_DIR = \"~/accounts/A\"}\n";
    let c = setup("cards-signin-account", "fallback = [\"claude:A\", \"claude:B\"]", extra);
    assert_eq!(run(&c, &herdr(NO_AGENTS), &["thread", "say", "t1", "scope #12"]).0, 0);
    let r = herdr(&live()).reply("herdr pane read w9:p1", signed_out);
    let later = herdsman::Ctx {
        now: T + 60,
        ..c.clone()
    };
    assert_eq!(run(&later, &r, &["check"]).0, 0);
    let line = kinds(&c, "thread-msg")
        .into_iter()
        .find(|e| e.str("signin").is_some())
        .unwrap();
    assert_eq!(line.str("signin"), Some("claude A"));
    assert!(page(&c).contains("claude is signed out (account A): sign in <button type=button data-su=\"claude A\""));
    assert_eq!(kinds(&c, "conductor-start")[1].str("account"), Some("B"));
}

#[test]
fn a_secret_goes_only_to_the_keychain_from_an_owner_device_and_a_secret_reference_injects_at_spawn() {
    let value = "hunter2-SECRET-value";
    let c = setup(
        "cards-secret",
        "",
        "[fleet.sendmeter.env]\nAPI_KEY = \"secret:API_KEY\"\n",
    );
    let bad = format!("{CONFIG}\n[fleet.sendmeter.env]\nX = \"secret:x'; rm -rf ~\"\n");
    assert!(
        herdsman::config::Config::parse(&bad).is_err(),
        "a secret: reference is a name"
    );
    let (code, out) = run(
        &c,
        &Fake::default(),
        &["secret", "request", "t1", "API_KEY", "--why", "the search API"],
    );
    assert_eq!(code, 0, "{out}");
    let f = herdr(NO_AGENTS);
    let r = Shell(&f, "API_KEY", Shared::default());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let form = format!("op=secret&t=t1&name=API_KEY&act=save&value={value}");
    // Not a paired device: no card input, and the POST is refused before the keychain.
    let guest = chat(&s, "", T);
    assert!(
        guest.contains("only a paired owner device can enter it") && !guest.contains("class=chs"),
        "{guest}"
    );
    assert!(req(&s, "/thread", &form, "", T).starts_with("HTTP/1.1 401"));
    assert!(!f.store.borrow().contains_key("herdsman/sendmeter/API_KEY"));

    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    assert!(chat(&s, &cookie, T).contains("<form class=chs data-t=\"t1\" data-name=\"API_KEY\"><input type=password"));
    let res = req(&s, "/thread", &form, &cookie, T + 10);
    assert_eq!(body(&res), "secret API_KEY set ✓ (by owner)\n", "{res}");
    // #441: the value went to the OS store and nowhere else.
    assert_eq!(f.store.borrow()["herdsman/sendmeter/API_KEY"], value);
    let calls = f.calls.borrow().join("\n");
    let log = herdsman::events::jsonl(&c.state).unwrap();
    let (_, show) = run(&c, &Fake::default(), &["thread", "show", "t1", "--json"]);
    let page = chat(&s, &cookie, T + 20);
    for (what, text) in [("argv", &calls), ("events", &log), ("chat", &show), ("board", &page)] {
        assert!(!text.contains(value), "the secret is in {what}: {text}");
    }
    assert!(show.contains("secret API_KEY set ✓ (by owner)") && !page.contains("class=chs"));
    // `secret:API_KEY` reaches the lane's own shell from the file spawn delivered: the conductor's start sourced it.
    assert_eq!(*r.2.borrow(), [value], "{calls}");
    assert!(calls.contains("&& . '") && !f.inputs.borrow().iter().any(|i| i.1.contains(value)));
    assert!(
        body(&req(&s, "/thread", &form, &cookie, T + 30)).contains("NO_SUCH_REQUEST"),
        "one value per request"
    );
    // `secret ls` lists names only.
    assert_eq!(run(&c, &Fake::default(), &["secret", "ls"]).1, "sendmeter API_KEY\n");
}
