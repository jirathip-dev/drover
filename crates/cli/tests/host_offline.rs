//! #478: one unreachable host (bazzite, its `herdr --machine bazzite agent list` failing) refuses only its own spawns
//! `HOST_OFFLINE`; its lanes show `host offline` (never `lane-dead`), `check` alerts its drop and return once each, a
//! supersede's pane left there is killed once it is back, and `kill`/`close --host` pick one host's agent of a name.
mod common;
use common::*;
use herdsman::config::Config;
use herdsman::events::{append, read, Event};
use serde_json::json;

const NOW: u64 = 1_000_000;
const WT: &str = "/home/me/.herdr/worktrees/sendmeter/issue-12";

fn setup(name: &str) -> herdsman::Ctx {
    let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    let toml = format!("{base}\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n");
    let c = ctx(&tmpdir(name), &toml);
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "bazzite": {"tags": ["os:linux"], "home": "/home/me", "probed": NOW, "seen": NOW, "up": true},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    c
}

fn ev(c: &herdsman::Ctx, ts: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

/// #12's impl lane, spawned on bazzite in pane w9:p1.
fn remote_spawn(c: &herdsman::Ctx) {
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "pane": "w9:p1", "workspace": "w9",
                   "worktree": WT, "branch": "issue-12", "host": "bazzite", "machine": "bazzite"});
    ev(c, NOW - 900, "spawn", d);
}

/// The main host answers a full spawn; bazzite answers nothing (`agent list` fails: unreachable).
fn mini_only() -> Fake {
    Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }")
        .reply("sysctl -n hw.ncpu", "4")
        .reply("gh issue view", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply("git ", "")
        .reply(
            "herdr worktree create",
            r#"{"result":{"workspace":{"workspace_id":"w1"}}}"#,
        )
        .reply(
            "herdr pane list --workspace w1",
            r#"{"result":{"panes":[{"pane_id":"w1:p1","agent":null}]}}"#,
        )
        .reply("herdr pane run w1:p1 export", "{}")
        .reply("herdr agent start", "{}")
        .reply("herdr agent wait", "{}")
}

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

#[test]
fn an_unreachable_host_refuses_only_its_own_spawns() {
    let c = setup("offline-spawn");
    let r = mini_only();
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "13", "impl", "--host", "bazzite"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED HOST_OFFLINE: bazzite: unreachable"), "{out}");
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "14", "impl", "--host", "mini"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    assert!(r.called("herdr agent start sendmeter-impl-14"));
    // #676: a lane whose issue last ran on bazzite leaves it while it is unreachable: the main host takes it.
    remote_spawn(&c);
    ev(&c, NOW - 100, "verdict", json!({"verdict": "FAIL", "sha": "abc"}));
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "rev"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        r.called("herdr agent start sendmeter-rev-12-r2"),
        "{:?}",
        r.calls.borrow()
    );
}

#[test]
fn a_logged_lane_on_the_down_host_still_blocks_a_duplicate() {
    let c = setup("offline-dup");
    remote_spawn(&c);
    let r = mini_only();
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl", "--host", "mini"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED LANE_LIVE: unfinished impl lane"), "{out}");
    assert!(!r.called("herdr agent start"), "{:?}", r.calls.borrow());
}

#[test]
fn its_lanes_show_host_offline_alert_once_and_recover() {
    let c = setup("offline-show");
    remote_spawn(&c);
    let down = mini_only();
    for _ in 0..2 {
        assert_eq!(run(&c, &down, &["check"]).0, 0);
    }
    let off = kinds(&c, "host-offline");
    assert_eq!(off.len(), 1, "once per drop");
    assert_eq!(off[0].str("host"), Some("bazzite"));
    assert!(off[0].str("reason").unwrap().contains("sendmeter-impl-12"), "{off:?}");
    assert!(kinds(&c, "lane-dead").is_empty(), "a lane on a down host is not dead");
    let (_, board) = run(&c, &down, &["board", "sendmeter"]);
    assert!(board.contains("host offline: bazzite"), "{board}");
    let (_, status) = run(&c, &down, &["status"]);
    assert!(status.contains("host offline: bazzite"), "{status}");
    let (_, w) = run(&c, &down, &["waiting", "--json"]);
    let w: serde_json::Value = serde_json::from_str(&w).unwrap();
    let row = w.as_array().unwrap().iter().find(|x| x["issue"] == 12).unwrap().clone();
    assert_eq!(
        (row["who"].as_str(), &row["next_cmd"]),
        (Some("none"), &json!(null)),
        "drive does nothing"
    );

    // Back: one `host-online`, and the lane shows its real state again.
    let up = mini_only().reply(
        "herdr --machine bazzite agent list",
        &agents(&[("sendmeter-impl-12", "working", WT, "w9:p1")]),
    );
    for _ in 0..2 {
        assert_eq!(run(&c, &up, &["check"]).0, 0);
    }
    assert_eq!(kinds(&c, "host-online").len(), 1);
    let (_, status) = run(&c, &up, &["status"]);
    assert!(!status.contains("host offline"), "{status}");
    let (_, board) = run(&c, &up, &["board", "sendmeter"]);
    assert!(!board.contains("host offline"), "{board}");
    // It drops again: a second `host-offline`.
    assert_eq!(run(&c, &down, &["check"]).0, 0);
    assert_eq!(kinds(&c, "host-offline").len(), 2);
}

#[test]
fn a_pane_a_supersede_left_on_a_down_host_is_killed_once_it_is_back() {
    let c = setup("offline-supersede");
    remote_spawn(&c);
    let r = mini_only();
    let args = ["spawn", "sendmeter", "12", "impl", "--supersede", "--host", "mini"];
    let (code, out) = run(&c, &r, &args);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    let s = kinds(&c, "supersede").pop().unwrap();
    assert_eq!(
        (s.str("stale_host"), s.str("stale_pane")),
        (Some("bazzite"), Some("w9:p1"))
    );
    let cfg = Config::load(&c.config).unwrap();
    // Still down: nothing happens.
    herdsman::check::stale(&c, &cfg, &r, &mut Vec::new()).unwrap();
    assert!(kinds(&c, "stale-pane-killed").is_empty());
    // Back, with the old pane there and the new lane of the same name on mini.
    let back = Fake::default()
        .reply(
            "herdr agent list",
            &agents(&[("sendmeter-impl-12", "working", "/x", "w1:p1")]),
        )
        .once(
            "herdr --machine bazzite agent list",
            0,
            &agents(&[("sendmeter-impl-12", "idle", WT, "w9:p1")]),
        )
        .reply("herdr --machine bazzite agent list", NO_AGENTS)
        .reply("herdr --machine bazzite pane close w9:p1", "{}");
    for _ in 0..2 {
        herdsman::check::stale(&c, &cfg, &back, &mut Vec::new()).unwrap();
    }
    assert_eq!(
        back.count("herdr --machine bazzite pane close w9:p1"),
        1,
        "{:?}",
        back.calls.borrow()
    );
    assert!(!back.called("herdr pane close"), "the new lane is untouched");
    let k = kinds(&c, "stale-pane-killed");
    assert_eq!(
        (k.len(), k[0].str("host"), k[0].str("pane")),
        (1, Some("bazzite"), Some("w9:p1"))
    );
    assert!(kinds(&c, "lane-killed").is_empty());
    let (_, status) = run(&c, &back, &["status"]);
    assert!(
        status.contains("impl") && status.contains("sendmeter-impl-12"),
        "{status}"
    );
}

/// Mini's live lane and bazzite's stale pane share the name: `--host` picks bazzite's.
#[test]
fn kill_and_close_take_a_host_when_names_collide() {
    let c = setup("offline-kill");
    remote_spawn(&c);
    ev(
        &c,
        NOW - 500,
        "supersede",
        json!({"role": "impl", "agent": "sendmeter-impl-12"}),
    );
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "pane": "w1:p1", "workspace": "w1",
                   "worktree": "/x", "branch": "issue-12", "host": "mini"});
    ev(&c, NOW - 400, "spawn", d);
    let r = Fake::default()
        .reply(
            "herdr agent list",
            &agents(&[("sendmeter-impl-12", "working", "/x", "w1:p1")]),
        )
        .once(
            "herdr --machine bazzite agent list",
            0,
            &agents(&[("sendmeter-impl-12", "idle", WT, "w9:p1")]),
        )
        .reply("herdr --machine bazzite agent list", NO_AGENTS)
        .reply("herdr --machine bazzite pane close w9:p1", "{}");
    let (code, out) = run(&c, &r, &["close", "sendmeter-impl-12"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("IS_LANE"), "{out}");
    let (code, out) = run(&c, &r, &["close", "sendmeter-impl-12", "--host", "bazzite"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(r.count("herdr --machine bazzite pane close w9:p1"), 1);
    let (code, out) = run(&c, &r, &["kill", "sendmeter-impl-12", "--host", "bazzite", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(r.count("herdr --machine bazzite pane close w9:p1"), 2);
    assert!(!r.called("herdr pane close"), "{:?}", r.calls.borrow());
    assert_eq!(kinds(&c, "stale-pane-killed").len(), 1);
    assert!(kinds(&c, "lane-killed").is_empty(), "mini's lane lives on");
    let (code, out) = run(&c, &r, &["kill", "sendmeter-impl-12", "--host", "bazzite", "--yes"]);
    assert_eq!(code, 3, "killed once: {out}");
    assert!(out.contains("NO_SUCH_AGENT"), "{out}");
}

#[test]
fn another_hosts_agent_list_is_cut_off_at_its_limit() {
    use herdsman::runner::{argv, limit, Real};
    use std::time::{Duration, Instant};
    let list = argv(&["--machine", "air", "agent", "list"]);
    assert_eq!(limit("herdr", &list), Duration::from_secs(20));
    assert_eq!(
        limit("herdr", &argv(&["agent", "list"])),
        Duration::from_mins(1),
        "#713: this host's own herdr gets the default"
    );
    assert_eq!(
        limit("herdr", &argv(&["--machine", "air", "agent", "start"])),
        Duration::from_mins(1),
        "only a read is cut short"
    );
    let t = Instant::now();
    let o = Real
        .within("sh", &argv(&["-c", "sleep 30"]), b"", Duration::from_millis(300))
        .unwrap();
    assert_eq!(o.code, 124, "{o:?}");
    assert!(o.stderr.contains("no answer"), "{}", o.stderr);
    assert!(
        t.elapsed() < Duration::from_secs(5),
        "killed at its limit, not after the sleep"
    );
    let o = Real
        .within("sh", &argv(&["-c", "echo up"]), b"", Duration::from_secs(5))
        .unwrap();
    assert_eq!((o.code, o.stdout.trim()), (0, "up"), "an answer in time is kept");
}

#[test]
fn a_host_offline_15_min_gets_its_lanes_restarted_on_the_main_host() {
    let c = setup("offline-move");
    let st = c.state.parent().unwrap();
    let note = st.join("notes.txt");
    let toml = std::fs::read_to_string(&c.config).unwrap();
    let sink = format!(
        "\n[[notify]]\ncmd = \"cat >> '{}'\"\non = [\"lanes-moved\"]\n",
        note.display()
    );
    std::fs::write(&c.config, toml + &sink).unwrap();
    let hosts = |up: bool| {
        let cache = json!({
            "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
            "bazzite": {"tags": ["os:linux"], "home": "/home/me", "probed": NOW, "seen": if up { NOW } else { NOW - 901 }, "up": up},
        });
        std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    };
    hosts(false);
    remote_spawn(&c);
    let d = json!({"role": "impl", "agent": "sendmeter-impl-13", "harness": "claude", "pane": "w9:p2", "workspace": "w9",
                   "worktree": "/home/me/.herdr/worktrees/sendmeter/issue-13", "branch": "issue-13", "host": "bazzite", "machine": "bazzite"});
    append(&c.state, &Event::new(NOW - 800, "sendmeter", Some(13), "spawn", d)).unwrap();
    let local = "git -C /tmp/does-not-matter/sendmeter";
    let fail = |p: String| {
        (
            p,
            herdsman::runner::Out {
                code: 1,
                ..Default::default()
            },
        )
    };
    let mut r = mini_only();
    r.replies.insert(0, fail(format!("{local} fetch origin issue-13")));
    r.replies
        .insert(0, fail(format!("{local} rev-parse --verify --quiet origin/issue-13")));
    let sha = "1234567890abcdef1234567890abcdef12345678";
    r.replies.insert(
        0,
        (
            format!("{local} rev-parse --verify --quiet origin/issue-12"),
            herdsman::runner::Out {
                stdout: sha.into(),
                ..Default::default()
            },
        ),
    );

    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    for n in [12, 13] {
        let sp = read(&c.state)
            .unwrap()
            .to_vec()
            .into_iter()
            .rfind(|e| e.is("spawn", "sendmeter", n))
            .unwrap();
        assert_eq!(
            (sp.str("host"), sp.str("role")),
            (Some("mini"), Some("impl")),
            "#{n}: {out}"
        );
        assert!(
            r.called(&format!("herdr agent start sendmeter-impl-{n}")),
            "{:?}",
            r.calls.borrow()
        );
    }
    let creates: Vec<String> = r
        .calls
        .borrow()
        .iter()
        .filter(|c| c.starts_with("herdr worktree create"))
        .cloned()
        .collect();
    assert!(
        creates
            .iter()
            .any(|c| c.contains("--branch issue-12 --base origin/issue-12")),
        "{creates:?}"
    );
    assert!(
        creates
            .iter()
            .any(|c| c.contains("--branch issue-13 --base origin/staging")),
        "{creates:?}"
    );
    let moved = kinds(&c, "lane-moved");
    let m = |n: u64| moved.iter().find(|e| e.issue == Some(n)).unwrap();
    assert_eq!(moved.len(), 2, "{moved:?}");
    assert_eq!(
        (m(12).str("host"), m(12).str("agent"), m(12).str("sha")),
        (Some("bazzite"), Some("sendmeter-impl-12"), Some(sha))
    );
    assert_eq!(
        (m(13).str("host"), m(13).str("agent"), m(13).str("sha")),
        (Some("bazzite"), Some("sendmeter-impl-13"), None)
    );
    let posted = std::fs::read_to_string(&note).unwrap_or_default();
    assert_eq!(posted.lines().count(), 1, "one line: {posted:?}");
    assert!(
        posted.contains("sendmeter-impl-12") && posted.contains("sendmeter-impl-13"),
        "{posted}"
    );
    assert!(posted.contains("unpushed work stayed on bazzite"), "{posted}");
    let (_, status) = run(&c, &r, &["status"]);
    assert!(!status.contains("host offline"), "the old lanes are gone: {status}");

    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    hosts(true);
    let up = mini_only().reply("herdr --machine bazzite agent list", NO_AGENTS);
    let (code, out) = run(&c, &up, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(kinds(&c, "spawn").len(), 4, "{out}");
    assert_eq!(kinds(&c, "lane-moved").len(), 2);
    assert_eq!(std::fs::read_to_string(&note).unwrap().lines().count(), 1);
    assert!(
        !up.called("herdr --machine bazzite agent start"),
        "{:?}",
        up.calls.borrow()
    );
}
