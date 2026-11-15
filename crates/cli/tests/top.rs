mod common;
use common::*;
use drover::events::{append, Event};
use serde_json::{json, Value};

const PS: &str = "ps -axo pid=,ppid=,pcpu=,etime=,command=";

fn ps() -> String {
    "  101    50  12.5    05:00 /Users/me/.cargo/bin/cargo test --all
  102   101  85.0 01:02:03 /Users/me/.rustup/bin/rustc --crate-name meter
  103     1  40.0    10:00 /opt/homebrew/bin/node server.js
  104     1  30.0    10:00 /usr/bin/python3 job.py
  105     1  20.0    10:00 /usr/bin/ruby gem.rb
  106     1   1.0    10:00 /bin/zsh -l
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
        .reply("sysctl -n vm.loadavg", "{ 6.00 5.00 4.00 }\n")
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
        "load/cpu 3.00 (limit 2.5) · heavy slots 1/2 · respawns held 1\n",
        "sendmeter-impl-12 (sendmeter:12:impl)\n",
        "  101 12% 5m /Users/me/.cargo/bin/cargo test --all [cargo]\n",
        "    102 85% 62m /Users/me/.rustup/bin/rustc --crate-name meter\n",
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
    assert_eq!(v["load_per_cpu"], 3.0);
    assert_eq!(
        (v["limit"].clone(), v["heavy_used"].clone(), v["heavy_max"].clone()),
        (json!(2.5), json!(1), json!(2))
    );
    assert_eq!(v["respawns_held"], 1);
    assert_eq!(v["lanes"].as_array().unwrap().len(), 1, "{v}");
    let l = &v["lanes"][0];
    assert_eq!(
        (l["agent"].as_str(), l["lane"].as_str()),
        (Some("sendmeter-impl-12"), Some("sendmeter:12:impl"))
    );
    let p: Vec<(&str, u64, f64, u64, Value)> = (l["procs"].as_array().unwrap().iter())
        .map(|p| {
            let k = p["kind"].clone();
            (
                p["pid"].as_str().unwrap(),
                p["depth"].as_u64().unwrap(),
                p["cpu"].as_f64().unwrap(),
                p["secs"].as_u64().unwrap(),
                k,
            )
        })
        .collect();
    assert_eq!(
        p,
        [
            ("101", 0, 12.5, 300, json!("cargo")),
            ("102", 1, 85.0, 3723, Value::Null)
        ]
    );
    assert_eq!(l["waits"], json!([{"kind": "lint", "cmd": "sh lint.sh", "secs": 240}]));
}

fn history(c: &drover::Ctx, rows: &[(u64, f64)]) {
    let p = c.state.with_file_name("host-history.jsonl");
    let mut text = std::fs::read_to_string(&p).unwrap_or_default();
    for (ts, load) in rows {
        text += &format!("{}\n", json!({"ts": ts, "host": "mini", "load_cpu": load, "lanes": 1}));
    }
    std::fs::write(p, text).unwrap();
}

fn minutes(from: u64, to: u64, load: f64) -> Vec<(u64, f64)> {
    (from..=to).map(|m| (m * 60, load)).collect()
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
    history(&c, &minutes(1000, 1009, 3.0));
    c.now = at(1009);
    run(&c, &f, &["check"]);
    assert_eq!(overloads(&c).len(), 0, "9 minutes over is not yet an overload");
    history(&c, &minutes(1010, 1010, 3.0));
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
    assert!(why.starts_with("mini load/cpu 3.00 > 2.5 for 10m; top: 85% "), "{why}");
    for m in [1011, 1015] {
        history(&c, &minutes(m, m, 4.0));
        c.now = at(m);
        run(&c, &f, &["check"]);
    }
    assert_eq!(overloads(&c).len(), 1, "quiet while the overload lasts");
    history(&c, &minutes(1016, 1016, 2.0));
    history(&c, &minutes(1017, 1026, 3.0));
    c.now = at(1026);
    run(&c, &f, &["check"]);
    assert_eq!(overloads(&c).len(), 1, "a new overload waits its own 10 minutes");
    history(&c, &minutes(1027, 1027, 3.0));
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
    history(&c, &minutes(1000, 1003, 3.0));
    c.now = 1003 * 60;
    run(&c, &f, &["check"]);
    assert_eq!(overloads(&c).len(), 1);
}
