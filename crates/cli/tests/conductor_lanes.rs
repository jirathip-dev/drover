//! #422: the Lanes tab shows each thread's conductor as a known lane of the thread's repo (role `conductor`), first in
//! its subgroup, named by the thread, `asleep` (dimmed) once its pane closed; an `all` thread's in `all repos`; a tap
//! opens the thread; Others keeps only agents herdsman did not start.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use herdsman::serve::Server;
use serde_json::{json, Value};

const T: u64 = 1_000_000;

/// The log: impl #5 (newer than the conductors), threads t6 (awake), t7 (`all`, awake) and t8 (asleep), plus t9
/// that never started a conductor. herdr runs the impl, both awake conductors and two agents herdsman never started.
fn setup(name: &str) -> (herdsman::Ctx, Value) {
    let cfg = format!(
        "{CONFIG}\n[fleet.herdsman]\nrepo = \"o/herdsman\"\nlocal = \"/p/herdsman\"\nintegration = \"staging\"\n"
    );
    let c = ctx(&tmpdir(name), &cfg);
    let ev = |ts: u64, fleet: &str, issue: Option<u64>, kind: &str, d: Value| {
        append(&c.state, &Event::new(ts, fleet, issue, kind, d)).unwrap()
    };
    let spawn = json!({"role": "impl", "agent": "herdsman-impl-5", "harness": "claude", "model": "sonnet"});
    ev(900_500, "herdsman", Some(5), "spawn", spawn);
    for (id, fleet, title, ts) in [
        ("t6", "herdsman", "Conductor-run design lanes", 900_200),
        ("t7", "all", "Every repo", 900_100),
        ("t8", "herdsman", "Old chat", 900_000),
    ] {
        ev(ts, fleet, None, "thread-new", json!({"thread": id, "title": title}));
        let d = json!({"thread": id, "agent": format!("conductor-{id}"), "pane": "w9:p1", "harness": "claude", "model": "opus"});
        ev(ts, fleet, None, "conductor-start", d);
    }
    ev(
        900_300,
        "herdsman",
        None,
        "conductor-sleep",
        json!({"thread": "t8", "agent": "conductor-t8", "why": "idle"}),
    );
    ev(
        900_300,
        "herdsman",
        None,
        "thread-new",
        json!({"thread": "t9", "title": "Never started"}),
    );
    let agents = json!([
        {"name": "herdsman-impl-5", "agent_status": "idle", "pane_id": "w1:p1", "cwd": "/x"},
        {"name": "conductor-t6", "agent_status": "idle", "pane_id": "w6:p1", "cwd": "/x"},
        {"name": "conductor-t7", "agent_status": "working", "pane_id": "w7:p1", "cwd": "/x"},
        {"name": "conductor-t99", "agent_status": "idle", "pane_id": "w8:p1", "cwd": "/x"},
        {"agent": "codex", "agent_status": "idle", "pane_id": "w4:p1", "cwd": "/Users/me/side", "workspace_id": "w4"},
    ]);
    (c, agents)
}

#[test]
fn conductors_are_lanes_of_their_repo_first_in_its_group_and_asleep_once_closed() {
    let (c, agents) = setup("conductor-lanes");
    let cfg = herdsman::config::Config::load(&c.config).unwrap();
    let events = read(&c.state).unwrap();
    let st = herdsman::events::fold(&events);
    let lanes = herdsman::lanes::join(
        &cfg,
        &Fake::default(),
        &c.worktrees,
        &events,
        &st,
        agents.as_array().unwrap(),
        &Default::default(),
    );
    let row = |l: &herdsman::lanes::Lane| {
        let s = |v: &Option<String>| v.clone().unwrap_or_default();
        (
            s(&l.fleet),
            l.name.clone(),
            l.role.clone(),
            l.state.clone(),
            s(&l.model),
            s(&l.thread),
            l.issue,
        )
    };
    let got: Vec<_> = lanes.iter().map(row).collect();
    let r = |f: &str, n: &str, role: &str, st: &str, m: &str, t: &str, i| {
        (f.into(), n.into(), role.into(), st.into(), m.into(), t.into(), i)
    };
    assert_eq!(
        got,
        [
            r("herdsman", "herdsman-impl-5", "impl", "idle", "sonnet", "", Some(5)),
            r(
                "herdsman",
                "Conductor-run design lanes · t6",
                "conductor",
                "idle",
                "opus",
                "t6",
                None
            ),
            r("all", "Every repo · t7", "conductor", "working", "opus", "t7", None),
            // A `conductor-<id>` name with no such thread in the log is no lane of herdsman's.
            r("", "conductor-t99", "other", "idle", "", "", None),
            r("", "w4", "other", "idle", "", "", None),
            r("herdsman", "Old chat · t8", "conductor", "asleep", "opus", "t8", None),
        ]
    );
    // Grouped: the herdsman subgroup opens with its conductors (newest first), then the impl that changed later.
    let groups = herdsman::lanes::group(&lanes, |l| l.since);
    let names = |sec: &str, f: &str| -> Vec<String> {
        let subs = &groups.iter().find(|g| g.0 == sec).unwrap().1;
        subs.iter()
            .find(|s| s.0 == f)
            .unwrap()
            .1
            .iter()
            .map(|l| l.name.clone())
            .collect()
    };
    let idle: Vec<&str> = groups
        .iter()
        .find(|g| g.0 == "idle")
        .unwrap()
        .1
        .iter()
        .map(|s| s.0.as_str())
        .collect();
    assert_eq!(idle, ["herdsman", "Others"]);
    assert_eq!(
        names("idle", "herdsman"),
        ["Conductor-run design lanes · t6", "Old chat · t8", "herdsman-impl-5"]
    );
    assert_eq!(names("working", "all repos"), ["Every repo · t7"]);
    assert_eq!(names("idle", "Others"), ["conductor-t99", "w4"]);
}

/// The Lanes tab's HTML: each conductor row carries its thread (the tap target), an asleep one is gone (dimmed).
#[test]
fn the_lanes_tab_renders_conductor_rows_with_their_thread() {
    let (c, agents) = setup("conductor-lanes-page");
    let list = json!({"result": {"agents": agents}}).to_string();
    let r = Fake::default().reply("herdr agent list", &list);
    let s = Server::new(&c, &r, None);
    let page = with_queue(
        &String::from_utf8_lossy(&s.respond("GET /?all=1 HTTP/1.1\r\n\r\n", T)),
        &s,
        "",
        T,
    );
    let at = |s: &str| page.find(s).unwrap_or_else(|| panic!("{s} missing"));
    let idle = &page[at("<h2><span class=a>idle (")..];
    let herd = &idle[idle.find("<h3>herdsman (3)</h3>").unwrap()..];
    assert!(
        herd.starts_with("<h3>herdsman (3)</h3><div class=lane data-agent=\"Conductor-run design lanes · t6\" data-role=conductor data-thread=t6>"),
        "{herd}"
    );
    assert!(page.contains("<div class=lane data-agent=\"Old chat · t8\" data-role=conductor data-gone data-thread=t8>"));
    assert!(page.contains("<span class=\"nw m\">asleep"), "asleep reads muted");
    assert!(page.contains(
        "<h3>all repos (1)</h3><div class=lane data-agent=\"Every repo · t7\" data-role=conductor data-thread=t7>"
    ));
    let others = &page[at("<h3>Others (2)</h3>")..];
    let others = &others[..others.find("<h2>").unwrap_or(others.len())];
    assert!(
        !others.contains("data-thread") && others.contains("conductor-t99"),
        "{others}"
    );
    let css = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/serve.css")).unwrap();
    assert!(css.contains(".lane[data-thread][data-gone]>.lr{opacity:.55}"));
}

/// In WebKit at 390 px: a tap on the asleep conductor's row opens its thread in the chat tab (no lane sheet).
#[test]
fn a_tap_on_a_conductor_row_opens_its_thread_in_the_chat_tab() {
    if !(cfg!(target_os = "macos") && std::process::Command::new("swift").arg("--version").output().is_ok()) {
        eprintln!("skipping: no swift/WebKit here");
        return;
    }
    let (c, agents) = setup("conductor-lanes-tap");
    let list = json!({"result": {"agents": agents}}).to_string();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let at = addr.clone();
    std::thread::spawn(move || {
        let r = Fake::default().reply("herdr agent list", &list);
        let s = Server::new(&c, &r, Some(at));
        for mut st in l.incoming().flatten() {
            let mut buf = [0u8; 8192];
            let n = std::io::Read::read(&mut st, &mut buf).unwrap_or(0);
            let _ = std::io::Write::write_all(&mut st, &s.respond(&String::from_utf8_lossy(&buf[..n]), T));
        }
    });
    let js = "$('.lane[data-thread=\"t8\"] .lr').click(); \
        const v = $('#chat .cvp[data-t=\"t8\"]'); \
        window.late = [+(document.documentElement.dataset.tab === 'chat'), +!$('#chat').hidden, +!!v, \
          +(v && getComputedStyle(v).display !== 'none'), +(THR === 't8'), +!!(history.state && history.state.thr === 't8'), \
          +!$('#sheet.on'), errs.length];";
    let png = tmpdir("conductor-lanes-png").join("tap.png");
    let o = swift()
        .args([
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/webview.swift"),
            &format!("http://{addr}/?all=1"),
            "390",
            "lanes",
            "scene",
            png.to_str().unwrap(),
            js,
        ])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{out} {}", String::from_utf8_lossy(&o.stderr));
    let m: Vec<f64> = out.split_whitespace().skip(2).filter_map(|n| n.parse().ok()).collect();
    // chat tab, shown, t8's thread, visible, open, a history entry, no lane sheet, page errors.
    assert_eq!(m, [1., 1., 1., 1., 1., 1., 1., 0.], "{out}");
}
