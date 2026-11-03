//! #337: the board's speed on a fixture of 30 lanes on 2 hosts (mini, this host, and bazzite, remote), every remote
//! call 300 ms slow: a step makes at most one remote call per host, and the page, the scene data, `status` and the
//! first paint in WebKit stay under their targets (hard: a miss fails). The numbers go to `$GITHUB_STEP_SUMMARY`.
mod common;
use common::*;
use herdsman::events::{append, Event};
use herdsman::runner::{Out, Runner};
use serde_json::json;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const NOW: u64 = 1_000_000;
const ADDR: &str = "127.0.0.1:7777";

/// The fixture's runner: a remote call (ssh, `herdr --machine`) sleeps 300 ms and is counted by host.
struct Slow(Fake, Mutex<Vec<String>>);
impl Runner for Slow {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        let host = match prog {
            "ssh" => args.iter().rev().nth(2).cloned(),
            "herdr" if args.first().is_some_and(|a| a == "--machine") => args.get(1).cloned(),
            _ => None,
        };
        if let Some(h) = host {
            std::thread::sleep(Duration::from_millis(300));
            self.1.lock().unwrap().push(h);
        }
        self.0.run(prog, args)
    }
    fn sleep(&self, _: u64) {}
}

/// 30 impl lanes, #1–#15 on mini and #16–#30 on bazzite, each idle with a pane and an unpushed worktree.
fn fixture() -> (herdsman::Ctx, Slow) {
    let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    let cfg = format!("{base}\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n");
    let c = ctx(&tmpdir("perf"), &cfg);
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let seen = json!({"mini": {"home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
                      "bazzite": {"home": "/home/me", "probed": NOW, "seen": NOW, "up": true}});
    std::fs::write(st.join("hosts.json"), seen.to_string()).unwrap();
    let (mut mini, mut bazzite) = (vec![], vec![]);
    for n in 1..=30u64 {
        let remote = n > 15;
        let wt = if remote {
            format!("/home/me/.herdr/worktrees/sendmeter/issue-{n}")
        } else {
            c.worktrees
                .join(format!("sendmeter/issue-{n}"))
                .to_string_lossy()
                .into()
        };
        let agent = format!("sendmeter-impl-{n}");
        let mut d = json!({"role": "impl", "agent": agent, "harness": "claude", "pane": format!("w{n}:p1"),
                           "workspace": format!("w{n}"), "worktree": wt, "branch": format!("issue-{n}")});
        if remote {
            (d["host"], d["machine"]) = (json!("bazzite"), json!("bazzite"));
        }
        append(&c.state, &Event::new(NOW - 900, "sendmeter", Some(n), "spawn", d)).unwrap();
        let a = json!({"name": agent, "agent_status": "idle", "pane_id": format!("w{n}:p1"), "cwd": wt});
        if remote { &mut bazzite } else { &mut mini }.push(a);
    }
    let list = |a: &[serde_json::Value]| json!({"result": {"agents": a}}).to_string();
    let f = Fake::default()
        .reply("herdr agent list", &list(&mini))
        .reply("herdr --machine bazzite agent list", &list(&bazzite))
        .reply("herdr workspace list", r#"{"result":{"workspaces":[]}}"#)
        .reply("herdr pane read", "⏺ Working on it\n\n❯ \n")
        .reply("herdr --machine bazzite pane read", "⏺ Working on it\n\n❯ \n")
        .reply("git -C", &format!("{} {}\n", "a".repeat(40), NOW - 600))
        .reply("ssh", &format!("{} {}\n", "a".repeat(40), NOW - 600));
    (c, Slow(f, Mutex::default()))
}

fn ms(t: Instant) -> u128 {
    t.elapsed().as_millis()
}

fn get(s: &herdsman::serve::Server, path: &str) -> (u128, usize) {
    let t = Instant::now();
    let res = s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), NOW);
    let text = String::from_utf8_lossy(&res);
    assert!(
        text.starts_with("HTTP/1.1 200"),
        "{path}: {}",
        &text[..text.len().min(300)]
    );
    (ms(t), res.len())
}

/// Appends `line` to the job summary on CI (the numbers on the PR's checks page), and prints it.
fn report(line: &str) {
    eprintln!("{line}");
    if let Ok(p) = std::env::var("GITHUB_STEP_SUMMARY") {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).create(true).open(p).unwrap();
        writeln!(f, "{line}").unwrap();
    }
}

#[test]
fn a_step_makes_one_remote_call_per_host_and_requests_make_none() {
    let (c, r) = fixture();
    let t = Instant::now();
    let (code, out) = run(&c, &r, &["drive", "--fast"]);
    let step = ms(t);
    assert_eq!(code, 0, "{out}");
    let calls = std::mem::take(&mut *r.1.lock().unwrap());
    assert!(calls.len() <= 1, "remote calls in one step: {calls:?}");
    assert!(herdsman::snapshot::load(&c).is_some_and(|s| s.reads.keys().any(|k| k.contains("issue-20"))));
    let s = herdsman::serve::Server::new(&c, &r, Some(ADDR.into()));
    let (page, size) = get(&s, "/");
    let (herd, _) = get(&s, "/herd.json");
    let (frags, _) = get(&s, "/frags?tab=lanes");
    let t = Instant::now();
    let (code, out) = run(&c, &r, &["status", "sendmeter"]);
    let status = ms(t);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("as of the step's look"), "{out}");
    let calls = r.1.lock().unwrap().clone();
    assert!(calls.is_empty(), "remote calls on a request or status: {calls:?}");
    report(&format!(
        "#337 perf (30 lanes, 2 hosts, +300 ms per remote call): step {step} ms · GET / {page} ms ({} KB) · \
         /herd.json {herd} ms · /frags?tab=lanes {frags} ms · status {status} ms",
        size / 1024
    ));
    assert!(page < 150, "GET / took {page} ms (target < 150)");
    assert!(herd < 100, "/herd.json took {herd} ms (target < 100)");
    assert!(status < 300, "status took {status} ms (target < 300)");
    assert!(size < 40 * 1024, "GET / is {size} bytes (target < 40 KB)");
}

/// #337: only what decides something is read: a recorded verdict's file never again, a merged lane's worktree never.
#[test]
fn a_recorded_verdict_and_a_merged_lane_are_never_read() {
    let (c, r) = fixture();
    let rev = "/home/me/.herdr/worktrees/sendmeter/rev-16-r1";
    let d = json!({"role": "rev", "agent": "sendmeter-rev-16-r1", "harness": "claude", "pane": "w40:p1", "workspace": "w40",
                   "worktree": rev, "branch": "rev-16-r1", "sha": "b".repeat(40), "host": "bazzite", "machine": "bazzite"});
    append(&c.state, &Event::new(NOW - 800, "sendmeter", Some(16), "spawn", d)).unwrap();
    let v = json!({"verdict": "FAIL", "sha": "b".repeat(40), "branch": "issue-16"});
    append(&c.state, &Event::new(NOW - 700, "sendmeter", Some(16), "verdict", v)).unwrap();
    append(
        &c.state,
        &Event::new(NOW - 600, "sendmeter", Some(17), "merged", json!({})),
    )
    .unwrap();
    assert_eq!(run(&c, &r, &["drive", "--fast"]).0, 0);
    let keys: Vec<String> = herdsman::snapshot::load(&c).unwrap().reads.into_keys().collect();
    assert!(keys.iter().any(|k| k.contains("issue-18")), "{keys:?}");
    assert!(!keys.iter().any(|k| k.contains("rev-16-r1/verdict.md")), "{keys:?}");
    assert!(!keys.iter().any(|k| k.contains("issue-17'")), "{keys:?}");
}

/// #337: the page carries the open tab only (the `tab` cookie, else Queue); `/frags?tab=` is that tab and the needs cards.
#[test]
fn the_page_and_frags_carry_one_tab() {
    let (c, r) = fixture();
    assert_eq!(run(&c, &r, &["drive", "--fast"]).0, 0);
    let s = herdsman::serve::Server::new(&c, &r, Some(ADDR.into()));
    let body = |path: &str, extra: &str| {
        let res = s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}\r\n"), NOW);
        String::from_utf8_lossy(&res).into_owned()
    };
    let page = body("/", "");
    assert!(page.contains("<section id=lanes></section>") && !page.contains("<section id=queue></section>"));
    let page = body("/", "Cookie: tab=lanes\r\n");
    assert!(page.contains("sendmeter-impl-20") && page.contains("<section id=queue></section>"));
    let j: serde_json::Value =
        serde_json::from_str(body("/frags?tab=host", "").split_once("\r\n\r\n").unwrap().1).unwrap();
    let ids: Vec<&String> = j.as_object().unwrap().keys().collect();
    assert_eq!(ids, ["host", "needs"]);
}

/// #337: the first contentful paint in WebKit (warm: the second load in one WebKit, its caches and service worker up;
/// the cold one, a new WebKit process, is reported) of the page a real board serves from the snapshot, under 300 ms. Runs where WebKit is at hand (the macOS CI job).
#[test]
fn the_first_paint_in_webkit_is_under_300ms() {
    if std::process::Command::new("swift").arg("--version").output().is_err() || !cfg!(target_os = "macos") {
        eprintln!("skipping: no swift/WebKit here");
        return;
    }
    let (c, r) = fixture();
    assert_eq!(run(&c, &r, &["drive", "--fast"]).0, 0);
    let (c, r): (&'static herdsman::Ctx, &'static Slow) = (Box::leak(Box::new(c)), Box::leak(Box::new(r)));
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/", l.local_addr().unwrap());
    std::thread::spawn(move || herdsman::serve::serve_on(c, r, l, false));
    let file = tmpdir("perf-fcp").join("page.png");
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/webview.swift");
    // Read after the shot's wait (`window.late`): the entry lands once the frame has painted.
    let fcp = "Object.defineProperty(window, 'late', {get: () => [(performance.getEntriesByName('first-contentful-paint')[0] || {startTime: -1}).startTime]})";
    let paint = |warm: bool| {
        let mut sw = swift();
        if warm {
            sw.env("HERDSMAN_WARM", "1");
        }
        let o = sw
            .args([script, &base, "390", "-", "horse"])
            .arg(&file)
            .arg(fcp)
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&o.stdout).into_owned();
        let v: Vec<f64> = out.split_whitespace().filter_map(|n| n.parse().ok()).collect();
        assert!(
            o.status.success() && v.len() == 3 && v[2] >= 0.0,
            "{out} {}",
            String::from_utf8_lossy(&o.stderr)
        );
        v[2]
    };
    let (cold, warm) = (paint(false), paint(true));
    report(&format!(
        "#337 perf: first contentful paint in WebKit {warm:.0} ms warm ({cold:.0} ms cold)"
    ));
    assert!(warm < 300.0, "first contentful paint {warm:.0} ms (target < 300)");
}
