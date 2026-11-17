use super::*;

fn board_json(c: &drover::Ctx, r: &'static dyn Runner, path: &str) -> Value {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let sc = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&sc, r, l, false));
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: {addr}\r\n\r\n").unwrap();
    let mut body = String::new();
    std::io::Read::read_to_string(&mut s, &mut body).unwrap();
    serde_json::from_str(body.split_once("\r\n\r\n").expect(&body).1).expect(&body)
}

fn api_lane(v: &Value, name: &str) -> Value {
    let all = v["lanes"].as_array().unwrap_or_else(|| panic!("{v}"));
    all.iter()
        .find(|l| l["name"] == name)
        .cloned()
        .unwrap_or_else(|| panic!("{v}"))
}

#[test]
fn each_shed_state_reaches_api_v1_and_the_plugin_unchanged() {
    let b = FakeShed::new();
    let c = ctx(&tmpdir("acp-states"), &acp_config(&b.sock, "claude"));
    let states = [
        "working",
        "running",
        "waiting-approval",
        "rate-limited",
        "done",
        "error",
        "hibernated",
    ];
    for (i, s) in (1u64..).zip(states) {
        let name = format!("sendmeter-impl-{i}");
        b.add(
            &name,
            json!({"fleet": "sendmeter", "issue": i.to_string(), "role": "impl", "harness": "claude"}),
        );
        b.set(&name, |t| t["reported"] = json!({"state": s, "note": format!("n{i}")}));
        let d = json!({"role": "impl", "agent": name, "harness": "claude", "runtime": "shed", "pane": name,
            "worktree": format!("/w/issue-{i}")});
        drover::events::append(&c.state, &Event::new(999_000, "sendmeter", Some(i), "spawn", d)).unwrap();
    }
    let r: &'static Fake = Box::leak(Box::new(Fake::default().reply("herdr agent list", NO_AGENTS)));
    let v = board_json(&c, r, "/api/v1/state");
    for (i, s) in (1u64..).zip(states) {
        let l = api_lane(&v, &format!("sendmeter-impl-{i}"));
        assert_eq!(
            (l["state"].as_str(), l["note"].as_str()),
            (Some(s), Some(&*format!("n{i}"))),
            "{v}"
        );
    }
    let counts = json!({"busy": 2, "waiting_approval": 1, "waiting_owner": 0, "rate_limited": 1});
    assert_eq!(v["counts"], counts, "{v}");
    let q = board_json(&c, r, "/screens/queue?more=sendmeter").to_string();
    for (i, s) in (1u64..).zip(states) {
        let shown = if s == "rate-limited" {
            format!("{s} · resets n{i} · ")
        } else {
            format!("▸ {s} · n{i} · ")
        };
        assert!(q.contains(&shown), "{shown} on the queue screen: {q}");
    }
    for (label, n) in [("busy", 2), ("approval", 1), ("owner", 0), ("rate-limited", 1)] {
        assert!(
            q.contains(&format!(r#"{{"label":"{label}","value":"{n}"}}"#)),
            "{label}: {q}"
        );
    }
    assert!(q.contains(r#""action":"limit""#), "the rate-limited row's picks: {q}");
}

#[test]
fn a_lane_ending_its_turn_on_an_ask_waits_on_the_owner_until_it_is_answered() {
    let (c, b, f, wt, _) = acp_lane("acp-waiting-owner");
    let cli = Push(b.cli(&f), wt.clone());
    let (code, out) = run(&later(&c, 30), &cli, &["ask", "sendmeter", "12", "Which way?"]);
    assert_eq!(code, 0, "{out}");
    session(&b, &[turn()]);
    drive_at(&c, &cli, 60);
    b.set("sendmeter-impl-12", |t| {
        t["reported"] = json!({"state": "done", "note": "end_turn"});
    });
    let r: &'static Fake = Box::leak(Box::new(Fake::default().reply("herdr agent list", NO_AGENTS)));
    let state = |c: &drover::Ctx| api_lane(&board_json(c, r, "/api/v1/state"), "sendmeter-impl-12")["state"].clone();
    assert_eq!(state(&later(&c, 80)), "waiting-owner");
    let id = logged(&c, "needs-decision")[0].str("id").unwrap().to_string();
    assert_eq!(run(&later(&c, 90), &cli, &["answer", "sendmeter", &id, "1"]).0, 0);
    assert_eq!(state(&later(&c, 100)), "done");
}

#[test]
fn a_pending_tool_call_in_a_live_session_is_not_silent() {
    let (c, b, f, wt, _) = acp_lane("acp-tool");
    mid_turn(&b);
    session(&b, &[turn(), tool("in_progress"), cpu(80)]);
    let cli = Push(b.cli(&f), wt.clone());
    drive_at(&c, &cli, 60 + 3 * SILENT);
    assert!(!f.called("shed cancel"));
    assert!(logged(&c, "nudged").is_empty());
    session(&b, &[tool("completed")]);
    drive_at(&c, &cli, 60 + 3 * SILENT + 1);
    let out = drive_at(&c, &cli, 60 + 4 * SILENT + 1);
    assert!(f.called("shed cancel sendmeter-impl-12"), "{out}");
}

#[test]
fn a_running_lane_is_never_nudged() {
    let (c, b, f, wt, _) = acp_lane("acp-running");
    b.set("sendmeter-impl-12", |t| {
        t["reported"] = json!({"state": "running", "note": "cargo test"});
    });
    session(&b, &[turn(), tool("in_progress"), cpu(0)]);
    let cli = Push(b.cli(&f), wt.clone());
    drive_at(&c, &cli, 60);
    let out = drive_at(&c, &cli, 60 + 3 * SILENT);
    assert!(!f.called("shed cancel"), "{out}");
    assert!(logged(&c, "nudged").is_empty(), "{out}");
    assert!(logged(&c, "respawned-silent").is_empty(), "{out}");
}

fn rate_limited(b: &FakeShed) {
    b.set("sendmeter-impl-12", |t| {
        t["reported"] = json!({"state": "rate-limited", "note": "5pm"});
    });
}

#[test]
fn wait_sends_continue_at_the_reset_only_on_the_owners_pick() {
    let (c, b, f, wt, _) = acp_lane("acp-limit-wait");
    let cli = Push(b.cli(&f), wt.clone());
    let (code, out) = run(&later(&c, 60), &cli, &["limit", "sendmeter", "12"]);
    assert_eq!(
        (code, out.contains("NOT_RATE_LIMITED")),
        (3, true),
        "a working lane has no limit to wait out: {out}"
    );
    rate_limited(&b);
    let out = drive_at(&c, &cli, 2 * 86_400);
    assert_eq!(
        f.count("shed send sendmeter-impl-12 continue"),
        0,
        "no pick, no continue: {out}"
    );
    assert_eq!(logged(&c, "spawn").len(), 1, "no pick, no switch: {out}");
    let (code, out) = run(&later(&c, 2 * 86_400), &cli, &["limit", "sendmeter", "12"]);
    assert_eq!(code, 0, "{out}");
    let p = &logged(&c, "limit-picked")[0];
    assert_eq!(
        (p.str("pick"), p.str("by"), p.str("reset")),
        (Some("wait"), Some("owner"), Some("5pm"))
    );
    let at = p.val("at").as_u64().unwrap();
    assert!(at > c.now + 2 * 86_400, "{p:?}");
    drive_at(&c, &cli, at - c.now - 1);
    assert_eq!(
        f.count("shed send sendmeter-impl-12 continue"),
        0,
        "not before the reset"
    );
    drive_at(&c, &cli, at - c.now);
    drive_at(&c, &cli, at - c.now + 60);
    assert_eq!(f.count("shed send sendmeter-impl-12 continue"), 1, "once at the reset");
    assert_eq!(logged(&c, "spawn").len(), 1);
}
