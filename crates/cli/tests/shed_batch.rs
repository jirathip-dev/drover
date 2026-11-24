mod common;
use common::*;
use drover::events::{append, Event};
use drover::hosts::On;
use drover::runner::{Out, Runner};
use drover::shed::proto::Method;
use drover::shed::{self, Wire};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

const NOW: u64 = 1_000_000;
const LANE: &str = "sendmeter-impl-12";

fn connects(f: &Fake, method: &str) -> usize {
    let want = format!("\"method\":\"{method}\"");
    let inputs = f.inputs.borrow();
    inputs
        .iter()
        .filter(|(l, i)| l.contains("'shed' 'connect'") && i.contains(&want))
        .count()
}

fn host() -> (FakeShed, Fake) {
    let b = FakeShed::new();
    b.lane(LANE, "working", "/w/issue-12");
    b.screen(LANE, "remote screen\n");
    (b, Fake::default())
}

fn independent() -> Vec<(Method, Value)> {
    vec![
        (Method::Ls, json!({})),
        (Method::Get, json!({"name": LANE})),
        (Method::Read, json!({"name": LANE, "lines": 5})),
    ]
}

#[test]
fn independent_calls_to_another_host_share_one_shed_connect_and_answer_as_single_calls() {
    let (b, f) = host();
    let over = OverSsh {
        f: &f,
        hosts: vec![("box", &b)],
    };
    let local = FakeShed::new();
    let base = shed::With(&over, local.shed());
    let on = On::new(&base, Some("box"));
    let qs = independent();
    let single: Vec<Value> = (qs.iter())
        .map(|(m, p)| shed::call(&on, &Wire::Connect, *m, p.clone()).unwrap())
        .collect();
    let before = f.calls.borrow().len();
    assert_eq!(
        before,
        qs.len(),
        "one shed connect per single call: {:?}",
        f.calls.borrow()
    );
    let batch = shed::calls(&on, &Wire::Connect, &qs).unwrap();
    let calls = f.calls.borrow()[before..].to_vec();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(calls[0].ends_with("box -- 'shed' 'connect'"), "{calls:?}");
    let batch: Vec<Value> = batch.into_iter().map(Result::unwrap).collect();
    assert_eq!(batch, single);
    assert_eq!(batch[2]["text"], json!("remote screen\n"));
    assert!(local.methods().is_empty(), "nothing went to this host's server");
}

#[test]
fn independent_calls_on_this_hosts_socket_answer_as_single_calls() {
    let (b, f) = host();
    let w = b.shed().wire;
    let qs = independent();
    let single: Vec<Value> = (qs.iter())
        .map(|(m, p)| shed::call(&f, &w, *m, p.clone()).unwrap())
        .collect();
    let batch: Vec<Value> = (shed::calls(&f, &w, &qs).unwrap().into_iter())
        .map(Result::unwrap)
        .collect();
    assert_eq!(batch, single);
    assert_eq!(b.methods(), ["ls", "get", "read", "ls", "get", "read"]);
}

#[test]
fn one_failed_request_in_a_batch_fails_only_that_call() {
    let (b, f) = host();
    let over = OverSsh {
        f: &f,
        hosts: vec![("box", &b)],
    };
    let base = shed::With(&over, FakeShed::new().shed());
    let on = On::new(&base, Some("box"));
    let qs = [
        (Method::Get, json!({"name": LANE})),
        (Method::Get, json!({"name": "nowhere"})),
        (Method::Read, json!({"name": LANE, "lines": 5})),
    ];
    let alone = shed::call(&on, &Wire::Connect, Method::Get, json!({"name": "nowhere"})).unwrap_err();
    assert!(format!("{alone:#}").starts_with("NO_SUCH_TERMINAL"), "{alone:#}");
    for w in [Wire::Connect, b.shed().wire] {
        let got = shed::calls(&on, &w, &qs).unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].as_ref().unwrap()["name"], json!(LANE));
        assert_eq!(format!("{:#}", got[1].as_ref().unwrap_err()), format!("{alone:#}"));
        assert_eq!(got[2].as_ref().unwrap()["text"], json!("remote screen\n"));
    }
}

#[test]
fn a_lost_link_fails_every_call_of_its_batch_as_a_single_call_fails() {
    let f = Fake::default().reply("ssh", "");
    let base = shed::With(&f, FakeShed::new().shed());
    let on = On::new(&base, Some("box"));
    let alone = shed::call(&on, &Wire::Connect, Method::Ls, json!({})).unwrap_err();
    let batch = shed::calls(&on, &Wire::Connect, &independent()).unwrap_err();
    assert_eq!(format!("{batch:#}"), format!("{alone:#}"));
}

struct Lag<'a>(OverSsh<'a>, AtomicUsize);

impl Lag<'_> {
    fn hop(&self, prog: &str) {
        if prog == "ssh" {
            self.1.fetch_add(1, Relaxed);
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Runner for Lag<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.hop(prog);
        self.0.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.hop(prog);
        self.0.run_in(prog, args, input)
    }
    fn sleep(&self, _: u64) {}
}

#[test]
fn a_drive_pass_reads_every_crashed_lanes_last_words_on_a_host_over_one_shed_connect() {
    let base = CONFIG.replace("harness  =  \"claude\"", "harness = \"claude\"");
    let cfg = format!("{base}\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n");
    let c = ctx(&tmpdir("shed-batch"), &cfg);
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let seen = json!({"mini": {"home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
                      "bazzite": {"home": "/home/me", "probed": NOW, "seen": NOW, "up": true}});
    std::fs::write(st.join("hosts.json"), seen.to_string()).unwrap();
    let bazzite = FakeShed::new();
    let crashed: Vec<String> = (16..=20).map(|n| format!("sendmeter-impl-{n}")).collect();
    for n in 16..=30u64 {
        let wt = format!("/home/me/.herdr/worktrees/sendmeter/issue-{n}");
        let agent = format!("sendmeter-impl-{n}");
        let d = json!({"role": "impl", "agent": agent, "harness": "claude", "pane": agent, "runtime": "shed",
                       "worktree": wt, "branch": format!("issue-{n}"), "host": "bazzite", "machine": "bazzite"});
        append(&c.state, &Event::new(NOW - 900, "sendmeter", Some(n), "spawn", d)).unwrap();
        bazzite.lane(&agent, "idle", &wt);
        bazzite.screen(&agent, &format!("⏺ Working on #{n}\n\n❯ \n"));
    }
    for a in &crashed {
        bazzite.set(a, |t| {
            (t["state"], t["exit"], t["reported"]) = (json!("exited"), json!(1), json!({"state": "working"}));
        });
    }
    let f = Fake::default()
        .reply("git -C", &format!("{} {}\n", "a".repeat(40), NOW - 600))
        .reply("ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m bazzite -- 'sh' '-c' 'd=", "")
        .reply("ssh", &format!("{} {}\n", "a".repeat(40), NOW - 600));
    let lag = Lag(
        OverSsh {
            f: &f,
            hosts: vec![("bazzite", &bazzite)],
        },
        AtomicUsize::new(0),
    );
    let local = FakeShed::new();
    let r = shed::With(&lag, local.shed());
    let t = Instant::now();
    let (code, out) = run(&c, &r, &["drive"]);
    let ms = t.elapsed().as_millis();
    assert_eq!(code, 0, "{out}");
    let (ssh, reads) = (lag.1.load(Relaxed), connects(&f, "read"));
    eprintln!(
        "#1487 perf (15 lanes on one remote host, 5 crashed, +100 ms per ssh session): drive pass {ms} ms, \
         {ssh} ssh sessions, {reads} shed connect reading last words"
    );
    let events = drover::events::read(&c.state).unwrap();
    for a in &crashed {
        let gone = events
            .iter()
            .rfind(|e| e.kind == "lane-state" && e.str("agent") == Some(a.as_str()))
            .unwrap_or_else(|| panic!("no lane-state for {a}"));
        assert_eq!(gone.str("state"), Some("gone"), "{a}");
        let n = a.trim_start_matches("sendmeter-impl-");
        assert_eq!(
            gone.str("said"),
            Some(format!("last output: ⏺ Working on #{n} | ❯").as_str()),
            "{a}"
        );
    }
    assert_eq!(reads, 1, "one shed connect reads every crashed lane's last words");
}
