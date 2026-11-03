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

const HERDR_STATUS: &str = "Client:\n  version: 0.9.5\n\nserver:\n  version: 0.9.5\n";

/// #235: replies that make bazzite healthy: herdr same version in its login session, git over SSH, gh and Claude
/// logged in inside a pane, and this herdsman's version.
fn healthy(f: Fake) -> Fake {
    f.reply(&format!("{SSH}'sh' '-c' 'PATH="), HERDR_STATUS)
        .reply(&format!("{SSH}'sh' '-c' 'p=$(pgrep"), "")
        .reply(&format!("{SSH}'git' 'ls-remote'"), "abc\tHEAD\n")
        .reply(
            "herdr --machine bazzite workspace create",
            r#"{"result":{"root_pane":{"pane_id":"w5:p1"}}}"#,
        )
        .reply("herdr --machine bazzite pane read w5:p1", "gh:0\nclaude:0\nhs:done\n")
        .reply(
            &format!("{SSH}'/opt/herdsman/bin/herdsman' '--version'"),
            &format!("herdsman {}\n", herdsman::VERSION),
        )
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
    let r = healthy(fake(1.0, 1.0))
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

#[test]
fn a_remote_lane_runs_every_command_on_its_host() {
    let c = setup("hosts-spawn", "", true, 0);
    // The impl role has an account: its token must never reach bazzite (Q3).
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
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    run(
        &c,
        &Fake::default(),
        &["queue", "sendmeter", "add", "12", "--needs", "docker"],
    );
    let ws = r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#;
    let r = fake(0.1, 0.1)
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
        .reply("herdr --machine bazzite agent wait sendmeter-impl-12", "{}");
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
    let g = &page[page.find("<span class=gn>bazzite</span>").expect("a bazzite group")..];
    let g = &g[..g.find("</details>").unwrap()];
    assert!(g.contains("<b class=m>offline · last seen 33m</b>"), "{g}");
    assert!(
        g.split("data-t=").skip(1).all(|t| t.starts_with('m')),
        "all grey, no gauges: {g}"
    );
    assert!(page.contains("<span class=gn>mini</span>"));
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
    assert!(page.contains("<b class=m>off · draining 1 lane</b>"), "draining shown");
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
        page.contains("<b class=m>off · not used for lanes</b>")
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

/// #235: bazzite's health rows with every check passing but the one `bad` prefix, which fails.
fn health_with(c: &herdsman::Ctx, bad: &str) -> Vec<String> {
    let cfg = Config::load(&c.config).unwrap();
    let r = healthy(fake(0.1, 0.1)).once(bad, 1, "Client:\n  version: 0.9.5\n\nserver:\n  version: 0.9.0\n");
    let on = herdsman::hosts::On::new(&r, Some("bazzite"));
    herdsman::hosts::health(c, &cfg, &on, &["docker".into()], "/home/me")
}

#[test]
fn each_health_check_says_what_failed_and_how_to_fix_it() {
    let c = setup("hosts-health", "", true, 0);
    assert!(health_with(&c, "nothing").is_empty(), "all ✓");
    let cases = [
        (format!("{SSH}'sh' '-c' 'PATH="), "✗ herdr — server none, client ? → herdr update on the host"),
        (format!("{SSH}'sh' '-c' 'p=$(pgrep"), "✗ herdr session — the server runs outside the login session (keychain locked) → restart the herdr server from its launchd job: `launchctl kickstart -k gui/<uid>/<label>`"),
        (format!("{SSH}'git' 'ls-remote'"), "✗ git sendmeter — ls-remote https://github.com/jirathip-dev/sendmeter.git → add the host's SSH key to GitHub and set `git config --global url.git@github.com:.insteadOf https://github.com/`"),
        ("herdr --machine bazzite workspace create".into(), "✗ gh — not logged in inside a herdr pane → gh auth login"),
        (format!("{SSH}'/opt/herdsman/bin/herdsman'"), "! herdsman — missing there, herdsman 0.1.0"),
    ];
    for (bad, want) in cases {
        let rows = health_with(&c, &bad);
        assert!(rows[0].starts_with(want), "{bad}: {rows:?}");
    }
    // A server older than its client (a herdr update without a restart).
    let r = healthy(fake(0.1, 0.1)).once(
        &format!("{SSH}'sh' '-c' 'PATH="),
        0,
        "Client:\n  version: 0.9.5\n\nserver:\n  version: 0.9.0\n",
    );
    let cfg = Config::load(&c.config).unwrap();
    let rows = herdsman::hosts::health(&c, &cfg, &herdsman::hosts::On::new(&r, Some("bazzite")), &[], "/h");
    assert!(rows[0].starts_with("✗ herdr — server 0.9.0, client 0.9.5"), "{rows:?}");
    // Logged out of Claude only (the pane's screen).
    let r = healthy(fake(0.1, 0.1)).once("herdr --machine bazzite pane read", 0, "gh:0\nclaude:1\nhs:done\n");
    let rows = herdsman::hosts::health(&c, &cfg, &herdsman::hosts::On::new(&r, Some("bazzite")), &[], "/h");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(rows[0].starts_with("✗ claude — not logged in inside a herdr pane → claude auth login"));
    assert!(
        r.called("herdr --machine bazzite workspace close w5"),
        "the throwaway workspace is closed"
    );
    // doctor lists each failure under the host, and exits 1 on a ✗.
    let r = healthy(fake(0.1, 0.1))
        .reply(&format!("{SSH}'sh' '-c' 'uname"), "Linux\nx86_64\n/home/me\ndocker\n")
        .once(&format!("{SSH}'git' 'ls-remote'"), 128, "");
    let (code, out) = run(&c, &r, &["doctor"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("\n✗ host bazzite git sendmeter — ls-remote"), "{out}");
    let (_, out) = run(&c, &healthy(r), &["doctor"]);
    assert!(
        out.contains("\n✓ host bazzite — herdr, session, git, gh, claude, herdsman 0.1.0 ("),
        "{out}"
    );
}

#[test]
fn an_unhealthy_host_takes_no_lanes_and_shows_red() {
    let c = setup("hosts-sick", "", true, 0);
    let mut all = cached(&c);
    all.get_mut("bazzite").unwrap().health = vec!["✗ gh — not logged in inside a herdr pane → gh auth login".into()];
    std::fs::write(
        c.state.with_file_name("hosts.json"),
        serde_json::to_string(&all).unwrap(),
    )
    .unwrap();
    let r = fake(2.0, 0.1);
    assert_eq!(
        placed(&c, &r, 12, "impl", None).as_deref(),
        Ok("mini"),
        "idle but unhealthy bazzite is skipped"
    );
    run(&c, &r, &["queue", "sendmeter", "add", "12", "--needs", "docker"]);
    let e = placed(&c, &r, 12, "impl", None).unwrap_err();
    assert_eq!(
        e,
        "NO_HOST: bazzite: unhealthy: ✗ gh — not logged in inside a herdr pane → gh auth login"
    );
    let cfg = Config::load(&c.config).unwrap();
    let s = &cached(&c)["bazzite"];
    let (tone, words) = herdsman::hosts::shown(&cfg.hosts["bazzite"], s, 0, NOW);
    assert_eq!(
        (tone, words.as_str()),
        (
            "r",
            "unhealthy · gh — not logged in inside a herdr pane → gh auth login"
        )
    );
}

#[test]
fn an_older_herdsman_there_warns_and_is_rebuilt_at_this_sha_once() {
    let extra =
        "\n[fleet.herdsman]\nrepo = \"jirathip-dev/herdsman\"\nlocal = \"~/src/herdsman\"\nintegration = \"staging\"\n";
    let c = setup("hosts-update", "", true, 0);
    std::fs::write(&c.config, config("") + extra).unwrap();
    let old = healthy(fake(0.1, 0.1))
        .reply(&format!("{SSH}'sh' '-c' 'uname"), "Linux\nx86_64\n/home/me\ndocker\n")
        .reply("sh -c uname -s", "Darwin\narm64\n/Users/me\n")
        .reply(&format!("{SSH}'sh' '-c' 'nohup"), "")
        .once(
            &format!("{SSH}'/opt/herdsman/bin/herdsman' '--version'"),
            0,
            "herdsman 0.1.0 (0ld)\n",
        );
    let (code, out) = run(&c, &old, &["hosts", "refresh"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("bazzite · up · herdsman — herdsman 0.1.0 (0ld) there"),
        "amber, not red: {out}"
    );
    let sha = env!("HERDSMAN_SHA");
    let script = format!(
        "{SSH}'sh' '-c' 'nohup sh -c \"$1\" </dev/null >\"$HOME/.herdsman-update.log\" 2>&1 &' 'sh' 'cd '\\''/home/me/src/herdsman'\\'' && git fetch -q origin && git checkout -q --detach {sha} && PATH=\"$HOME/.cargo/bin:$PATH\" cargo install --path . --locked --root '\\''/opt/herdsman'\\'' --force && '\\''/opt/herdsman/bin/herdsman'\\'' profiles sync'"
    );
    assert!(old.called(&script), "{:#?}", old.calls.borrow());
    let ev = read(&c.state).unwrap();
    let up = ev.iter().find(|e| e.kind == "host-update").unwrap();
    assert_eq!((up.str("host"), up.str("sha")), (Some("bazzite"), Some(sha)));
    assert!(cached(&c)["bazzite"].health[0].starts_with("! herdsman"));
    // Still old on the next probe: no second build of the same sha.
    let again = healthy(fake(0.1, 0.1))
        .reply(&format!("{SSH}'sh' '-c' 'uname"), "Linux\nx86_64\n/home/me\ndocker\n")
        .reply("sh -c uname -s", "Darwin\narm64\n/Users/me\n")
        .once(
            &format!("{SSH}'/opt/herdsman/bin/herdsman' '--version'"),
            0,
            "herdsman 0.1.0 (0ld)\n",
        );
    run(&c, &again, &["hosts", "refresh"]);
    assert!(!again.called(&format!("{SSH}'sh' '-c' 'nohup")));
    // The build landed: the version reads back the same and the warning clears.
    run(
        &c,
        &healthy(fake(0.1, 0.1)).reply(&format!("{SSH}'sh' '-c' 'uname"), "Linux\nx86_64\n/home/me\ndocker\n"),
        &["hosts", "refresh"],
    );
    assert!(cached(&c)["bazzite"].health.is_empty());
}
