//! #157: multi-host lanes with a fake herdr of two machines (`mini`, this host; `bazzite`, over `--machine`/`ssh`)
//! and fake probes: tags, needs and NO_HOST, least-busy placement, per-host admission, `--machine` on every lane call,
//! a remote clean, an offline host that never pages, and the on/off switch (amendment A).
mod common;
use common::*;
use herdsman::config::Config;
use herdsman::events::{append, read, Event};
use herdsman::hosts::{cached, place};
use herdsman::serve::Server;
use serde_json::json;

const NOW: u64 = 1_000_000;
const SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 bazzite -- ";
const WT: &str = "/home/me/.herdr/worktrees/sendmeter/issue-12";
const ADDR: &str = "127.0.0.1:7777";

/// CONFIG on Claude with two hosts; `extra` lands in `[hosts.bazzite]`.
fn config(extra: &str) -> String {
    let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    format!("{base}\n[hosts.mini]\ntags = [\"fast\"]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n{extra}")
}

/// A probed cache: mini (macOS, xcode) and bazzite (Linux, docker), bazzite last seen `seen` s ago.
fn setup(name: &str, extra: &str, bazzite_up: bool, seen: u64) -> herdsman::Ctx {
    let c = ctx(&tmpdir(name), &config(extra));
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos", "arch:arm64", "xcode"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "bazzite": {"tags": ["os:linux", "arch:x86_64", "docker"], "home": "/home/me", "probed": NOW,
                    "seen": NOW - seen, "up": bazzite_up, "load": 0.1, "mem": 12.0, "disk": 200.0},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    c
}

/// Both hosts reachable; mini at `mini` load/cpu, bazzite at `remote` (load ÷ 4 cpus, Linux `/proc`).
fn fake(mini: f64, remote: f64) -> Fake {
    Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("herdr --machine bazzite agent list", NO_AGENTS)
        .reply("sysctl -n vm.loadavg", &format!("{{ {} 1 1 }}", mini * 4.0))
        .reply("sysctl -n hw.ncpu", "4")
        .reply(
            &format!("{SSH}'cat' '/proc/loadavg'"),
            &format!("{} 1 1 1/9 42", remote * 4.0),
        )
        .reply(&format!("{SSH}'nproc'"), "4")
}

fn ev(c: &herdsman::Ctx, ts: u64, issue: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(issue), kind, d)).unwrap();
}

fn remote_spawn(c: &herdsman::Ctx, ts: u64) {
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "pane": "w9:p1", "workspace": "w9",
                   "worktree": WT, "branch": "issue-12", "host": "bazzite", "machine": "bazzite"});
    ev(c, ts, 12, "spawn", d);
}

/// Where a lane of `role` on #`issue` goes: the host's name, or the refusal.
fn placed(c: &herdsman::Ctx, r: &Fake, issue: u64, role: &str, force: Option<&str>) -> Result<String, String> {
    let cfg = Config::load(&c.config).unwrap();
    let events = read(&c.state).unwrap();
    match place(c, &cfg, r, &events, ("sendmeter", issue, role), force) {
        Ok(p) => {
            let p = p.unwrap();
            p.refusal
                .map_or(Ok(p.name), |(code, why)| Err(format!("{code}: {why}")))
        }
        Err(e) => Err(format!("{e:#}")),
    }
}

#[test]
fn tags_are_detected_cached_and_refreshed_hourly() {
    let c = ctx(&tmpdir("hosts-tags"), &config(""));
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let r = fake(1.0, 1.0)
        .reply("sh -c uname -s", "Darwin\narm64\n/Users/me\nxcode\n")
        .reply(
            &format!("{SSH}'sh' '-c' 'uname -s"),
            "Linux\naarch64\n/home/me\ndocker\n",
        );
    let (code, out) = run(&c, &r, &["hosts", "refresh"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("mini · up · tags os:macos,arch:arm64,xcode,fast · load/cpu 1.00"),
        "{out}"
    );
    assert!(
        out.contains("bazzite · up · tags os:linux,arch:arm64,docker"),
        "hand tags only where set: {out}"
    );
    assert_eq!(cached(&c)["bazzite"].home, "/home/me");
    let probes = |r: &Fake| r.count("sh -c uname") + r.count(&format!("{SSH}'sh' '-c' 'uname"));
    assert_eq!(probes(&r), 2);
    let later = |s| herdsman::Ctx {
        now: NOW + s,
        ..c.clone()
    };
    assert_eq!(run(&later(60), &r, &["check"]).0, 0);
    assert_eq!(probes(&r), 2, "cached within the hour");
    assert_eq!(run(&later(3600), &r, &["check"]).0, 0);
    assert_eq!(probes(&r), 4, "probed again after an hour");
}

#[test]
fn needs_pick_the_host_whose_tags_cover_them_else_no_host() {
    let c = setup("hosts-needs", "", true, 0);
    let r = fake(0.1, 2.0);
    assert_eq!(
        placed(&c, &r, 12, "impl", None).as_deref(),
        Ok("mini"),
        "least busy, no needs"
    );
    run(&c, &r, &["queue", "sendmeter", "add", "12", "--needs", "docker"]);
    assert_eq!(
        placed(&c, &r, 12, "impl", None).as_deref(),
        Ok("bazzite"),
        "only bazzite has docker"
    );
    let (_, ls) = run(&c, &r, &["queue", "sendmeter", "ls"]);
    assert!(ls.contains("#12    queued  needs docker"), "{ls}");
    assert_eq!(run(&c, &r, &["queue", "sendmeter", "set", "12", "needs", "gpu"]).0, 0);
    let e = placed(&c, &r, 12, "impl", None).unwrap_err();
    assert_eq!(e, "REFUSED NO_HOST: no host has gpu");
    // A hand tag in fleet.toml covers it.
    let c2 = setup("hosts-needs-hand", "tags = [\"gpu\"]", true, 0);
    run(&c2, &r, &["queue", "sendmeter", "add", "12", "--needs", "gpu"]);
    assert_eq!(placed(&c2, &r, 12, "impl", None).as_deref(), Ok("bazzite"));
    // The fleet's needs add to the item's: nothing has both.
    let text = config("").replace(
        "integration = \"staging\"",
        "integration = \"staging\"\nneeds = [\"xcode\"]",
    );
    std::fs::write(&c.config, text).unwrap();
    run(&c, &r, &["queue", "sendmeter", "set", "12", "needs", "docker"]);
    let e = placed(&c, &r, 12, "impl", None).unwrap_err();
    assert_eq!(e, "REFUSED NO_HOST: no host has docker, xcode");
}

#[test]
fn the_least_busy_host_wins() {
    let c = setup("hosts-busy", "", true, 0);
    assert_eq!(placed(&c, &fake(2.0, 0.2), 12, "impl", None).as_deref(), Ok("bazzite"));
    assert_eq!(placed(&c, &fake(0.2, 2.0), 12, "impl", None).as_deref(), Ok("mini"));
    // `--host` forces one.
    assert_eq!(
        placed(&c, &fake(0.2, 2.0), 12, "impl", Some("bazzite")).as_deref(),
        Ok("bazzite")
    );
}

#[test]
fn admission_runs_per_host() {
    let c = setup("hosts-admit", "", true, 0);
    // mini is over load/cpu 2.5: HOST_BUSY there, so bazzite takes the lane.
    assert_eq!(placed(&c, &fake(3.0, 2.0), 12, "impl", None).as_deref(), Ok("bazzite"));
    // bazzite has 1 GB of disk: DISK_LOW there, so mini takes it although bazzite is idle.
    let df = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/sda 9 8 1048576 90% /\n";
    let low = || fake(1.0, 0.1).reply(&format!("{SSH}'df' '-k' '/'"), df);
    assert_eq!(placed(&c, &low(), 12, "impl", None).as_deref(), Ok("mini"));
    let e = placed(&c, &low(), 12, "impl", Some("bazzite")).unwrap_err();
    assert_eq!(e, "DISK_LOW: bazzite: 1.0 GB free < 10; `herdsman clean --yes`");
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let (code, out) = run(&c, &low(), &["spawn", "sendmeter", "12", "impl", "--host", "bazzite"]);
    assert_eq!(code, 3, "{out}");
    // Every host busy: the least busy one's refusal.
    let e = placed(&c, &fake(3.0, 4.0), 12, "impl", None).unwrap_err();
    assert!(e.starts_with("HOST_BUSY: mini: load/cpu 3.00 > 2.5"), "{e}");
}

/// The impl role gets account A (its token returned); its profiles synced.
fn with_account(c: &herdsman::Ctx) -> &'static str {
    let token = concat!("sk", "-ant-oat01-AAAA-secret");
    let env = c.state.parent().unwrap().join("tokens.env");
    std::fs::write(&env, format!("CLAUDE_CODE_OAUTH_TOKEN_A={token}\n")).unwrap();
    let toml = std::fs::read_to_string(&c.config).unwrap().replacen(
        "harness = \"claude\"",
        "harness = \"claude\"\naccount = \"A\"",
        1,
    ) + &format!(
        "\n[accounts]\naccounts_env = \"{}\"\nA = \"CLAUDE_CODE_OAUTH_TOKEN_A\"\n",
        env.display()
    );
    std::fs::write(&c.config, toml).unwrap();
    assert_eq!(run(c, &Fake::default(), &["profiles", "sync"]).0, 0);
    token
}

/// A fresh lane on bazzite: its clone, worktree, pane and agent start all answer.
fn remote_fake() -> Fake {
    let ws = r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#;
    fake(0.1, 0.1)
        .reply("gh issue view 12", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply(&format!("{SSH}'gh' 'repo' 'clone' 'jirathip-dev/sendmeter'"), "")
        .reply(&format!("{SSH}'git' '-C'"), "")
        .reply(&format!("{SSH}'sh' '-c' 'mkdir"), "")
        .reply("herdr --machine bazzite worktree create", ws)
        .reply(
            "herdr --machine bazzite pane list --workspace w9",
            r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#,
        )
        .reply("herdr --machine bazzite pane run w9:p1 export", "{}")
        .reply("herdr --machine bazzite agent start sendmeter-impl-12", "{}")
        .reply("herdr --machine bazzite agent wait sendmeter-impl-12", "{}")
}

#[test]
fn a_remote_lane_runs_every_command_on_its_host() {
    let c = setup("hosts-spawn", "", true, 0);
    // The impl role has an account: its token must never reach bazzite (Q3).
    let token = with_account(&c);
    run(
        &c,
        &Fake::default(),
        &["queue", "sendmeter", "add", "12", "--needs", "docker"],
    );
    let r = remote_fake();
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    let calls = r.calls.borrow();
    let herdr: Vec<_> = calls.iter().filter(|c| c.starts_with("herdr ")).collect();
    assert!(
        herdr.len() >= 6 && herdr.iter().all(|c| c.starts_with("herdr --machine bazzite ")),
        "{herdr:?}"
    );
    assert!(
        !calls.iter().any(|c| c.starts_with("git ")),
        "git runs on bazzite: {calls:?}"
    );
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with(&format!("{SSH}'gh' 'repo' 'clone'"))),
        "first use clones"
    );
    assert!(
        calls.iter().any(|c| c.contains(&format!("--path {WT} "))),
        "worktree under bazzite's home"
    );
    assert!(calls
        .iter()
        .any(|c| c.starts_with(&format!("{SSH}'sh' '-c' 'mkdir")) && c.ends_with(&format!("'{WT}/.brief.md'"))));
    assert!(
        !calls.iter().any(|c| c.contains("OAUTH_TOKEN") || c.contains(token)),
        "token sent to bazzite: {calls:?}"
    );
    let start = calls.iter().find(|c| c.contains("agent start")).unwrap();
    assert!(start.contains(&format!("Read {WT}/.brief.md")), "{start}");
    drop(calls);
    let e = read(&c.state)
        .unwrap()
        .into_iter()
        .rfind(|e| e.kind == "spawn")
        .unwrap();
    assert_eq!(
        (e.str("host"), e.str("machine"), e.str("worktree"), e.str("account")),
        (Some("bazzite"), Some("bazzite"), Some(WT), None)
    );
    // Its review round goes to the same host (the worktree lives there).
    assert_eq!(
        placed(&c, &fake(0.1, 3.0), 12, "rev", None).unwrap_err(),
        "HOST_BUSY: bazzite: load/cpu 3.00 > 2.5"
    );
    let (_, status) = run(&c, &r, &["status"]);
    assert!(status.contains("HOST") && status.contains(" bazzite "), "{status}");
}

#[test]
fn clean_removes_a_remote_worktree_over_its_host() {
    let c = setup("hosts-clean", "", true, 0);
    remote_spawn(&c, NOW - 900);
    ev(
        &c,
        NOW - 100,
        12,
        "merged",
        json!({"pr": "34", "merge_sha": "abc", "into": "staging"}),
    );
    let r = fake(1.0, 1.0)
        .reply(&format!("{SSH}'sh' '-c' 'test -e"), "")
        .reply(&format!("{SSH}'git' '-C' '{WT}' 'rev-list'"), "0\n")
        .reply(&format!("{SSH}'git' '-C' '{WT}' 'status'"), "")
        .reply("herdr --machine bazzite worktree remove --workspace w9", "{}")
        .reply("herdr --machine bazzite pane close w9:p1", "{}");
    let (code, out) = run(&c, &r, &["clean", "sendmeter", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&format!("removed {WT} (sendmeter#12 merged)")), "{out}");
    assert!(r.called("herdr --machine bazzite worktree remove --workspace w9"));
    assert!(r.called("herdr --machine bazzite pane close w9:p1"));
    assert!(!r.calls.borrow().iter().any(|c| c.starts_with("git ")));
}

#[test]
fn an_offline_host_never_pages_and_shows_grey() {
    let c = setup("hosts-offline", "", false, 1980);
    remote_spawn(&c, NOW - 4000);
    // A lane on this host in the same state does page.
    ev(
        &c,
        NOW - 4000,
        13,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-13", "branch": "issue-13"}),
    );
    let r = Fake::default().reply("herdr agent list", NO_AGENTS);
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    let pages: Vec<_> = read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| ["lane-dead", "stall"].contains(&e.kind.as_str()))
        .collect();
    assert!(!pages.is_empty() && pages.iter().all(|e| e.issue == Some(13)), "{out}");
    let (_, hosts) = run(&c, &r, &["hosts"]);
    assert!(
        hosts.contains("bazzite · offline · last seen 33m · tags os:linux,arch:x86_64,docker"),
        "{hosts}"
    );
    assert!(hosts.contains("1 lane(s) sendmeter-impl-12"), "{hosts}");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let page =
        String::from_utf8_lossy(&s.respond(&format!("GET / HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), NOW)).into_owned();
    let g = &page[page
        .find("<article class=hcd data-h=\"bazzite\"")
        .expect("a bazzite card")..];
    let g = &g[..g.find("</article>").unwrap()];
    assert!(g.contains("<span class=\"hw m\">offline · last seen 33m</span>"), "{g}");
    assert!(g.split("data-t=").skip(1).all(|t| t.starts_with('m')), "all grey: {g}");
    let mini = page.find("data-h=\"mini\"").expect("a mini card");
    assert!(mini < page.find("data-h=\"bazzite\"").unwrap(), "main first");
}

#[test]
fn an_off_host_is_never_chosen_but_drains_its_lanes() {
    let c = setup("hosts-off", "enabled = false", true, 0);
    let r = fake(2.0, 0.1);
    assert_eq!(
        placed(&c, &r, 12, "impl", None).as_deref(),
        Ok("mini"),
        "bazzite is idler but off"
    );
    run(&c, &r, &["queue", "sendmeter", "add", "14", "--needs", "docker"]);
    assert_eq!(
        placed(&c, &r, 14, "impl", None).unwrap_err(),
        "REFUSED NO_HOST: no enabled host has docker"
    );
    // Its running lane still gets its review and fix rounds there.
    remote_spawn(&c, NOW - 60);
    assert_eq!(placed(&c, &r, 12, "rev", None).as_deref(), Ok("bazzite"));
    assert_eq!(placed(&c, &r, 12, "impl", None).as_deref(), Ok("bazzite"));
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let page =
        String::from_utf8_lossy(&s.respond(&format!("GET / HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), NOW)).into_owned();
    assert!(
        page.contains("<span class=\"hw m\">off · draining 1 lane</span>"),
        "draining shown"
    );
    // Every host off: nothing new starts.
    let text = std::fs::read_to_string(&c.config)
        .unwrap()
        .replace("tags = [\"fast\"]", "tags = [\"fast\"]\nenabled = false");
    std::fs::write(&c.config, text).unwrap();
    assert_eq!(
        placed(&c, &r, 15, "impl", None).unwrap_err(),
        "REFUSED NO_HOST: no enabled host"
    );
}

#[test]
fn cli_and_board_toggles_both_log() {
    let c = setup("hosts-toggle", "", true, 0);
    let r = fake(1.0, 1.0);
    assert_eq!(
        run(&c, &r, &["hosts", "off", "bazzite"]),
        (0, "bazzite: off for new lanes\n".into())
    );
    assert!(std::fs::read_to_string(&c.config)
        .unwrap()
        .contains("machine = \"bazzite\"\nenabled = false\n"));
    let last = |c: &herdsman::Ctx| read(&c.state).unwrap().pop().unwrap();
    let e = last(&c);
    assert_eq!(
        (e.kind.as_str(), e.str("host"), e.str("by")),
        ("host-off", Some("bazzite"), Some("cli"))
    );
    assert_eq!(run(&c, &r, &["hosts", "off", "nope"]).0, 1);
    // The board's [on] button: paired devices only.
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let get =
        || String::from_utf8_lossy(&s.respond(&format!("GET / HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), NOW)).into_owned();
    let page = get();
    assert!(
        page.contains("<span class=\"hw m\">off · not used for lanes</span>")
            && page.contains("name=to value=on><button>on</button>")
    );
    let i = page.find("name=mtime value=").unwrap() + 17;
    let mt: String = page[i..].chars().take_while(char::is_ascii_digit).collect();
    let body = format!("op=host&fleet=&host=bazzite&to=on&mtime={mt}");
    let post = |cookie: &str| {
        let req = format!(
            "POST / HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        String::from_utf8_lossy(&s.respond(&req, NOW)).into_owned()
    };
    assert!(post("").starts_with("HTTP/1.1 401"), "unpaired");
    let cookie = pair(&s, &c, "", NOW);
    let res = post(&cookie);
    assert!(res.contains("bazzite%20on") || res.contains("bazzite on"), "{res}");
    let e = last(&c);
    assert_eq!(
        (e.kind.as_str(), e.str("host"), e.str("by")),
        ("host-on", Some("bazzite"), Some("web"))
    );
    assert!(std::fs::read_to_string(&c.config).unwrap().contains("enabled = true"));
}

#[test]
fn lane_cards_and_the_herd_name_the_host() {
    let c = setup("hosts-cards", "", true, 0);
    remote_spawn(&c, NOW - 60);
    let row = r#"{"result":{"agents":[{"name":"sendmeter-impl-12","agent_status":"working","pane_id":"w9:p1"}]}}"#;
    let r = Fake::default()
        .reply("herdr --machine bazzite agent list", row)
        .reply("herdr agent list", NO_AGENTS);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let page =
        String::from_utf8_lossy(&s.respond(&format!("GET / HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), NOW)).into_owned();
    let card = &page[page.find("data-agent=\"sendmeter-impl-12\"").expect("lane row")..];
    assert!(
        card[..card.find("</div></div>").unwrap()].contains("impl · bazzite · round 0 · claude"),
        "{card}"
    );
    let herd = String::from_utf8_lossy(&s.respond(&format!("GET /herd.json HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), NOW))
        .into_owned();
    assert!(herd.contains("\"host\":\"bazzite\""), "{herd}");
}

/// #236: a dry run places the lane first and prints the argvs exactly as a real spawn sends them: bazzite's home in
/// every path, `--machine`, and no account token.
#[test]
fn a_remote_dry_run_prints_the_argv_the_spawn_sends() {
    let c = setup("hosts-dry", "", true, 0);
    let token = with_account(&c);
    let text = std::fs::read_to_string(&c.config).unwrap().replace(
        "local = \"/tmp/does-not-matter/sendmeter\"",
        "local = \"~/src/sendmeter\"\nenv = { TOOLS = \"~/bin\" }",
    );
    std::fs::write(&c.config, text).unwrap();
    let r = remote_fake();
    let cfg = Config::load(&c.config).unwrap();
    herdsman::gh::spec(&c, &r, cfg.fleet("sendmeter").unwrap(), "sendmeter", 12, true).unwrap();
    let args = ["spawn", "sendmeter", "12", "impl", "--host", "bazzite", "--json"];
    let (code, dry) = run(&c, &r, &[&args[..], &["--dry-run"]].concat());
    assert_eq!(code, 0, "{dry}");
    let pj: serde_json::Value = serde_json::from_str(&dry).unwrap();
    let mine = herdsman::config::home().display().to_string();
    let cmds: Vec<String> = pj["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let w: Vec<&str> = c.as_array().unwrap().iter().map(|w| w.as_str().unwrap()).collect();
            w.join(" ").replace("<pane>", "w9:p1")
        })
        .collect();
    let all = cmds.join("\n");
    assert!(!all.contains("OAUTH_TOKEN") && !all.contains(token), "{all}");
    assert!(pj["account"].is_null() && pj["machine"] == "bazzite", "{pj}");
    assert!(!all.contains(&format!("{mine}/")), "main host's home: {all}");
    assert!(all.contains("--cwd /home/me/src/sendmeter --branch issue-12"), "{all}");
    assert!(
        all.contains("TOOLS='/home/me/bin'") && all.contains(&format!("Read {WT}/.brief.md")),
        "{all}"
    );
    assert!(
        cmds[0].starts_with(&format!("{SSH}'git' '-C' '/home/me/src/sendmeter' 'fetch'")),
        "{all}"
    );
    assert!(
        cmds[1..].iter().all(|c| c.starts_with("herdr --machine bazzite ")),
        "{all}"
    );
    // Golden: the real spawn sends exactly these argvs.
    let n = r.calls.borrow().len();
    let (code, out) = run(&c, &r, &args);
    assert_eq!(code, 0, "{out}");
    let sent = r.calls.borrow()[n..].to_vec();
    for c in &cmds {
        assert!(sent.contains(c), "planned but not sent: {c}\nsent: {sent:#?}");
    }
}

/// #236: a design lane runs on the main host whatever the issue's history or the load says; another host is refused.
/// A role pin (`host = "<name>"` or `"main"`) beats least-busy placement.
#[test]
fn design_runs_on_the_main_host_only_and_a_role_pin_holds() {
    let c = setup("hosts-design", "", true, 0);
    remote_spawn(&c, NOW - 900);
    let r = fake(1.0, 0.1);
    assert_eq!(placed(&c, &r, 12, "impl", None).as_deref(), Ok("bazzite"), "history");
    assert_eq!(placed(&c, &r, 12, "design", None).as_deref(), Ok("mini"));
    assert_eq!(placed(&c, &r, 13, "design", Some("mini")).as_deref(), Ok("mini"));
    let main_only = "REFUSED DESIGN_MAIN_ONLY: host bazzite: design lanes run on the main host only";
    assert_eq!(placed(&c, &r, 13, "design", Some("bazzite")).unwrap_err(), main_only);
    let text = std::fs::read_to_string(&c.config).unwrap().replace(
        "profile = \"fleet-rev\"",
        "profile = \"fleet-rev\"\nhost = \"bazzite\"\n\n[fleet.sendmeter.role.design]\nharness = \"claude\"\nhost = \"bazzite\"",
    );
    std::fs::write(&c.config, &text).unwrap();
    assert_eq!(
        placed(&c, &fake(0.1, 2.0), 13, "rev", None).as_deref(),
        Ok("bazzite"),
        "pinned"
    );
    assert_eq!(
        placed(&c, &r, 13, "design", None).unwrap_err(),
        main_only,
        "a pin can't move design"
    );
    let (code, out) = run(
        &c,
        &r,
        &["spawn", "sendmeter", "13", "design", "--host", "bazzite", "--dry-run"],
    );
    assert_eq!((code, out.trim()), (3, main_only));
    let main = text.replace("host = \"bazzite\"", "host = \"main\"");
    std::fs::write(&c.config, main).unwrap();
    assert_eq!(placed(&c, &fake(2.0, 0.1), 13, "rev", None).as_deref(), Ok("mini"));
    assert_eq!(placed(&c, &r, 13, "design", None).as_deref(), Ok("mini"));
}

/// #236: an agent that fails to start on another host leaves no half-made workspace there.
#[test]
fn a_failed_remote_start_closes_its_workspace() {
    let c = setup("hosts-fail", "", true, 0);
    with_account(&c);
    let r = remote_fake()
        .once_err("herdr --machine bazzite agent start", 1, r#"{"error":{"code":"boom"}}"#)
        .reply("herdr --machine bazzite workspace close w9", "{}");
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl", "--host", "bazzite"]);
    assert_eq!(code, 1, "{out}");
    assert!(r.called("herdr --machine bazzite workspace close w9"));
    assert!(!r.called("herdr --machine bazzite pane close"));
}

/// #249: a review on the remote host: its verdict.md is read over ssh and recorded, its pane closed with `--machine`
/// (one a worktree remove already closed counts), and the board names the host.
#[test]
fn a_remote_review_is_read_over_ssh_and_recorded() {
    let c = setup("hosts-verdict", "", true, 0);
    remote_spawn(&c, NOW - 900);
    let rev = "/home/me/.herdr/worktrees/sendmeter/rev-12-r1";
    let head = "a".repeat(40);
    let d = json!({"role": "rev", "agent": "sendmeter-rev-12-r1", "harness": "claude", "pane": "w8:p1",
                   "workspace": "w8", "worktree": rev, "branch": "rev-12-r1", "sha": head, "host": "bazzite",
                   "machine": "bazzite"});
    ev(&c, NOW - 600, 12, "spawn", d);
    let pr = json!({"number": 34, "state": "OPEN", "baseRefName": "staging", "headRefOid": head,
                    "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "IN_PROGRESS"}]});
    let r = fake(1.0, 1.0)
        .reply("gh pr view issue-12", &pr.to_string())
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply(
            &format!("{SSH}'cat' '{rev}/verdict.md'"),
            "FAIL\n- blocking [code] a.rs:1\n",
        )
        .reply(&format!("{SSH}'git' '-C' '{WT}' 'log'"), &format!("{head} 20"))
        .reply(&format!("{SSH}'sh' '-c' 'test -e"), "")
        .reply(&format!("{SSH}'git' '-C' '{rev}' 'rev-list'"), "0\n")
        .reply(&format!("{SSH}'git' '-C' '{rev}' 'status'"), "")
        .reply("herdr --machine bazzite worktree remove --workspace w8", "{}")
        .once_err(
            "herdr --machine bazzite pane close w8:p1",
            1,
            r#"{"error":{"code":"pane_not_found"}}"#,
        );
    let cfg = Config::load(&c.config).unwrap();
    let w = herdsman::waiting::scan(&c, &cfg, &r, &read(&c.state).unwrap(), &[], true);
    let w = w.iter().find(|(s, _)| s.issue == 12).unwrap();
    assert_eq!(w.1.what, "review done on bazzite · recording within 1 min");
    let (code, out) = run(&c, &r, &["drive", "--fast"]);
    assert_eq!(code, 0, "{out}");
    let all = read(&c.state).unwrap();
    let v = all.iter().find(|e| e.kind == "verdict").expect(&out);
    assert_eq!((v.str("verdict"), v.str("sha")), (Some("FAIL"), Some(head.as_str())));
    assert!(r.called("herdr --machine bazzite pane close w8:p1"));
    assert!(all.iter().any(|e| e.kind == "lane-closed"), "{out}");
}

/// #275: a remote lane is told the main host (`HERDSMAN_MAIN`, its path never rehomed) and an answer to it lands
/// with `--machine`: the prompt, then Enter, then the wait that confirms it.
#[test]
fn a_remote_lane_knows_the_main_host_and_answers_reach_it() {
    let c = setup("hosts-answer", "", true, 0);
    with_account(&c);
    let r = remote_fake();
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl", "--host", "bazzite"]);
    assert_eq!(code, 0, "{out}");
    let export = "herdr --machine bazzite pane run w9:p1 export";
    let calls = r.calls.borrow().clone();
    let lane = calls.iter().find(|c| c.starts_with(export)).unwrap();
    assert!(
        lane.ends_with(" HERDSMAN_MAIN='mini:/opt/herdsman/bin/herdsman'"),
        "{lane}"
    );
    let d = json!({"id": "q1", "question": "Which?", "options": ["a", "b"], "agent": "sendmeter-impl-12"});
    ev(&c, NOW, 12, "needs-decision", d);
    let r = Fake::default()
        .reply(
            "herdr --machine bazzite agent prompt sendmeter-impl-12 DECISION q1: a",
            "{}",
        )
        .reply("herdr --machine bazzite pane send-keys w9:p1 Enter", "{}")
        .reply("herdr --machine bazzite agent wait sendmeter-impl-12", "{}");
    let (code, out) = run(&c, &r, &["answer", "sendmeter", "q1", "1"]);
    assert_eq!(
        (code, out.as_str()),
        (0, "sendmeter q1: a -> sendmeter-impl-12\n"),
        "{:?}",
        r.calls.borrow()
    );
    assert_eq!(r.calls.borrow().len(), 3, "{:?}", r.calls.borrow());
}

/// #275: `agent_name_not_found` on another host: one more start when no harness came up, none when one did (it is
/// named instead); every call on that host, and no workspace closed.
#[test]
fn a_lost_agent_name_retries_once_without_a_double_start() {
    let lost =
        r#"{"error":{"code":"agent_name_not_found","message":"named agent no longer owns the target terminal"}}"#;
    let start = "herdr --machine bazzite agent start";
    for up in [false, true] {
        let c = setup("hosts-lost", "", true, 0);
        with_account(&c);
        let pane = json!({"result": {"pane": {"pane_id": "w9:p1", "agent": if up { "claude" } else { "" }}}});
        let mut r = remote_fake()
            .once_err(start, 1, lost)
            .reply("herdr --machine bazzite agent rename w9:p1 sendmeter-impl-12", "{}");
        if up {
            r = r.reply("herdr --machine bazzite pane get w9:p1", &pane.to_string());
        }
        let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl", "--host", "bazzite"]);
        assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
        assert_eq!(r.count(start), if up { 1 } else { 2 });
        assert_eq!(r.called("herdr --machine bazzite agent rename"), up);
        assert!(!r.called("herdr --machine bazzite workspace close"));
        let calls = r.calls.borrow();
        assert!(calls
            .iter()
            .filter(|c| c.starts_with("herdr "))
            .all(|c| c.starts_with("herdr --machine bazzite ")));
    }
}

/// #275: asks a lane logged on its own host before forwarding are imported into the main log once, under a new id.
#[test]
fn asks_stranded_on_a_remote_host_are_imported_once() {
    let c = setup("hosts-import", "", true, 0);
    remote_spawn(&c, NOW - 900);
    let d = json!({"id": "q1", "question": "Old?", "options": [], "agent": "x"});
    ev(&c, NOW - 800, 9, "needs-decision", d);
    let stranded = json!({"ts": NOW - 600, "fleet": "sendmeter", "issue": 12, "type": "needs-decision",
                          "id": "q59", "question": "Which way?", "options": ["a"], "agent": null});
    let log = "/home/me/.local/state/herdsman/events.jsonl";
    let r = fake(1.0, 1.0).reply(&format!("{SSH}'cat' '{log}'"), &format!("{stranded}\n"));
    for _ in 0..2 {
        assert_eq!(run(&c, &r, &["check"]).0, 0);
    }
    let all = read(&c.state).unwrap();
    let asks: Vec<_> = all
        .iter()
        .filter(|e| e.kind == "needs-decision" && e.issue == Some(12))
        .collect();
    assert_eq!(asks.len(), 1, "imported once");
    let a = asks[0];
    assert_eq!(
        (a.str("id"), a.str("from"), a.str("agent"), a.ts),
        (Some("q2"), Some("bazzite:q59"), Some("sendmeter-impl-12"), NOW - 600)
    );
    let (_, out) = run(&c, &r, &["asks", "sendmeter"]);
    assert!(out.contains("q2 #12") && out.contains("Which way?"), "{out}");
}
