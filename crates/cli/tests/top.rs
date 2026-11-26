mod common;
use common::*;
use drover::events::{append, Event};
use serde_json::{json, Value};

const PS: &str = "ps -axo pid=,ppid=,pcpu=,etime=,pri=,nice=,command=";

fn ps() -> String {
    "  101    50  12.5    05:00  4  0 /Users/me/.cargo/bin/cargo test --all
  102   101  85.0 01:02:03 31  0 /Users/me/.rustup/bin/rustc --crate-name meter
  103     1  40.0    10:00 31  0 /opt/homebrew/bin/node server.js
  104     1  30.0    10:00 31  0 /usr/bin/python3 job.py
  105     1  20.0    10:00 31  0 /usr/bin/ruby gem.rb
  106     1   1.0    10:00 31  0 /bin/zsh -l
"
    .into()
}

fn setup(name: &str, limits: &str) -> (drover::Ctx, FakeShed, Fake, String) {
    let heavy = "\n[fleet.sendmeter.heavy]\ncargo = [\"cargo test*\"]\nlint = [\"sh lint.sh*\"]\n";
    let conf = CONFIG.replace("[limits]\n", &format!("[limits]\n{limits}"));
    let c = ctx(&tmpdir(name), &format!("{conf}{heavy}\n[hosts.mini]\nheavy_max = 2\n"));
    let wt = c.worktrees.join("sendmeter/issue-12").to_string_lossy().into_owned();
    let sh = FakeShed::attached(&c);
    sh.lane("sendmeter-impl-12", "working", &wt);
    sh.lane("sendmeter-impl-13", "working", "/elsewhere");
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "worktree": wt});
    append(&c.state, &Event::new(10, "sendmeter", Some(12), "spawn", d)).unwrap();
    let lsof = format!("p101\nccargo\nfcwd\nn{wt}\np102\ncrustc\nfcwd\nn{wt}/meter\np103\ncnode\nfcwd\nn/Users/me\n");
    let f = Fake::default()
        .reply(PS, &ps())
        .reply("lsof", &lsof)
        .reply(
            "iostat",
            " us sy id   1m   5m   15m\n 50 30 20  6 5 4\n 60 30 10  6 5 4\n",
        )
        .reply("sysctl -n hw.ncpu", "2\n");
    (c, sh, f, wt)
}

fn ticket(c: &drover::Ctx, n: u32, lane: &str, kind: &str, running: bool) -> std::fs::File {
    let dir = c.state.with_file_name("heavy");
    std::fs::create_dir_all(&dir).unwrap();
    let t = json!({"lane": lane, "kind": kind, "cmd": format!("sh {kind}.sh"), "since": c.now - 240,
                   "running": running, "kind_max": null});
    let p = dir.join(format!("{n:020}-1.json"));
    std::fs::write(&p, t.to_string()).unwrap();
    let f = std::fs::File::open(&p).unwrap();
    f.lock().unwrap();
    f
}

#[test]
fn top_lists_each_live_lane_with_its_process_tree_and_heavy_waits_in_text_and_json() {
    let (c, _sh, f, _) = setup("top-lanes", "");
    let _held = [
        ticket(&c, 1, "sendmeter:12:impl", "lint", false),
        ticket(&c, 2, "sendmeter:14:impl", "cargo", true),
    ];
    let pass = json!({"actions": [{"issue": 13, "step": "respawn", "code": "RESPAWN_PACED"},
                                  {"issue": 15, "step": "respawn", "code": null}]});
    append(&c.state, &Event::new(c.now - 30, "sendmeter", None, "drive-pass", pass)).unwrap();
    let (code, out) = run(&c, &f, &["top"]);
    assert_eq!(code, 0, "{out}");
    let want = [
        "cpu idle 10% (floor 15%) · heavy slots 1/2 · respawns held 1\n",
        "sendmeter-impl-12 (sendmeter:12:impl)\n",
        "  101 12% 5m /Users/me/.cargo/bin/cargo test --all [cargo]\n",
        "    102 85% 62m /Users/me/.rustup/bin/rustc --crate-name meter [normal priority]\n",
        "  waiting for heavy slot lint 4m: sh lint.sh\n",
    ];
    let mut at = 0;
    for w in want {
        let i = out[at..].find(w).unwrap_or_else(|| panic!("{w:?} in order: {out}"));
        at += i + w.len();
    }
    assert!(!out.contains("node"), "a process outside the lane: {out}");
    assert!(
        !out.contains("sendmeter-impl-13"),
        "an agent with no spawn is no lane: {out}"
    );

    let (code, out) = run(&c, &f, &["top", "--json"]);
    assert_eq!(code, 0, "{out}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["cpu_idle"], 10.0);
    assert_eq!(
        (v["limit"].clone(), v["heavy_used"].clone(), v["heavy_max"].clone()),
        (json!(15.0), json!(1), json!(2))
    );
    assert_eq!(v["respawns_held"], 1);
    assert_eq!(v["lanes"].as_array().unwrap().len(), 1, "{v}");
    let l = &v["lanes"][0];
    assert_eq!(
        (l["agent"].as_str(), l["lane"].as_str()),
        (Some("sendmeter-impl-12"), Some("sendmeter:12:impl"))
    );
    let p: Vec<(&str, u64, f64, u64, Value, bool)> = (l["procs"].as_array().unwrap().iter())
        .map(|p| {
            let k = p["kind"].clone();
            (
                p["pid"].as_str().unwrap(),
                p["depth"].as_u64().unwrap(),
                p["cpu"].as_f64().unwrap(),
                p["secs"].as_u64().unwrap(),
                k,
                p["background"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        p,
        [
            ("101", 0, 12.5, 300, json!("cargo"), true),
            ("102", 1, 85.0, 3723, Value::Null, false)
        ]
    );
    assert_eq!(l["waits"], json!([{"kind": "lint", "cmd": "sh lint.sh", "secs": 240}]));
}

#[test]
fn doctor_crosses_lane_priority_for_a_build_lane_process_at_normal_priority_and_ticks_it_once_all_are_background() {
    let (c, _sh, f, _) = setup("top-priority", "");
    let (_, out) = run(&c, &f, &["doctor"]);
    assert!(
        out.contains("✗ lane priority — 1 lane processes at normal priority (sendmeter-impl-12, 102)\n"),
        "{out}"
    );
    let low = ps().replace("01:02:03 31  0", "01:02:03 20 10");
    let mut f = f;
    f.replies.retain(|r| r.0 != PS);
    let f = f.reply(PS, &low);
    let (_, out) = run(&c, &f, &["doctor"]);
    assert!(out.contains("✓ lane priority\n"), "{out}");
    let (_, out) = run(&c, &f, &["top"]);
    assert!(!out.contains("[normal priority]"), "{out}");
}

fn history(c: &drover::Ctx, rows: &[(u64, f64)]) {
    let p = c.state.with_file_name("host-history.jsonl");
    let mut text = std::fs::read_to_string(&p).unwrap_or_default();
    for (ts, idle) in rows {
        text += &format!("{}\n", json!({"ts": ts, "host": "mini", "cpu_idle": idle, "lanes": 1}));
    }
    std::fs::write(p, text).unwrap();
}

fn minutes(from: u64, to: u64, idle: f64) -> Vec<(u64, f64)> {
    (from..=to).map(|m| (m * 60, idle)).collect()
}

fn overloads(c: &drover::Ctx) -> Vec<Value> {
    let log = drover::events::jsonl(&c.state).unwrap();
    let all = log.lines().map(|l| serde_json::from_str::<Value>(l).unwrap());
    all.filter(|e| e["type"] == "host-overloaded").collect()
}

#[test]
fn ten_minutes_over_the_limit_alerts_once_with_the_top_five_and_again_after_load_drops() {
    let (mut c, _sh, f, _) = setup("top-overload", "");
    let at = |m: u64| m * 60;
    history(&c, &minutes(1000, 1009, 10.0));
    c.now = at(1009);
    run(&c, &f, &["check"]);
    assert_eq!(overloads(&c).len(), 0, "9 minutes over is not yet an overload");
    history(&c, &minutes(1010, 1010, 10.0));
    c.now = at(1010);
    run(&c, &f, &["check"]);
    let all = overloads(&c);
    assert_eq!(all.len(), 1, "{all:?}");
    let top: Vec<&str> = all[0]["top"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    assert_eq!(
        top,
        [
            "85% /Users/me/.rustup/bin/rustc --crate-name meter (lane sendmeter-impl-12)",
            "40% /opt/homebrew/bin/node server.js (other)",
            "30% /usr/bin/python3 job.py (other)",
            "20% /usr/bin/ruby gem.rb (other)",
            "12% /Users/me/.cargo/bin/cargo test --all (lane sendmeter-impl-12)",
        ]
    );
    let why = all[0]["reason"].as_str().unwrap();
    assert!(why.starts_with("mini cpu idle 10% < 15% for 10m; top: 85% "), "{why}");
    for m in [1011, 1015] {
        history(&c, &minutes(m, m, 8.0));
        c.now = at(m);
        run(&c, &f, &["check"]);
    }
    assert_eq!(overloads(&c).len(), 1, "quiet while the overload lasts");
    history(&c, &minutes(1016, 1016, 50.0));
    history(&c, &minutes(1017, 1026, 10.0));
    c.now = at(1026);
    run(&c, &f, &["check"]);
    assert_eq!(overloads(&c).len(), 1, "a new overload waits its own 10 minutes");
    history(&c, &minutes(1027, 1027, 10.0));
    c.now = at(1027);
    run(&c, &f, &["check"]);
    let all = overloads(&c);
    assert_eq!(
        all.len(),
        2,
        "back under the limit, the next overload alerts again: {all:?}"
    );
    assert_eq!(all[1]["key"], at(1017));
}

#[test]
fn overload_minutes_comes_from_the_config() {
    let (mut c, _sh, f, _) = setup("top-overload-cfg", "overload_minutes = 3\n");
    history(&c, &minutes(1000, 1003, 10.0));
    c.now = 1003 * 60;
    run(&c, &f, &["check"]);
    assert_eq!(overloads(&c).len(), 1);
}

#[test]
fn a_missing_tool_names_itself_on_the_cpu_line() {
    let (c, _sh, f, _) = setup("top-missing-tool", "");
    let f = f.once("iostat", 127, "");
    let (code, out) = run(&c, &f, &["top"]);
    assert_eq!(code, 0, "{out}");
    let first = out.lines().next().unwrap();
    assert!(
        first.starts_with("? cpu idle — iostat not found (floor 15%) · "),
        "{out}"
    );
}

struct Held(Option<std::process::Child>);

impl Drop for Held {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            drover_sys::kill_group(c.id());
            c.wait().unwrap();
        }
    }
}

#[test]
fn a_running_hold_shows_in_top_json_while_it_lasts() {
    use std::os::unix::process::CommandExt;
    let (c, _sh, f, _) = setup(
        "top-hold",
        "min_cpu_idle = -1.0\nmin_free_gb = 0.0\nmin_free_mem_gb = 0.0\n",
    );
    let dir = c.state.parent().unwrap().to_path_buf();
    let job = "for i in $(seq 200); do [ -f go ] && break; sleep 0.05; done; [ -f go ]";
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_drover"))
        .current_dir(&dir)
        .env("FLEET_CONFIG", &c.config)
        .env("FLEET_STATE", &c.state)
        .env("GITHUB_REPOSITORY", "acme/det")
        .args(["hold", "--wait", "1", "--", "sh", "-c", job])
        .process_group(0)
        .spawn()
        .unwrap();
    let mut held = Held(Some(child));
    let holds = || {
        let (code, out) = run(&c, &f, &["top", "--json"]);
        assert_eq!(code, 0, "{out}");
        serde_json::from_str::<Value>(&out).unwrap()["holds"].clone()
    };
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while holds()[0]["running"] != json!(true) {
        assert!(std::time::Instant::now() < end, "no running hold: {}", holds());
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let h = holds()[0].clone();
    assert_eq!((h["lane"].as_str(), h["kind"].as_str()), (Some("ci:det"), Some("ci")));
    std::fs::write(dir.join("go"), "").unwrap();
    assert!(held.0.take().unwrap().wait().unwrap().success());
    assert_eq!(holds(), json!([]));
}

#[test]
fn top_shows_a_routed_command_as_remote_with_its_tag() {
    let (c, _sh, f, wt) = setup("top-remote", "");
    let gate = format!("runners run --tag linux --dir {wt} --env GATEKEEPER_HEAD -- bash -c ./.gatekeeper/run check --pr-body target/gatekeeper/pr-body.md");
    let ps = format!("{}  107   101   0.5    02:00  4  0 /Users/me/.local/bin/runners run --tag linux --dir {wt} -- bash -c cargo test\n  108   101   0.5    02:00  4  0 /Users/me/.local/bin/{gate}\n", ps());
    let lsof = format!("p101\nccargo\nfcwd\nn{wt}\np107\ncrunners\nfcwd\nn{wt}\np108\ncrunners\nfcwd\nn{wt}\n");
    let f = Fake { replies: vec![], ..f }
        .reply(PS, &ps)
        .reply("lsof", &lsof)
        .reply(
            "iostat",
            " us sy id   1m   5m   15m\n 50 30 20  6 5 4\n 60 30 10  6 5 4\n",
        )
        .reply("sysctl -n hw.ncpu", "2\n");
    let (code, out) = run(&c, &f, &["top"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&format!("    107 0% 2m /Users/me/.local/bin/runners run --tag linux --dir {wt} -- bash -c cargo test [remote linux]\n")), "{out}");
    assert!(
        out.contains(&format!("    108 0% 2m /Users/me/.local/bin/{gate} [remote linux]\n")),
        "a routed gate check: {out}"
    );
    let (_, out) = run(&c, &f, &["top", "--json"]);
    let v: Value = serde_json::from_str(&out).unwrap();
    let procs = v["lanes"][0]["procs"].as_array().unwrap();
    assert_eq!(procs.iter().find(|p| p["pid"] == "107").unwrap()["remote"], "linux");
}

#[test]
fn top_shows_a_lanes_slot_held_here_with_why_its_route_fell_back() {
    let (c, _sh, f, _) = setup("top-here", "");
    let _held = ticket(&c, 1, "sendmeter:12:impl", "cargo", true);
    let p = c.state.with_file_name("heavy").join(format!("{:020}-1.json", 1));
    let mut t: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    t["here"] = json!("exit 75");
    std::fs::write(&p, t.to_string()).unwrap();
    let (code, out) = run(&c, &f, &["top"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("  heavy slot cargo here (exit 75) 4m: sh cargo.sh\n"),
        "{out}"
    );
    let (_, out) = run(&c, &f, &["top", "--json"]);
    let v: Value = serde_json::from_str(&out).unwrap();
    let held = json!([{"kind": "cargo", "cmd": "sh cargo.sh", "secs": 240, "here": "exit 75"}]);
    assert_eq!(v["lanes"][0]["held"], held);
}
