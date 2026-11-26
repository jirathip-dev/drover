mod common;
use common::*;
use drover::config::Config;
use drover::events::{append, read, Event};
use serde_json::json;

const NOW: u64 = 1_000_000;
const WT: &str = "/home/me/.herdr/worktrees/sendmeter/issue-12";

fn setup(name: &str) -> (drover::Ctx, FakeShed) {
    let base = CONFIG.replace("harness  =  \"claude\"", "harness = \"claude\"");
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
    let sh = FakeShed::new();
    sh.attach(&c);
    remote::proxied(&c);
    (c, sh)
}

fn ev(c: &drover::Ctx, ts: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

fn remote_spawn(c: &drover::Ctx) {
    let d = json!({"role": "impl", "agent": IMPL, "harness": "claude", "pane": IMPL, "workspace": "",
                   "worktree": WT, "branch": "issue-12", "host": "bazzite", "machine": "bazzite"});
    ev(c, NOW - 900, "spawn", d);
}

fn mini_only() -> Fake {
    Fake::default()
        .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }")
        .reply("sysctl -n hw.ncpu", "4")
        .reply("gh issue view", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply("git ", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

fn with_bazzite<'a>(f: &'a Fake, bazzite: &'a FakeShed) -> remote::Acp<'a, OverSsh<'a>> {
    remote::Acp::over(OverSsh {
        f,
        hosts: vec![("bazzite", bazzite)],
    })
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

#[test]
fn an_unreachable_host_refuses_only_its_own_spawns() {
    let (c, sh) = setup("offline-spawn");
    let r = mini_only();
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "13", "impl", "--host", "bazzite"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED HOST_OFFLINE: bazzite: unreachable"), "{out}");
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "14", "impl", "--host", "mini"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    assert!(sh.has("sendmeter-impl-14"), "{:?}", sh.methods());
    remote_spawn(&c);
    ev(&c, NOW - 100, "verdict", json!({"verdict": "FAIL", "sha": "abc"}));
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "rev"]);
    assert_eq!(code, 0, "{out}");
    assert!(sh.has("sendmeter-rev-12-r2"), "{:?}", sh.methods());
}

#[test]
fn a_logged_lane_on_the_down_host_still_blocks_a_duplicate() {
    let (c, sh) = setup("offline-dup");
    remote_spawn(&c);
    let r = mini_only();
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl", "--host", "mini"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED LANE_LIVE: unfinished impl lane"), "{out}");
    assert!(sh.lock().terms.is_empty(), "{:?}", sh.methods());
}

#[test]
fn its_lanes_show_host_offline_alert_once_and_recover() {
    let (c, _sh) = setup("offline-show");
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

    let (f, bazzite) = (mini_only(), FakeShed::new());
    bazzite.lane(IMPL, "working", WT);
    let up = with_bazzite(&f, &bazzite);
    for _ in 0..2 {
        assert_eq!(run(&c, &up, &["check"]).0, 0);
    }
    assert_eq!(kinds(&c, "host-online").len(), 1);
    let (_, status) = run(&c, &up, &["status"]);
    assert!(!status.contains("host offline"), "{status}");
    let (_, board) = run(&c, &up, &["board", "sendmeter"]);
    assert!(!board.contains("host offline"), "{board}");
    look(&c, &down);
    assert_eq!(run(&c, &down, &["check"]).0, 0);
    assert_eq!(kinds(&c, "host-offline").len(), 2);
}

#[test]
fn a_pane_a_supersede_left_on_a_down_host_is_killed_once_it_is_back() {
    let (c, sh) = setup("offline-supersede");
    remote_spawn(&c);
    let r = mini_only();
    let args = ["spawn", "sendmeter", "12", "impl", "--supersede", "--host", "mini"];
    let (code, out) = run(&c, &r, &args);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    let s = kinds(&c, "supersede").pop().unwrap();
    assert_eq!(
        (s.str("stale_host"), s.str("stale_pane")),
        (Some("bazzite"), Some(IMPL))
    );
    let cfg = Config::load(&c.config).unwrap();
    drover::check::stale(&c, &cfg, &sh.on(&r), &mut Vec::new()).unwrap();
    assert!(kinds(&c, "stale-pane-killed").is_empty());
    sh.lane(IMPL, "working", "/x");
    let (f, bazzite) = (Fake::default(), FakeShed::new());
    bazzite.lane(IMPL, "idle", WT);
    let back = with_bazzite(&f, &bazzite);
    look(&c, &sh.on(&back));
    for _ in 0..2 {
        drover::check::stale(&c, &cfg, &sh.on(&back), &mut Vec::new()).unwrap();
    }
    assert_eq!(bazzite.all("kill").len(), 1, "{:?}", bazzite.methods());
    assert!(!bazzite.has(IMPL), "the old pane is gone");
    assert!(!sh.called("kill", IMPL), "the new lane is untouched");
    let k = kinds(&c, "stale-pane-killed");
    assert_eq!(
        (k.len(), k[0].str("host"), k[0].str("pane")),
        (1, Some("bazzite"), Some(IMPL))
    );
    assert!(kinds(&c, "lane-killed").is_empty());
    let (_, status) = run(&c, &back, &["status"]);
    assert!(
        status.contains("impl") && status.contains("sendmeter-impl-12"),
        "{status}"
    );
}

#[test]
fn kill_and_close_take_a_host_when_names_collide() {
    let (c, sh) = setup("offline-kill");
    remote_spawn(&c);
    ev(
        &c,
        NOW - 500,
        "supersede",
        json!({"role": "impl", "agent": "sendmeter-impl-12"}),
    );
    let d = json!({"role": "impl", "agent": IMPL, "harness": "claude", "pane": IMPL, "workspace": "",
                   "worktree": "/x", "branch": "issue-12", "host": "mini"});
    ev(&c, NOW - 400, "spawn", d);
    sh.lane(IMPL, "working", "/x");
    let (f, bazzite) = (Fake::default(), FakeShed::new());
    bazzite.lane(IMPL, "idle", WT);
    let r = with_bazzite(&f, &bazzite);
    let (code, out) = run(&c, &r, &["close", "sendmeter-impl-12"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("IS_LANE"), "{out}");
    let (code, out) = run(&c, &r, &["close", "sendmeter-impl-12", "--host", "bazzite"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(bazzite.all("kill").len(), 1, "{:?}", bazzite.methods());
    let (code, out) = run(&c, &r, &["kill", "sendmeter-impl-12", "--host", "bazzite", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(bazzite.all("kill").len(), 2, "{:?}", bazzite.methods());
    assert!(!sh.called("kill", IMPL), "{:?}", sh.methods());
    assert_eq!(kinds(&c, "stale-pane-killed").len(), 1);
    assert!(kinds(&c, "lane-killed").is_empty(), "mini's lane lives on");
    let (code, out) = run(&c, &r, &["kill", "sendmeter-impl-12", "--host", "bazzite", "--yes"]);
    assert_eq!(code, 3, "killed once: {out}");
    assert!(out.contains("NO_SUCH_AGENT"), "{out}");
}

#[test]
fn a_call_is_killed_at_its_limit() {
    use drover::runner::{argv, Real};
    use std::time::{Duration, Instant};
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
    let (c, sh) = setup("offline-move");
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
    let row = drover::link::view::HostSeen {
        host: "bazzite".into(),
        link: "down".into(),
        since: NOW - 3600,
        heard: Some(NOW - 901),
        ..Default::default()
    };
    drover::db::view_put(&c.state, &row, None).unwrap();
    remote_spawn(&c);
    let d = json!({"role": "impl", "agent": "sendmeter-impl-13", "harness": "claude", "pane": "sendmeter-impl-13", "workspace": "",
                   "worktree": "/home/me/.herdr/worktrees/sendmeter/issue-13", "branch": "issue-13", "host": "bazzite", "machine": "bazzite"});
    append(&c.state, &Event::new(NOW - 800, "sendmeter", Some(13), "spawn", d)).unwrap();
    let local = "git -C /tmp/does-not-matter/sendmeter";
    let fail = |p: String| {
        (
            p,
            drover::runner::Out {
                code: 1,
                ..Default::default()
            },
        )
    };
    let mut r = mini_only();
    r.replies.insert(
        0,
        fail(format!(
            "{local} fetch origin +refs/heads/issue-13:refs/remotes/origin/issue-13"
        )),
    );
    for n in [12, 13] {
        r.replies
            .insert(0, fail(format!("{local} rev-parse --verify --quiet issue-{n}")));
    }
    r.replies
        .insert(0, fail(format!("{local} rev-parse --verify --quiet origin/issue-13")));
    let sha = "1234567890abcdef1234567890abcdef12345678";
    r.replies.insert(
        0,
        (
            format!("{local} rev-parse --verify --quiet origin/issue-12"),
            drover::runner::Out {
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
        assert!(sh.has(&format!("sendmeter-impl-{n}")), "{:?}", sh.methods());
    }
    let creates: Vec<String> = r
        .calls
        .borrow()
        .iter()
        .filter(|c| c.starts_with(&format!("{local} worktree add")))
        .cloned()
        .collect();
    assert!(
        creates
            .iter()
            .any(|c| c.contains(" -b issue-12 ") && c.ends_with(" origin/issue-12")),
        "{creates:?}"
    );
    assert!(
        creates
            .iter()
            .any(|c| c.contains(" -b issue-13 ") && c.ends_with(" origin/staging")),
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
    let (f, bazzite) = (mini_only(), FakeShed::new());
    let up = with_bazzite(&f, &bazzite);
    let (code, out) = run(&c, &up, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(kinds(&c, "spawn").len(), 4, "{out}");
    assert_eq!(kinds(&c, "lane-moved").len(), 2);
    assert_eq!(std::fs::read_to_string(&note).unwrap().lines().count(), 1);
    assert!(bazzite.lock().terms.is_empty(), "{:?}", bazzite.methods());
}

const SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m bazzite --";

fn remote(mut f: Fake) -> Fake {
    let fresh = format!("{SSH} 'sh' '-c' 'test -e \"$1\"' 'sh' '/home/me/.local/share/drover/work/sendmeter/issue-13'");
    let no = drover::runner::Out {
        code: 1,
        ..Default::default()
    };
    f.replies.push((fresh, no));
    f.reply(&format!("{SSH} 'sh' '-c' 'test -e"), "")
        .reply(&format!("{SSH} 'git'"), "")
        .reply(&format!("{SSH} '/opt/drover/bin/drover' 'profiles' 'sync'"), "")
        .reply(&format!("{SSH} 'sh' '-c' 'mkdir"), "")
        .reply(&format!("{SSH} 'sh' '-c' 'umask 077"), "")
        .reply(&format!("{SSH} 'rm' '-f'"), "")
}

#[test]
fn a_host_an_earlier_pass_found_down_takes_its_start_when_it_answers_this_pass() {
    let (c, _sh) = setup("offline-earlier-pass");
    let q = ["queue", "sendmeter", "add", "13", "--needs", "os:linux"];
    assert_eq!(run(&c, &Fake::default(), &q).0, 0);
    let down = c.state.with_file_name("hosts.down");
    std::fs::write(&down, json!({"bazzite": [NOW - 600, NOW - 10]}).to_string()).unwrap();
    let (f, bazzite) = (remote(mini_only()), FakeShed::new());
    let (code, out) = run(&c, &with_bazzite(&f, &bazzite), &["drive"]);
    assert_eq!(code, 0, "{out}");
    let pass = kinds(&c, "drive-pass").into_iter().last().unwrap();
    assert!(!pass.val("actions").to_string().contains("HOST_OFFLINE"), "{out}");
    assert!(bazzite.has("sendmeter-impl-13"), "{out} {:?}", bazzite.methods());
}

#[test]
fn a_start_on_a_host_offline_this_pass_waits_for_it_with_no_card_or_backoff() {
    let (c, _sh) = setup("offline-start");
    let q = ["queue", "sendmeter", "add", "13", "--needs", "os:linux"];
    assert_eq!(run(&c, &Fake::default(), &q).0, 0);
    let ssh = SSH;
    let at = |k: u64| drover::Ctx {
        now: NOW + k,
        ..c.clone()
    };
    let why = |c: &drover::Ctx| {
        let pass = kinds(c, "drive-pass").into_iter().last().unwrap();
        let acts = pass.val("actions").as_array().unwrap().clone();
        let a = acts.into_iter().find(|a| a["issue"] == 13).unwrap();
        a["why"].as_str().unwrap_or_default().to_string()
    };
    for k in [0, 240] {
        let (mut f, bazzite) = (remote(mini_only()), FakeShed::new());
        f.replies.insert(0, (ssh.into(), timed_out()));
        let (code, out) = run(&at(k), &with_bazzite(&f, &bazzite), &["drive"]);
        assert_eq!(code, 0, "{out}");
        assert!(
            why(&c).contains("bazzite: not sent (host offline this pass)"),
            "{}",
            why(&c)
        );
        assert!(bazzite.lock().terms.is_empty(), "{:?}", bazzite.methods());
        assert_eq!(run(&at(k + 60), &mini_only(), &["check"]).0, 0);
    }
    let mine: Vec<String> = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.issue == Some(13))
        .map(|e| e.kind)
        .collect();
    for k in ["spawn-error", "needs-human", "stuck", "spawn-failing", "needs-decision"] {
        assert!(!mine.iter().any(|m| m == k), "{k}: {mine:?}");
    }
    assert!(kinds(&c, "stuck").is_empty(), "{:?}", kinds(&c, "stuck"));
    let st = &drover::events::fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 13)];
    assert_eq!(st.state, "queued");

    let denied = remote(mini_only()).once_err(&format!("{ssh} 'sh' '-c' 'mkdir"), 1, "Permission denied");
    let bazzite = FakeShed::new();
    assert_eq!(run(&at(450), &with_bazzite(&denied, &bazzite), &["drive"]).0, 0);
    let errs = kinds(&c, "spawn-error");
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].str("error").unwrap().contains("Permission denied"), "{errs:?}");

    let (f, bazzite) = (remote(mini_only()), FakeShed::new());
    let (code, out) = run(&at(500), &with_bazzite(&f, &bazzite), &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(bazzite.has("sendmeter-impl-13"), "{out} {:?}", bazzite.methods());
}

const PROBE: &str = "ssh -oBatchMode=yes -oConnectTimeout=5 -oControlPath=none bazzite -- true";

fn pass_call(c: &drover::Ctx, r: &dyn drover::runner::Runner) -> drover::runner::Out {
    use drover::runner::Runner;
    let cfg = Config::load(&c.config).unwrap();
    let pass = drover::hosts::Pass::new(r, None).sparing(std::collections::BTreeSet::default(), Some((c, &cfg)));
    pass.run("ssh", &drover::runner::argv(&["bazzite", "--", "echo", "hi"]))
        .unwrap()
}

#[test]
fn one_slow_probe_is_retried_and_the_host_stays_up() {
    let (c, _sh) = setup("slow-probe-once");
    let f = mini_only()
        .once_err(PROBE, 124, "no answer")
        .reply(PROBE, "")
        .reply("ssh bazzite -- echo", "hi");
    let o = pass_call(&c, &f);
    assert_eq!((o.code, o.stdout.as_str()), (0, "hi"), "{o:?} {:?}", f.calls.borrow());
    assert_eq!(f.count(PROBE), 2, "{:?}", f.calls.borrow());
    assert!(!drover::hosts::down(&c).contains_key("bazzite"));
}

#[test]
fn two_slow_probes_in_a_row_mark_the_host_down() {
    let (c, _sh) = setup("slow-probe-twice");
    let f = mini_only()
        .once_err(PROBE, 124, "no answer")
        .once_err(PROBE, 124, "no answer")
        .reply(PROBE, "")
        .reply("ssh bazzite -- echo", "hi");
    let o = pass_call(&c, &f);
    assert_eq!(o.code, 255, "{o:?}");
    assert!(o.stderr.contains("host offline this pass"), "{o:?}");
    assert!(!f.called("ssh bazzite -- echo"), "{:?}", f.calls.borrow());
    assert!(drover::hosts::down(&c).contains_key("bazzite"));
}

#[test]
fn a_live_control_master_skips_the_fresh_handshake_probe() {
    let (c, _sh) = setup("probe-master");
    let f = mini_only()
        .reply("ssh -oControlPath=/tmp/drover-ssh-%C -O check bazzite", "")
        .reply(PROBE, "")
        .reply("ssh bazzite -- echo", "hi");
    let o = pass_call(&c, &f);
    assert_eq!(o.code, 0, "{o:?}");
    assert!(!f.called(PROBE), "{:?}", f.calls.borrow());
}

#[test]
fn after_a_retried_slow_probe_the_hosts_lanes_keep_their_state() {
    let (c, _sh) = setup("slow-probe-listing");
    remote_spawn(&c);
    let f = mini_only().once_err(PROBE, 124, "no answer").reply(PROBE, "");
    let bazzite = FakeShed::new();
    bazzite.lane(IMPL, "working", WT);
    let (code, out) = run(&c, &with_bazzite(&f, &bazzite), &["waiting", "--json"]);
    assert_eq!(code, 0, "{out}");
    let w: serde_json::Value = serde_json::from_str(&out).unwrap();
    let row = w.as_array().unwrap().iter().find(|x| x["issue"] == 12).unwrap().clone();
    let what = row.to_string();
    assert!(!what.contains("lane unknown"), "{what}");
    assert!(!what.contains("host offline"), "{what}");
    assert!(!bazzite.methods().is_empty(), "the listing was read");
    assert_eq!(f.count(PROBE), 2, "{:?}", f.calls.borrow());
}

const MUX: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m bazzite --";

fn timed_out() -> drover::runner::Out {
    drover::runner::Out {
        code: 124,
        ..Default::default()
    }
}

fn linux_start(f: Fake) -> Fake {
    let fresh = format!("{MUX} 'sh' '-c' 'test -e \"$1\"' 'sh' '/home/me/.local/share/drover/work/sendmeter/issue-13'");
    let mut f = f;
    f.replies.push((
        fresh,
        drover::runner::Out {
            code: 1,
            ..Default::default()
        },
    ));
    f.reply(&format!("{MUX} 'sh' '-c' 'test -e"), "")
        .reply(&format!("{MUX} 'git'"), "")
        .reply(&format!("{MUX} '/opt/drover/bin/drover' 'profiles' 'sync'"), "")
        .reply(&format!("{MUX} 'sh' '-c' 'mkdir"), "")
        .reply(&format!("{MUX} 'sh' '-c' 'umask 077"), "")
        .reply(&format!("{MUX} 'rm' '-f'"), "")
}

fn down() -> Fake {
    let mut f = linux_start(mini_only());
    f.replies.insert(0, (PROBE.into(), timed_out()));
    f.replies.insert(0, (MUX.into(), timed_out()));
    f
}

fn queue_linux(c: &drover::Ctx) {
    let q = ["queue", "sendmeter", "add", "13", "--needs", "os:linux"];
    assert_eq!(run(c, &Fake::default(), &q).0, 0);
}

fn last_code(c: &drover::Ctx) -> String {
    let pass = kinds(c, "drive-pass").into_iter().last().unwrap();
    let acts = pass.val("actions").as_array().unwrap().clone();
    let a = acts.into_iter().find(|a| a["issue"] == 13).unwrap();
    a["code"].as_str().unwrap_or_default().to_string()
}

#[test]
fn a_probe_that_times_out_once_then_answers_leaves_the_host_up_for_the_pass() {
    let (c, _sh) = setup("probe-retry");
    queue_linux(&c);
    let f = linux_start(mini_only()).once(PROBE, 124, "");
    let bazzite = FakeShed::new();
    let (code, out) = run(&c, &with_bazzite(&f, &bazzite), &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(bazzite.has("sendmeter-impl-13"), "{out} {:?}", bazzite.methods());
}

#[test]
fn a_host_that_fails_every_probe_is_offline_for_the_pass() {
    let (c, _sh) = setup("probe-down");
    queue_linux(&c);
    let (f, bazzite) = (down(), FakeShed::new());
    let (code, out) = run(&c, &with_bazzite(&f, &bazzite), &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(last_code(&c), "HOST_OFFLINE");
    assert!(bazzite.lock().terms.is_empty(), "{:?}", bazzite.methods());
}

#[test]
fn an_extended_round_held_only_by_its_offline_host_raises_no_card_and_starts_once_it_is_up() {
    let (c, _sh) = setup("offline-extend");
    queue_linux(&c);
    let ev13 = |ts: u64, kind: &str, d: serde_json::Value| {
        append(&c.state, &Event::new(ts, "sendmeter", Some(13), kind, d)).unwrap();
    };
    ev13(NOW - 3600, "round-extended", json!({"by": "owner", "queued": true}));
    let error = "bazzite: not sent (host offline this pass)";
    for ts in [NOW - 400, NOW - 100] {
        ev13(
            ts,
            "spawn-error",
            json!({"step": "start", "error": error, "head": null}),
        );
    }
    let (f, bazzite) = (down(), FakeShed::new());
    let (code, out) = run(&c, &with_bazzite(&f, &bazzite), &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(last_code(&c), "HOST_OFFLINE");
    assert!(bazzite.lock().terms.is_empty(), "{:?}", bazzite.methods());
    let cards: Vec<Event> = kinds(&c, "needs-human")
        .into_iter()
        .filter(|e| e.issue == Some(13))
        .collect();
    assert!(cards.is_empty(), "{cards:?}");
    assert!(drover::pending::requeued(&read(&c.state).unwrap(), "sendmeter", 13));

    let at = drover::Ctx {
        now: NOW + 60,
        ..c.clone()
    };
    let (f, bazzite) = (linux_start(mini_only()), FakeShed::new());
    let (code, out) = run(&at, &with_bazzite(&f, &bazzite), &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(bazzite.has("sendmeter-impl-13"), "{out} {:?}", bazzite.methods());
    assert!(kinds(&c, "needs-human").iter().all(|e| e.issue != Some(13)));
}

struct SlowShed<'a>(&'a dyn drover::runner::Runner, std::sync::atomic::AtomicUsize);

impl SlowShed<'_> {
    fn slow(&self, prog: &str, args: &[String]) -> Option<drover::runner::Out> {
        let line = format!("{prog} {}", args.join(" "));
        let listing = prog == "ssh" && line.contains("bazzite") && line.contains("'hosts' 'report'");
        let sc = std::sync::atomic::Ordering::SeqCst;
        let once = listing && self.1.try_update(sc, sc, |n| n.checked_sub(1)).is_ok();
        once.then(timed_out)
    }
}

impl drover::runner::Runner for SlowShed<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        self.run_in(prog, args, "")
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<drover::runner::Out> {
        self.slow(prog, args)
            .map_or_else(|| self.0.run_in(prog, args, input), Ok)
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
}

#[test]
fn a_slow_shed_listing_that_answers_is_read_not_unreadable() {
    let (c, _sh) = setup("slow-listing");
    remote_spawn(&c);
    let (f, bazzite) = (mini_only(), FakeShed::new());
    bazzite.lane(IMPL, "idle", WT);
    let over = with_bazzite(&f, &bazzite);
    let r = SlowShed(&over, std::sync::atomic::AtomicUsize::new(1));
    let (code, out) = run(&c, &r, &["waiting", "--json"]);
    assert_eq!(code, 0, "{out}");
    let w: serde_json::Value = serde_json::from_str(&out).unwrap();
    let row = w
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["issue"] == 12)
        .unwrap()
        .to_string();
    assert!(!row.contains("lane unknown"), "{row}");
    assert!(!row.contains("host offline"), "{row}");
    assert_eq!(
        r.1.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "the slow listing was sent: {:?}",
        f.calls.borrow()
    );
    assert!(!drover::hosts::down(&c).contains_key("bazzite"));
}

#[test]
fn a_start_held_by_a_slow_probes_offline_mark_goes_ahead_in_the_same_pass() {
    let (c, _sh) = setup("slow-probe-extend");
    queue_linux(&c);
    let ev13 = |ts: u64, kind: &str, d: serde_json::Value| {
        append(&c.state, &Event::new(ts, "sendmeter", Some(13), kind, d)).unwrap();
    };
    ev13(NOW - 3600, "round-extended", json!({"by": "owner", "queued": true}));
    let slow = mini_only()
        .once_err(PROBE, 124, "no answer")
        .once_err(PROBE, 124, "no answer");
    assert_eq!(run(&c, &slow, &["check"]).0, 0);
    assert!(
        drover::hosts::down(&c).contains_key("bazzite"),
        "the slow probe marked it down"
    );

    let (f, bazzite) = (linux_start(mini_only()), FakeShed::new());
    let (code, out) = run(&c, &with_bazzite(&f, &bazzite), &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_ne!(last_code(&c), "HOST_OFFLINE", "{out}");
    assert!(bazzite.has("sendmeter-impl-13"), "{out} {:?}", bazzite.methods());
    assert!(!drover::hosts::down(&c).contains_key("bazzite"));
    let p = drover::pending::of(&read(&c.state).unwrap(), "sendmeter", 13);
    assert!(p.is_none(), "{p:?}");
}

struct SlowHandshake<'a>(&'a dyn drover::runner::Runner);

impl drover::runner::Runner for SlowHandshake<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        self.0.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<drover::runner::Out> {
        self.0.run_in(prog, args, input)
    }
    fn run_for(
        &self,
        prog: &str,
        args: &[String],
        input: &str,
        limit: std::time::Duration,
    ) -> anyhow::Result<drover::runner::Out> {
        let fresh = format!("{prog} {}", args.join(" ")) == PROBE;
        if fresh && limit < std::time::Duration::from_secs(6) {
            return Ok(timed_out());
        }
        self.0.run_for(prog, args, input, limit)
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
}

#[test]
fn a_host_down_for_hours_whose_handshake_takes_6_s_is_up_after_one_refresh_and_its_held_start_goes_on() {
    let (c, _sh) = setup("offline-hours");
    queue_linux(&c);
    let at = |k: u64| drover::Ctx {
        now: NOW + k,
        ..c.clone()
    };
    for k in [0, 600, 3600] {
        let (f, bazzite) = (down(), FakeShed::new());
        assert_eq!(run(&at(k), &with_bazzite(&f, &bazzite), &["check"]).0, 0);
        assert_eq!(run(&at(k + 30), &with_bazzite(&f, &bazzite), &["drive"]).0, 0);
    }
    assert_eq!(last_code(&c), "HOST_OFFLINE");
    let later = at(9 * 3600);
    let f = linux_start(mini_only())
        .reply(&format!("{MUX} 'sh' '-lc'"), "")
        .reply(PROBE, "");
    let bazzite = FakeShed::new();
    let over = with_bazzite(&f, &bazzite);
    let up = SlowHandshake(&over);
    assert_eq!(run(&later, &up, &["hosts", "on", "bazzite"]).0, 0);
    let (code, out) = run(&later, &up, &["hosts", "refresh"]);
    assert_eq!(code, 0, "{out}");
    let row = out.lines().find(|l| l.starts_with("bazzite")).unwrap_or_default();
    assert!(!row.is_empty() && !row.contains("offline"), "{out}");
    assert_eq!(run(&later, &up, &["check"]).0, 0);
    let (code, out) = run(&at(9 * 3600 + 30), &up, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_ne!(last_code(&c), "HOST_OFFLINE", "{out} {:?}", f.calls.borrow());
    assert!(bazzite.has("sendmeter-impl-13"), "{out} {:?}", bazzite.methods());
    assert!(!drover::hosts::down(&c).contains_key("bazzite"));
}

#[test]
fn hosts_refresh_prints_an_offline_hosts_probe_exit_code_and_stderr() {
    let (c, _sh) = setup("offline-why");
    let refused = drover::runner::Out {
        code: 255,
        stderr: "debug1: reading config\nssh: connect to host bazzite port 22: Connection refused\n".into(),
        ..Default::default()
    };
    let mut f = mini_only();
    f.replies.insert(0, (PROBE.into(), refused.clone()));
    f.replies.insert(0, (MUX.into(), refused));
    let (code, out) = run(&c, &f, &["hosts", "refresh"]);
    assert_eq!(code, 0, "{out}");
    let row = out.lines().find(|l| l.starts_with("bazzite")).unwrap_or_default();
    assert!(row.contains("offline"), "{out}");
    let why = format!("probe `{PROBE}` exit 255: ssh: connect to host bazzite port 22: Connection refused");
    assert!(row.contains(&why), "{out}");
    assert!(drover::hosts::down(&c).contains_key("bazzite"));
}

#[test]
fn a_loaded_host_whose_frames_turned_late_keeps_its_lanes() {
    let (c, _sh) = setup("offline-loaded");
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "bazzite": {"tags": ["os:linux"], "home": "/home/me", "probed": NOW, "seen": NOW - 3600, "up": false},
    });
    std::fs::write(c.state.parent().unwrap().join("hosts.json"), cache.to_string()).unwrap();
    let row = drover::link::view::HostSeen {
        host: "bazzite".into(),
        link: "down".into(),
        since: NOW - 3600,
        heard: Some(NOW - 600),
        sample: Some(drover::link::frame::Sample {
            cpu_idle: Some(2.0),
            pressure: None,
            at: NOW - 600,
        }),
        ..Default::default()
    };
    drover::db::view_put(&c.state, &row, None).unwrap();
    remote_spawn(&c);
    let (code, out) = run(&c, &mini_only(), &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(kinds(&c, "lane-moved").is_empty(), "{out}");
}
