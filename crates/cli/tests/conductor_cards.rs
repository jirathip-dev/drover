//! #387: conductor chat step 3: `config propose` cards only the owner applies, the conductor's account switch with
//! its session, secrets that never leave the keychain, and the Telegram bridge against a fake Bot API.
mod common;
use common::*;
use herdsman::events::{read, Event};
use herdsman::runner::{Out, Runner};
use herdsman::serve::Server;
use serde_json::{json, Value};

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
    let r = format!("GET /frags?tab=chat HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}\r\n");
    let res = String::from_utf8_lossy(&s.respond(&r, now)).into_owned();
    let j: Value = serde_json::from_str(body(&res)).unwrap();
    j["chat"].as_str().unwrap().into()
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
            && page.contains("<p class=\"qs note\">config c1 applied by owner, backup kept, doctor ")
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
    let d = tmpdir("cards-switch-env");
    let env = d.join("tokens.env");
    std::fs::write(&env, "CLAUDE_CODE_OAUTH_TOKEN_A=a\nCLAUDE_CODE_OAUTH_TOKEN_B=b\n").unwrap();
    let accounts = format!(
        "[accounts]\naccounts_env = \"{}\"\nA = \"CLAUDE_CODE_OAUTH_TOKEN_A\"\nB = \"CLAUDE_CODE_OAUTH_TOKEN_B\"\n",
        env.display()
    );
    let c = setup("cards-switch", "fallback = [\"claude:A\", \"claude:B\"]", &accounts);
    assert_eq!(run(&c, &herdr(NO_AGENTS), &["thread", "say", "t1", "scope #12"]).0, 0);
    run(&c, &Fake::default(), &["thread", "reply", "t1", "Small or full?"]);
    let first = kinds(&c, "conductor-start")[0].clone();
    assert_eq!(first.str("account"), Some("A"));
    let session = first.str("session").unwrap().to_string();

    let banner = "You've hit your limit · resets 6:40pm";
    let r = herdr(&live()).reply("herdr pane read w9:p1", banner);
    let (code, out) = run(
        &herdsman::Ctx {
            now: T + 60,
            ..c.clone()
        },
        &r,
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
    assert!(
        calls.iter().any(|c| c.contains("CLAUDE_CODE_OAUTH_TOKEN_B")),
        "on B: {calls:?}"
    );
    let (_, show) = run(&c, &Fake::default(), &["thread", "show", "t1"]);
    assert!(
        show.contains(
            "owner: scope #12\nconductor: Small or full?\nsystem: switched to claude:B (claude:A resting until "
        ),
        "{show}"
    );
}

/// `Fake` that also records each `run_in` stdin.
struct Stdin(Fake, Shared<Vec<String>>);

impl Runner for Stdin {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.0.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.1.borrow_mut().push(input.into());
        self.0.run(prog, args)
    }
}

#[test]
fn a_secret_goes_only_to_the_keychain_from_an_owner_device_and_a_secret_reference_injects_at_spawn() {
    let value = "hunter2-SECRET-value";
    let hex: String = value.bytes().map(|b| format!("{b:02x}")).collect();
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
    let r = Stdin(
        herdr(NO_AGENTS).reply("security -i", "").reply(
            "security find-generic-password -a herdsman -s herdsman/sendmeter/API_KEY",
            "",
        ),
        Shared::default(),
    );
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let form = format!("op=secret&t=t1&name=API_KEY&act=save&value={value}");
    // Not a paired device: no card input, and the POST is refused before the keychain.
    let guest = chat(&s, "", T);
    assert!(
        guest.contains("only a paired owner device can enter it") && !guest.contains("class=chs"),
        "{guest}"
    );
    assert!(req(&s, "/thread", &form, "", T).starts_with("HTTP/1.1 401"));
    assert!(!r.0.called("security"));

    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    assert!(chat(&s, &cookie, T).contains("<form class=chs data-t=\"t1\" data-name=\"API_KEY\"><input type=password"));
    let res = req(&s, "/thread", &form, &cookie, T + 10);
    assert_eq!(body(&res), "secret API_KEY set ✓ (by owner)\n", "{res}");
    // The value went to `security -i` on stdin, hex-encoded, and nowhere else.
    assert_eq!(
        *r.1.borrow(),
        vec![format!(
            "add-generic-password -U -a herdsman -s herdsman/sendmeter/API_KEY -X {hex}\n"
        )]
    );
    let calls = r.0.calls.borrow().join("\n");
    let log = std::fs::read_to_string(&c.state).unwrap();
    let (_, show) = run(&c, &Fake::default(), &["thread", "show", "t1", "--json"]);
    let page = chat(&s, &cookie, T + 20);
    for (what, text) in [("argv", &calls), ("events", &log), ("chat", &show), ("board", &page)] {
        assert!(
            !text.contains(value) && !text.contains(&hex),
            "the secret is in {what}: {text}"
        );
    }
    assert!(show.contains("secret API_KEY set ✓ (by owner)") && !page.contains("class=chs"));
    // `secret:API_KEY` reaches the lane's own shell as a keychain read: the conductor's start exported it.
    let want = "API_KEY=\"$(security find-generic-password -a herdsman -s 'herdsman/sendmeter/API_KEY' -w 2>/dev/null || security find-generic-password -a herdsman -s 'herdsman/all/API_KEY' -w)\"";
    assert!(calls.contains(want), "{calls}");
    assert!(
        body(&req(&s, "/thread", &form, &cookie, T + 30)).contains("NO_SUCH_REQUEST"),
        "one value per request"
    );
    // `secret ls` lists names only.
    assert_eq!(run(&c, &Fake::default(), &["secret", "ls"]).1, "sendmeter API_KEY\n");
}

/// A fake Bot API behind curl: each method's reply, `getUpdates` from `updates` (then none); a download writes the
/// file curl's `output` names. Records what curl got on stdin.
struct Bot {
    herdr: Fake,
    updates: Shared<Vec<Value>>,
    sent: Shared<Vec<String>>,
}

impl Runner for Bot {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        if prog == "security" {
            return Ok(Out {
                code: 0,
                stdout: "TOKEN123:abc\n".into(),
                stderr: String::new(),
            });
        }
        self.herdr.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.herdr.run(prog, args)?; // the argv, for the token check
        self.sent.borrow_mut().push(input.into());
        let url = input.lines().next().unwrap();
        let method = url.trim_end_matches('"').rsplit('/').next().unwrap();
        let result = match method {
            "getUpdates" => Value::Array(std::mem::take(&mut *self.updates.borrow_mut())),
            "getFile" => json!({"file_path": "photos/file_1.jpg"}),
            "createForumTopic" => json!({"message_thread_id": 77}),
            "file_1.jpg" => {
                let out = input.lines().find_map(|l| l.strip_prefix("output = \"")).unwrap();
                std::fs::write(out.trim_end_matches('"'), b"JPEG").unwrap();
                return Ok(Out::default());
            }
            _ => json!({"message_id": 1}),
        };
        Ok(Out {
            code: 0,
            stdout: json!({"ok": true, "result": result}).to_string(),
            stderr: String::new(),
        })
    }
}

fn msg(topic: i64, from: i64, extra: Value) -> Value {
    let mut m = json!({"chat": {"id": -100}, "from": {"id": from}, "message_thread_id": topic});
    m.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    m
}

#[test]
fn telegram_maps_topics_to_threads_choices_to_buttons_and_images_both_ways() {
    let c = setup(
        "cards-telegram",
        "",
        "[telegram]\nchat = -100\nowner = 42\napi = \"https://bot.test\"\n",
    );
    let cfg = herdsman::config::Config::load(&c.config).unwrap();
    let r = Bot {
        herdr: herdr(&live()).reply("curl", ""),
        updates: Shared::default(),
        sent: Shared::default(),
    };
    let at = |n: u64| herdsman::Ctx {
        now: T + n,
        ..c.clone()
    };
    // First pass: nothing older than the bridge is relayed.
    herdsman::telegram::tick(&at(0), &cfg, &r, 0).unwrap();
    let created = |name: &str| json!({"forum_topic_created": {"name": name}});
    *r.updates.borrow_mut() = vec![
        json!({"update_id": 1, "message": msg(5, 42, created("sendmeter: dark mode"))}),
        json!({"update_id": 2, "message": msg(5, 42, json!({"caption": "look", "photo": [{"file_id": "s"}, {"file_id": "big"}]}))}),
        json!({"update_id": 3, "message": msg(5, 99, json!({"text": "not the owner"}))}),
        json!({"update_id": 4, "message": msg(6, 42, created("ideas"))}),
    ];
    herdsman::telegram::tick(&at(10), &cfg, &r, 0).unwrap();
    let news: Vec<(String, String)> = kinds(&c, "thread-new")
        .iter()
        .map(|e| (e.fleet.clone(), e.str("title").unwrap().into()))
        .collect();
    assert_eq!(
        news[1..],
        [
            ("sendmeter".into(), "sendmeter: dark mode".into()),
            ("all".into(), "ideas".into())
        ]
    );
    let saved = c
        .state
        .with_file_name("conductor")
        .join(format!("t2/uploads/{}-file_1.jpg", T + 10));
    assert_eq!(
        std::fs::read(&saved).unwrap(),
        b"JPEG",
        "the photo is in the thread's folder"
    );
    assert!(
        r.herdr.called("herdr agent start conductor-t2"),
        "{:?}",
        r.herdr.calls.borrow()
    );
    let start = kinds(&c, "thread-msg");
    assert_eq!(start.len(), 1, "the non-owner's message is dropped");
    assert_eq!(
        (start[0].str("text"), start[0].data["files"][0]["path"].as_str()),
        (Some("look"), saved.canonicalize().unwrap().to_str())
    );

    // Out: the conductor's reply and image, and an ask as inline buttons; then the tap answers it.
    let img = c.state.with_file_name("conductor").join("t2/mock.png");
    std::fs::write(&img, b"PNG").unwrap();
    run(
        &at(20),
        &Fake::default(),
        &["thread", "reply", "t2", "Here.", "--file", img.to_str().unwrap()],
    );
    let ask = [
        "ask",
        "sendmeter",
        "-",
        "Which?",
        "--option",
        "a",
        "--option",
        "b",
        "--recommend",
        "2",
        "--thread",
        "t2",
    ];
    assert_eq!(run(&at(21), &Fake::default(), &ask).1, "q1\n");
    run(
        &at(22),
        &Fake::default(),
        &["thread", "new", "sendmeter", "From the board"],
    );
    r.sent.borrow_mut().clear();
    herdsman::telegram::tick(&at(30), &cfg, &r, 0).unwrap();
    let sent = r.sent.borrow().clone();
    let has = |method: &str, part: &str| {
        sent.iter()
            .any(|s| s.contains(&format!("/{method}\"")) && s.contains(part))
    };
    assert!(
        has("sendMessage", r#"\"message_thread_id\":5,\"text\":\"Here.\""#),
        "{sent:?}"
    );
    assert!(
        has(
            "sendPhoto",
            &format!("form = \"photo=@{}\"", img.canonicalize().unwrap().display())
        ),
        "{sent:?}"
    );
    assert!(
        has(
            "sendMessage",
            r#"\"callback_data\":\"sendmeter:q1:2\",\"text\":\"2. b (recommended)\""#
        ),
        "{sent:?}"
    );
    assert!(has("createForumTopic", r#"\"name\":\"From the board\""#), "{sent:?}");
    assert_eq!(kinds(&c, "telegram-topic").last().unwrap().data["topic"], json!(77));

    *r.updates.borrow_mut() =
        vec![json!({"update_id": 5, "callback_query": {"id": "cb", "from": {"id": 42}, "data": "sendmeter:q1:2"}})];
    herdsman::telegram::tick(&at(40), &cfg, &r, 0).unwrap();
    let d = &kinds(&c, "decision")[0];
    assert_eq!((d.str("choice"), d.str("via")), (Some("b"), Some("telegram")));
    assert!(r
        .sent
        .borrow()
        .iter()
        .any(|s| s.contains("/answerCallbackQuery\"") && s.contains("answered q1")));
    // The token stays on curl's stdin, never argv; the cursor is saved.
    assert!(!r.herdr.calls.borrow().iter().any(|c| c.contains("TOKEN123")));
    let cur: Value =
        serde_json::from_str(&std::fs::read_to_string(c.state.with_file_name("telegram.json")).unwrap()).unwrap();
    assert_eq!(cur["offset"], json!(6));
}
