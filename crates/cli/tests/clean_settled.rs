mod common;
use common::*;
use drover::events::{read, Event};
use serde_json::json;
use std::path::Path;
use std::time::{Duration, Instant};

const NOW: u64 = 1_000_000;

fn line(e: &Event) -> String {
    serde_json::to_string(e).unwrap() + "\n"
}

fn lane(n: u64, wt: &Path, host: Option<&str>) -> serde_json::Value {
    let mut d = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "pane": format!("sendmeter-impl-{n}"),
                       "runtime": "shed", "worktree": wt, "branch": format!("issue-{n}")});
    if let Some(h) = host {
        d["host"] = h.into();
        d["machine"] = h.into();
    }
    d
}

fn pushed(wts: impl Iterator<Item = std::path::PathBuf>) -> Fake {
    let f = wts.fold(Fake::default(), |f, w| {
        f.reply(&format!("git -C {} rev-list", w.display()), "0\n")
    });
    f.reply("git ", "")
}

#[test]
fn a_dry_run_over_2000_merged_lanes_works_only_the_100_not_settled_and_status_drops_the_settled() {
    let d = tmpdir("clean-settled");
    let c = ctx(&d, CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    let old = d.join("herdr").join("sendmeter");
    let mut log = String::new();
    for n in 1..=2000 {
        let settled = n <= 1900;
        let wt = if settled {
            old.clone()
        } else {
            c.worktrees.join("sendmeter")
        }
        .join(format!("issue-{n}"));
        std::fs::create_dir_all(&wt).unwrap();
        let data = lane(n, &wt, None);
        log += &line(&Event::new(NOW - 900, "sendmeter", Some(n), "spawn", data.clone()));
        log += &line(&Event::new(NOW - 800, "sendmeter", Some(n), "merged", json!({"pr": n})));
        if settled {
            let mut d = data;
            d["why"] = "off-root".into();
            log += &line(&Event::new(NOW - 700, "sendmeter", Some(n), "clean-settled", d));
        }
    }
    rewrite_log(&c.state, &log);
    let r = pushed((1901..=2000).map(|n| c.worktrees.join("sendmeter").join(format!("issue-{n}"))));
    let start = Instant::now();
    let (code, out) = run(&c, &r, &["clean", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(start.elapsed() < Duration::from_secs(5), "{:?}", start.elapsed());
    assert_eq!(out.matches("would remove ").count(), 100, "{out}");
    assert!(out.contains("this host: 100 lanes checked, 100 would remove"), "{out}");
    let root = old.parent().unwrap().display().to_string();
    assert!(
        out.contains(&format!("orphan root {root}: 1900 worktrees off-root;")),
        "{out}"
    );
    assert_eq!(out.matches(&root).count(), 1, "the old root is one line: {out}");
    let calls = r.calls.borrow();
    let settled: Vec<_> = calls.iter().filter(|l| l.contains(&root)).collect();
    assert!(settled.is_empty(), "no work for a settled lane: {settled:?}");
    drop(calls);
    let (code, status) = run(&c, &Fake::default(), &["status"]);
    assert_eq!(code, 0, "{status}");
    assert_eq!(status.matches("clean me").count(), 100, "{status}");
    assert!(!status.contains("sendmeter #1900 "), "{status}");
}

#[test]
fn an_off_root_lane_is_settled_once_listed_under_its_orphan_root_and_removed_with_orphans() {
    let d = tmpdir("clean-orphan-root");
    let root = d.join("worktrees");
    let toml = format!("{CONFIG}\n[hosts.mini]\nroots = [\"{}\"]\n", root.display());
    let c = ctx(&d, &toml);
    let sh = FakeShed::new();
    sh.attach(&c);
    let old = d.join("herdr").join("sendmeter");
    let mut log = String::new();
    for n in 1..=3 {
        let wt = old.join(format!("issue-{n}"));
        std::fs::create_dir_all(&wt).unwrap();
        log += &line(&Event::new(
            NOW - 900,
            "sendmeter",
            Some(n),
            "spawn",
            lane(n, &wt, None),
        ));
        log += &line(&Event::new(NOW - 800, "sendmeter", Some(n), "merged", json!({"pr": n})));
    }
    rewrite_log(&c.state, &log);
    let shown = old.parent().unwrap().display().to_string();
    let line = format!("orphan root {shown}: 3 worktrees off-root;");
    let r = pushed((1..=3).map(|n| old.join(format!("issue-{n}"))));
    let (code, out) = run(&c, &r, &["clean", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&line), "{out}");
    assert!(!out.contains("keep "), "no line per lane: {out}");
    let settled = |c: &drover::Ctx| {
        (read(&c.state).unwrap().iter())
            .filter(|e| e.kind == "clean-settled")
            .count()
    };
    assert_eq!(settled(&c), 3);
    let (_, status) = run(&c, &Fake::default(), &["status"]);
    assert!(!status.contains("clean me"), "{status}");
    let (code, out) = run(&c, &r, &["clean", "--yes", "--verbose"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&line), "still listed once: {out}");
    assert!(!out.contains("keep "), "a settled lane is not walked again: {out}");
    assert_eq!(settled(&c), 3, "marked once");
    assert!(
        !r.calls.borrow().iter().any(|l| l.contains(&shown)),
        "{:?}",
        r.calls.borrow()
    );
    let (code, out) = run(&c, &r, &["clean", "--orphans", "--yes"]);
    assert_eq!(code, 0, "{out}");
    for n in 1..=3 {
        let wt = old.join(format!("issue-{n}")).display().to_string();
        assert!(
            r.called(&format!("git -C {wt} status")),
            "its wip check runs first: {out}"
        );
        assert!(out.contains(&format!("removed {wt} ")), "{out}");
    }
    assert!(!out.contains("orphan root"), "{out}");
    let (_, out) = run(&c, &Fake::default(), &["clean"]);
    assert!(!out.contains(&shown), "{out}");
}

#[test]
fn a_host_off_and_down_is_one_summary_line_and_no_ssh_call() {
    let d = tmpdir("clean-host-off");
    let toml = format!("{CONFIG}\n[hosts.mini]\n\n[hosts.boat3]\nmachine = \"boat3\"\nenabled = false\n");
    let c = ctx(&d, &toml);
    let sh = FakeShed::new();
    sh.attach(&c);
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({"boat3": {"tags": [], "home": "/Users/me", "probed": NOW, "seen": NOW - 9000, "up": false}});
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    let mut log = String::new();
    for n in 1..=42 {
        let wt = Path::new("/Users/me/.local/share/drover/work/sendmeter").join(format!("issue-{n}"));
        log += &line(&Event::new(
            NOW - 900,
            "sendmeter",
            Some(n),
            "spawn",
            lane(n, &wt, Some("boat3")),
        ));
        log += &line(&Event::new(NOW - 800, "sendmeter", Some(n), "merged", json!({"pr": n})));
    }
    rewrite_log(&c.state, &log);
    let r = Fake::default().reply("git ", "");
    let (code, out) = run(&c, &r, &["clean"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("boat3: 42 lanes, host off — not checked"), "{out}");
    assert_eq!(out.matches("boat3").count(), 1, "{out}");
    let calls = r.calls.borrow();
    let ssh: Vec<_> = calls.iter().filter(|l| l.contains("boat3")).collect();
    assert!(ssh.is_empty(), "{ssh:?}");
    drop(calls);
    let (_, out) = run(&c, &r, &["clean", "--verbose"]);
    assert_eq!(out.matches("host unreachable (not cleaned)").count(), 42, "{out}");
}
